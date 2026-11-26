//! `McpServer`：一行请求 → 一行响应的纯函数分发，加上 stdio / 任意 `Read+Write` 的传输外壳。
//!
//! 线格式在 [`crate::protocol`]，反向能力在 [`crate::bridge`]；本模块只做「方法 → 注册表」这一段。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use lilyco_core::context::HostBridge;
use lilyco_core::executor;
use lilyco_core::progress::Progress;
use lilyco_core::registry::Registry;
use lilyco_core::AppError;

use crate::bridge::{Caps, McpBridge, PendingMap};
use crate::protocol::*;

/// 最小 MCP 服务器（stdio 传输）
pub struct McpServer {
    registry: Arc<Registry>,
}

/// 资源 URI 前缀：`lilyco://command/<命令名>`
const COMMAND_URI_PREFIX: &str = "lilyco://command/";

fn command_uri(name: &str) -> String {
    format!("{COMMAND_URI_PREFIX}{name}")
}

/// `tools/call` 结果里可以被翻译成 MCP image 内容块的图片。
///
/// 约定（lilyco-mcp 不依赖任何具体域 crate，只能认字段）：命令的返回值里若带
/// `images: [{ data_base64, mime_type }]`，就把它翻成一个 MCP image 内容块。
/// 图片既可能挂在结果顶层（一次问答生成的图），也可能挂在每条消息上
/// （会话转录里每一轮的图），所以两处都收：先顶层 `images`，再逐个
/// `messages[].images`。
///
/// 客户端（含 dsh 的 MCP 客户端）要求 base64 严格 canonical —— 这里去掉所有
/// 空白字符，正好满足。只放行客户端普遍支持的四种 MIME。
const IMAGE_MIMES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];
/// 一次调用最多带几张图（防止 Agent 上下文被图片灌爆）
const MAX_IMAGE_BLOCKS: usize = 8;

pub(crate) fn image_blocks(value: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    push_image_blocks(value.get("images"), &mut out);
    if let Some(messages) = value.get("messages").and_then(|v| v.as_array()) {
        for message in messages {
            push_image_blocks(message.get("images"), &mut out);
        }
    }
    out
}

/// 收一组 `{ data_base64, mime_type }`，能识别的就追加成 image 内容块。
fn push_image_blocks(images: Option<&serde_json::Value>, out: &mut Vec<serde_json::Value>) {
    let Some(items) = images.and_then(|v| v.as_array()) else {
        return;
    };
    for item in items {
        if out.len() >= MAX_IMAGE_BLOCKS {
            return;
        }
        let mime = item
            .get("mime_type")
            .or_else(|| item.get("mimeType"))
            .and_then(|v| v.as_str())
            .unwrap_or("image/png");
        if !IMAGE_MIMES.contains(&mime) {
            continue;
        }
        let Some(data) = item.get("data_base64").and_then(|v| v.as_str()) else {
            continue;
        };
        let compact: String = data.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.is_empty() {
            continue;
        }
        out.push(serde_json::json!({
            "type": "image",
            "data": compact,
            "mimeType": mime,
        }));
    }
}

/// 把已经变成 image 内容块的 base64 从文本块里删掉，只留 `file_id` 之类的元信息。
///
/// 递归找 `images[]` 里的 `data_base64`，浅层就够（返回结构固定），但递归更稳。
pub(crate) fn strip_inline_data(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(images) = map.get_mut("images") {
                if let Some(items) = images.as_array_mut() {
                    for item in items {
                        if let Some(object) = item.as_object_mut() {
                            object.remove("data_base64");
                        }
                    }
                }
            }
            for (_, child) in map.iter_mut() {
                strip_inline_data(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_inline_data(item);
            }
        }
        _ => {}
    }
}

impl McpServer {
    /// 从命令注册表创建服务器
    pub fn new(registry: Registry) -> Self {
        Self {
            registry: Arc::new(registry),
        }
    }

    /// 处理一行 JSON-RPC 请求，返回响应 JSON 字符串。
    ///
    /// 通知（无 `id` 的请求，如 `notifications/initialized`）返回 `None`。
    pub fn handle_line(&self, line: &str) -> Option<String> {
        self.handle_line_with_sink(line, &mut |_| {})
    }

    /// [`McpServer::handle_line`] 的流式版本。
    ///
    /// `tools/call` 执行期间产生的 `notifications/progress` 通过 `sink`
    /// 逐行回调（每行一个完整 JSON-RPC 通知），返回值仍是最终响应。
    /// 纯 [`McpServer::handle_line`] 等价于 sink 为空。
    pub fn handle_line_with_sink(&self, line: &str, sink: &mut dyn FnMut(&str)) -> Option<String> {
        Self::dispatch(&self.registry, line, sink, None)
    }

    /// 分发核心（纯函数）：一行客户端请求 → 最终响应；反向桥可选注入
    fn dispatch(
        registry: &Registry,
        line: &str,
        sink: &mut dyn FnMut(&str),
        bridge: Option<Arc<dyn HostBridge>>,
    ) -> Option<String> {
        let req: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return Some(error_response(None, ERROR_PARSE, "parse error")),
        };

        let id = req.get("id").cloned();
        let method = req
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or_default();

        let result = match method {
            "initialize" => initialize_response(req.get("params")),
            "ping" => Ok(serde_json::json!({})),
            "tools/list" => Self::tools_list(registry),
            "tools/call" => Self::tools_call(registry, &req, sink, bridge),
            "resources/list" => Self::resources_list(registry),
            "resources/templates/list" => Ok(serde_json::json!({ "resourceTemplates": [] })),
            "resources/read" => Self::resources_read(registry, &req),
            "prompts/list" => Ok(serde_json::json!({ "prompts": [] })),
            "notifications/initialized" => return None, // 通知无需响应
            _ => {
                // JSON-RPC 2.0：无 id 的请求是通知，绝不能回响应
                // （DSH 的 MCP 客户端会发 notifications/cancelled 等）
                return id.map(|i| {
                    error_response(
                        Some(&i),
                        ERROR_METHOD_NOT_FOUND,
                        &format!("method not found: {method}"),
                    )
                });
            }
        };

        match result {
            Ok(res) => Some(success_response(id.as_ref(), res)),
            Err((code, msg)) => Some(error_response(id.as_ref(), code, &msg)),
        }
    }

    /// 在任意 `Read + Write` 对上提供服务（内存测试 / 自定义传输均可用）
    ///
    /// 双向 JSON-RPC 分流：
    /// - 带 `method` 的行 = 客户端请求/通知 → `dispatch`；其中 `tools/call`
    ///   丢到 worker 线程（handler 可经宿主桥反向发起 `sampling/createMessage`
    ///   / `roots/list`，主循环继续读行以路由客户端响应），其余同步处理
    /// - 无 `method` 且有 `id` 的行 = 客户端对服务端反向请求（`srv-N`）的
    ///   响应 → 路由进 pending 表，唤醒等待中的 handler
    /// - 通知与 worker 输出经共享 writer 锁即时写出（尽力而为）；主循环
    ///   最终响应的写失败仍向上传播
    pub fn serve<R: BufRead, W: Write + Send + 'static>(
        &self,
        mut reader: R,
        writer: W,
    ) -> std::io::Result<()> {
        let writer: Arc<Mutex<dyn Write + Send>> = Arc::new(Mutex::new(writer));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let caps = Arc::new(Caps::default());
        // 在途的 tools/call worker。客户端关闭 stdin（EOF）后主循环会退出，
        // 但 worker 可能还没写完响应 —— 必须 join 它们，否则短连接客户端
        // （脚本、`echo … | server`、CI）会丢响应。长连接客户端不受影响。
        let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();

        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break; // EOF
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(req) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                let mut w = writer.lock().unwrap();
                writeln!(w, "{}", error_response(None, ERROR_PARSE, "parse error"))?;
                w.flush()?;
                continue;
            };

            if req.get("method").and_then(|m| m.as_str()).is_some() {
                let method = req["method"].as_str().unwrap_or_default();
                // initialize：探测客户端能力（门控反向采样 / roots）
                if method == "initialize" {
                    caps.sampling.store(
                        req["params"]["capabilities"]["sampling"].is_object(),
                        Ordering::Relaxed,
                    );
                    caps.roots.store(
                        req["params"]["capabilities"]["roots"].is_object(),
                        Ordering::Relaxed,
                    );
                }
                if method == "tools/call" && req.get("id").is_some() {
                    let registry = Arc::clone(&self.registry);
                    let w2 = Arc::clone(&writer);
                    let pending2 = Arc::clone(&pending);
                    let caps2 = Arc::clone(&caps);
                    workers.push(std::thread::spawn(move || {
                        let bridge: Arc<dyn HostBridge> = Arc::new(McpBridge {
                            writer: Arc::clone(&w2),
                            pending: pending2,
                            caps: caps2,
                            next_id: AtomicU64::new(1),
                        });
                        let mut sink = |notification: &str| {
                            let mut w = w2.lock().unwrap();
                            let _ = writeln!(w, "{notification}");
                            let _ = w.flush();
                        };
                        if let Some(resp) =
                            Self::dispatch(&registry, &req.to_string(), &mut sink, Some(bridge))
                        {
                            let mut w = w2.lock().unwrap();
                            let _ = writeln!(w, "{resp}");
                            let _ = w.flush();
                        }
                    }));
                } else {
                    let mut sink = |notification: &str| {
                        let mut w = writer.lock().unwrap();
                        let _ = writeln!(w, "{notification}");
                        let _ = w.flush();
                    };
                    if let Some(resp) = Self::dispatch(&self.registry, trimmed, &mut sink, None) {
                        let mut w = writer.lock().unwrap();
                        writeln!(w, "{resp}")?;
                        w.flush()?;
                    }
                }
            } else if req.get("id").is_some() {
                // 客户端对服务端反向请求（srv-N）的响应 → 路由给等待者
                if let Some(id) = req["id"].as_str().map(str::to_string) {
                    let tx = pending.lock().unwrap().remove(&id);
                    if let Some(tx) = tx {
                        let payload = match req.get("result") {
                            Some(r) => Ok(r.clone()),
                            None => Err(req["error"]["message"]
                                .as_str()
                                .unwrap_or("client error")
                                .to_string()),
                        };
                        let _ = tx.send(payload);
                    }
                }
            }
        }
        // stdin 已 EOF。等在途的 tools/call 写完响应再返回 —— 否则短连接
        // 客户端（脚本 / CI / `echo … | server`）会在 worker 落盘前丢响应。
        for w in workers {
            let _ = w.join();
        }
        Ok(())
    }

    /// 在 stdin/stdout 上提供服务（MCP 标准传输，供 Agent 直接 spawn）
    pub fn serve_stdio(&self) -> std::io::Result<()> {
        // StdoutLock 非 Send；Stdout 每次写内部加锁，可安全跨线程共享
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        self.serve(BufReader::new(stdin), stdout)
    }

    // ── 方法实现 ────────────────────────────────────────

    fn tools_list(registry: &Registry) -> Result<serde_json::Value, (i64, String)> {
        let tools: Vec<serde_json::Value> = registry
            .visible()
            .map(|cmd| {
                // 安全分级写进工具描述，Agent 在 tools/list 就能看到门槛
                let tier = cmd.schema.safety;
                let description = if tier == lilyco_core::safety::SafetyTier::ReadOnly {
                    cmd.schema.about.clone()
                } else {
                    format!("{} [safety: {}]", cmd.schema.about, tier.tag())
                };
                serde_json::json!({
                    "name": cmd.name,
                    "description": description,
                    "inputSchema": cmd.schema.to_json_schema(),
                })
            })
            .collect();
        Ok(serde_json::json!({ "tools": tools }))
    }

    /// `resources/list`：把每条可见命令当作一个只读资源。
    ///
    /// URI 形如 `lilyco://command/<name>`，内容是该命令的 schema（`resources/read`
    /// 回的就是 `--schema` 里那一条）。这样 Agent 不必先跑工具就能读到命令的
    /// 参数与安全分级，也让只声明 resources 的客户端（dsh 的 MCP 资源面板）有东西可列。
    fn resources_list(registry: &Registry) -> Result<serde_json::Value, (i64, String)> {
        let resources: Vec<serde_json::Value> = registry
            .visible()
            .map(|cmd| {
                let tier = cmd.schema.safety;
                let description = if tier == lilyco_core::safety::SafetyTier::ReadOnly {
                    cmd.schema.about.clone()
                } else {
                    format!("{} [safety: {}]", cmd.schema.about, tier.tag())
                };
                serde_json::json!({
                    "uri": command_uri(&cmd.name),
                    "name": cmd.name,
                    "description": description,
                    "mimeType": "application/json",
                })
            })
            .collect();
        Ok(serde_json::json!({ "resources": resources }))
    }

    /// `resources/read`：回某条命令的 schema（JSON 文本）。
    fn resources_read(
        registry: &Registry,
        req: &serde_json::Value,
    ) -> Result<serde_json::Value, (i64, String)> {
        let uri = req
            .get("params")
            .and_then(|p| p.get("uri"))
            .and_then(|u| u.as_str())
            .ok_or((ERROR_INVALID_PARAMS, "missing resource uri".to_string()))?;
        let name = uri.strip_prefix(COMMAND_URI_PREFIX).ok_or((
            ERROR_INVALID_PARAMS,
            format!("unsupported resource uri: {uri}（本服务只提供 {COMMAND_URI_PREFIX}<命令名>）"),
        ))?;
        let cmd = registry
            .get(name)
            .ok_or((ERROR_INVALID_PARAMS, format!("unknown resource: {uri}")))?;
        let text = serde_json::to_string_pretty(&serde_json::json!({
            "name": cmd.name,
            "about": cmd.schema.about,
            "safety": cmd.schema.safety.tag(),
            "inputSchema": cmd.schema.to_json_schema(),
        }))
        .unwrap_or_default();
        Ok(serde_json::json!({
            "contents": [{
                "uri": uri,
                "mimeType": "application/json",
                "text": text,
            }]
        }))
    }

    fn tools_call(
        registry: &Registry,
        req: &serde_json::Value,
        sink: &mut dyn FnMut(&str),
        bridge: Option<Arc<dyn HostBridge>>,
    ) -> Result<serde_json::Value, (i64, String)> {
        let params = req.get("params").cloned().unwrap_or(serde_json::json!({}));
        let name = params
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or((ERROR_INVALID_PARAMS, "missing tool name".to_string()))?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        let cmd = registry
            .get(name)
            .ok_or((ERROR_INVALID_PARAMS, format!("unknown tool: {name}")))?;

        // Agent 直传参数没有 CLI clap 兜底 —— 先做 schema 校验
        // （CommandSchema::validate_args，三端唯一校验实现）
        if let Err(e) = cmd.schema.validate_args(&args) {
            return Err((ERROR_INVALID_PARAMS, e.to_string()));
        }

        let handler = cmd
            .handler
            .clone()
            .ok_or((ERROR_INTERNAL, format!("tool `{name}` has no handler")))?;

        // 客户端在 _meta.progressToken 请求进度 → 流式执行，
        // Progress::Started/Tick 转发为 notifications/progress（2024-11-05）
        let progress_token = params
            .get("_meta")
            .and_then(|m| m.get("progressToken"))
            .cloned();

        let result: Result<serde_json::Value, AppError> = match progress_token {
            Some(token) => {
                let task = executor::spawn_with(handler, args, bridge);
                let mut terminal: Option<Result<serde_json::Value, String>> = None;
                for event in task.rx {
                    match &event {
                        Progress::Started { total, message } => {
                            sink(&progress_notification(&token, 0.0, *total, message.clone()));
                        }
                        Progress::Tick {
                            current,
                            total,
                            message,
                            ..
                        } => {
                            sink(&progress_notification(
                                &token,
                                *current as f64,
                                *total,
                                message.clone(),
                            ));
                        }
                        Progress::Done { result, .. } => {
                            terminal = Some(Ok(result.clone()));
                        }
                        Progress::Error { message, .. } => {
                            terminal = Some(Err(message.clone()));
                        }
                        Progress::Telemetry { key, value } => {
                            // 遥测复用已协商的 progress 通道（零 spec 风险）：
                            // 数据点转成 message 形如 "altitude=50"，Agent 实时可见
                            sink(&progress_notification(
                                &token,
                                0.0,
                                None,
                                Some(format!("{key}={value}")),
                            ));
                        }
                        Progress::View { id, component } => {
                            // 语义组件在 MCP 面转成一条可读进度消息：
                            // `id={"kind":"progress",...}`，Agent 也能消费结构
                            let body = serde_json::to_string(component).unwrap_or_default();
                            sink(&progress_notification(
                                &token,
                                0.0,
                                None,
                                Some(format!("{id}={body}")),
                            ));
                        }
                        Progress::Log { .. } => {} // 日志不映射为进度通知
                    }
                }
                // handler panic 时 channel 关闭且无终态事件 → join 兜底
                match terminal {
                    Some(r) => r.map_err(AppError::Runtime),
                    None => match task.handle.join() {
                        Ok(v) => v,
                        Err(panic) => {
                            Err(AppError::Runtime(format!("handler panicked: {panic:?}")))
                        }
                    },
                }
            }
            // 无进度请求 → 同步执行（原路径，零开销）
            None => executor::execute_with(handler, args, bridge).result,
        };

        match result {
            Ok(value) => {
                // 图片既要在 image 内容块里给出，就别再让同一份 base64 在文本块里
                // 重复一遍：一个 1.5MB 的 PNG 会变成 3MB 的上下文。
                let blocks = image_blocks(&value);
                let text = if blocks.is_empty() {
                    value.to_string()
                } else {
                    let mut stripped = value.clone();
                    strip_inline_data(&mut stripped);
                    stripped.to_string()
                };
                let mut content = vec![serde_json::json!({
                    "type": "text",
                    "text": text,
                })];
                content.extend(blocks);
                Ok(serde_json::json!({ "content": content, "isError": false }))
            }
            Err(e) => Ok(serde_json::json!({
                "content": [{ "type": "text", "text": e.to_string() }],
                "isError": true,
            })),
        }
    }
}
