//! # Lilyco MCP
//!
//! 把 Lilyco 命令注册表暴露为标准 **Model Context Protocol** (MCP) 服务器。
//! 实现 2024-11-05 协议子集：`initialize` / `ping` / `tools/list` / `tools/call`，
//! 以及进度通知（`tools/call` 携带 `_meta.progressToken` 时流式返回
//! `notifications/progress`）。
//!
//! 设计说明：
//! - 低耦合：只依赖 `lilyco-core`（registry + executor），不关心任何渲染端；
//!   新增一个 AI 后端 = 新增一个 crate，core 零改动。
//! - 可测试：核心逻辑是纯函数 [`McpServer::handle_line`]（一行请求 → 一行响应），
//!   [`McpServer::serve`] 可挂任意 `Read + Write` 对。
//! - 与官方 MCP SDK 的关系：这里手工实现最小 stdio 子集（零额外依赖），
//!   满足 Agent 直接调用；采样 / roots 等完整能力可另建 `lilyco-mcp-full`
//!   基于 modelcontextprotocol/rust-sdk 包装，core 无需变化。
//!
//! ## 使用
//!
//! ```ignore
//! let registry = ...; // lilyco_core::Registry
//! lilyco_mcp::McpServer::new(registry).serve_stdio()?;
//! ```

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lilyco_core::context::HostBridge;
use lilyco_core::executor;
use lilyco_core::progress::Progress;
use lilyco_core::registry::Registry;
use lilyco_core::AppError;

/// MCP 协议版本（2024-11-05）
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC 错误码
pub const ERROR_PARSE: i64 = -32700;
pub const ERROR_METHOD_NOT_FOUND: i64 = -32601;
pub const ERROR_INVALID_PARAMS: i64 = -32602;
pub const ERROR_INTERNAL: i64 = -32603;

/// 客户端能力（initialize 时探测，门控反向请求）
#[derive(Default)]
struct Caps {
    sampling: AtomicBool,
    roots: AtomicBool,
}

/// 服务端反向请求的 pending 表：id → 响应回传 sender
type PendingMap = Arc<Mutex<HashMap<String, mpsc::Sender<Result<serde_json::Value, String>>>>>;

/// 反向请求的响应等待上限（工具内嵌 LLM 调用可能较慢）
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(300);

/// 最小 MCP 服务器（stdio 传输）
pub struct McpServer {
    registry: Arc<Registry>,
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
            "initialize" => initialize_response(),
            "ping" => Ok(serde_json::json!({})),
            "tools/list" => Self::tools_list(registry),
            "tools/call" => Self::tools_call(registry, &req, sink, bridge),
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
                    std::thread::spawn(move || {
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
                    });
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
                serde_json::json!({
                    "name": cmd.name,
                    "description": cmd.schema.about,
                    "inputSchema": cmd.schema.to_json_schema(),
                })
            })
            .collect();
        Ok(serde_json::json!({ "tools": tools }))
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
            Ok(value) => Ok(serde_json::json!({
                "content": [{ "type": "text", "text": value.to_string() }],
                "isError": false,
            })),
            Err(e) => Ok(serde_json::json!({
                "content": [{ "type": "text", "text": e.to_string() }],
                "isError": true,
            })),
        }
    }
}

/// 构造一条 `notifications/progress`（MCP 2024-11-05）。
/// `total` / `message` 为 `None` 时省略字段。
fn progress_notification(
    token: &serde_json::Value,
    progress: f64,
    total: Option<u64>,
    message: Option<String>,
) -> String {
    let mut params = serde_json::Map::new();
    params.insert("progressToken".into(), token.clone());
    params.insert("progress".into(), serde_json::json!(progress));
    if let Some(t) = total {
        params.insert("total".into(), serde_json::json!(t));
    }
    if let Some(m) = message {
        params.insert("message".into(), serde_json::json!(m));
    }
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/progress",
        "params": params,
    })
    .to_string()
}

// ── 宿主桥（MCP 反向能力：采样 / roots） ──────────────────

/// 把 `sampling/createMessage` / `roots/list` 映射为 server → client
/// JSON-RPC 请求的宿主桥。
///
/// 协议规定采样与 roots 是**客户端**能力：客户端须在 initialize 的
/// capabilities 中声明，未声明时对应方法返回带指引的错误（人机协同：
/// 客户端保留模型访问与审批控制权）。
struct McpBridge {
    writer: Arc<Mutex<dyn Write + Send>>,
    pending: PendingMap,
    caps: Arc<Caps>,
    next_id: AtomicU64,
}

impl McpBridge {
    /// 发出反向请求并等待客户端响应（响应由 serve 主循环路由进 pending 表）。
    /// server 侧请求 id 用 `srv-N` 字符串，与客户端的数字 id 天然不冲突。
    fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        let id = format!("srv-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        {
            let mut w = self.writer.lock().unwrap();
            if writeln!(w, "{req}").and_then(|_| w.flush()).is_err() {
                self.pending
                    .lock()
                    .unwrap()
                    .remove(&format!("srv-{}", self.next_id.load(Ordering::Relaxed) - 1));
                return Err(AppError::Runtime(
                    "反向请求写出失败（客户端已断开？）".into(),
                ));
            }
        }

        match rx.recv_timeout(BRIDGE_TIMEOUT) {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(msg)) => Err(AppError::Runtime(format!("客户端拒绝 {method}: {msg}"))),
            Err(_) => Err(AppError::Runtime(format!(
                "{method} 响应超时（{BRIDGE_TIMEOUT:?}）"
            ))),
        }
    }
}

impl HostBridge for McpBridge {
    fn sample(&self, prompt: &str, max_tokens: u32) -> Result<String, AppError> {
        if !self.caps.sampling.load(Ordering::Relaxed) {
            return Err(AppError::Runtime(
                "MCP 客户端未声明 sampling 能力（initialize capabilities）".into(),
            ));
        }
        // 2024-11-05 规范：messages[{role, content{type,text}}] + maxTokens 必填
        let result = self.request(
            "sampling/createMessage",
            serde_json::json!({
                "messages": [
                    { "role": "user", "content": { "type": "text", "text": prompt } }
                ],
                "maxTokens": max_tokens,
            }),
        )?;
        match result["content"]["text"].as_str() {
            Some(text) => Ok(text.to_string()),
            None => Err(AppError::Runtime("sampling 响应缺少 content.text".into())),
        }
    }

    fn roots(&self) -> Result<Vec<(String, String)>, AppError> {
        if !self.caps.roots.load(Ordering::Relaxed) {
            return Err(AppError::Runtime(
                "MCP 客户端未声明 roots 能力（initialize capabilities）".into(),
            ));
        }
        let result = self.request("roots/list", serde_json::json!({}))?;
        let mut out = Vec::new();
        if let Some(roots) = result["roots"].as_array() {
            for r in roots {
                out.push((
                    r["uri"].as_str().unwrap_or_default().to_string(),
                    r["name"].as_str().unwrap_or_default().to_string(),
                ));
            }
        }
        Ok(out)
    }
}

// ── JSON-RPC 装配 ────────────────────────────────────────

fn initialize_response() -> Result<serde_json::Value, (i64, String)> {
    Ok(serde_json::json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "lilyco",
            "version": env!("CARGO_PKG_VERSION"),
        }
    }))
}

fn success_response(id: Option<&serde_json::Value>, result: serde_json::Value) -> String {
    let mut resp = serde_json::Map::new();
    resp.insert("jsonrpc".into(), serde_json::json!("2.0"));
    resp.insert("id".into(), id.cloned().unwrap_or(serde_json::Value::Null));
    resp.insert("result".into(), result);
    serde_json::to_string(&serde_json::Value::Object(resp)).unwrap_or_default()
}

fn error_response(id: Option<&serde_json::Value>, code: i64, message: &str) -> String {
    let mut resp = serde_json::Map::new();
    resp.insert("jsonrpc".into(), serde_json::json!("2.0"));
    resp.insert("id".into(), id.cloned().unwrap_or(serde_json::Value::Null));
    resp.insert(
        "error".into(),
        serde_json::json!({ "code": code, "message": message }),
    );
    serde_json::to_string(&serde_json::Value::Object(resp)).unwrap_or_default()
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco_core::registry::{RegisteredCommand, Registry};
    use lilyco_core::schema::{ArgKind, ArgSchema, CommandSchema};
    use lilyco_core::{App, AppError, Context};
    use std::collections::HashMap;

    /// 一个可执行的最小 App：echo
    struct Echo;

    impl App for Echo {
        fn schema() -> CommandSchema {
            CommandSchema {
                name: "echo".into(),
                about: "echo the given text".into(),
                args: vec![ArgSchema {
                    name: "text".into(),
                    about: "text to echo".into(),
                    kind: ArgKind::Text,
                    required: true,
                    default: None,
                }],
                subcommands: vec![],
            }
        }

        fn from_args(args: &HashMap<String, serde_json::Value>) -> Result<Self, AppError> {
            if !args.contains_key("text") {
                return Err(AppError::InvalidArg("missing text".into()));
            }
            Ok(Echo)
        }

        fn run(&self, ctx: &Context) -> Result<serde_json::Value, AppError> {
            let r = serde_json::json!({ "echoed": true });
            ctx.done(r.clone(), 0);
            Ok(r)
        }
    }

    fn test_registry() -> Registry {
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::from_app::<Echo>()).unwrap();
        reg
    }

    #[test]
    fn initialize_returns_protocol_version() {
        let server = McpServer::new(test_registry());
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(v["result"]["capabilities"]["tools"], serde_json::json!({}));
    }

    #[test]
    fn tools_list_lists_visible_commands() {
        let server = McpServer::new(test_registry());
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let tools = v["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "echo");
        assert_eq!(tools[0]["inputSchema"]["type"], "object");
    }

    #[test]
    fn tools_call_executes_handler() {
        let server = McpServer::new(test_registry());
        let req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hi"}}}"#;
        let resp = server.handle_line(req).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], false);
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("echoed"));
    }

    #[test]
    fn tools_call_unknown_tool_is_error() {
        let server = McpServer::new(test_registry());
        let req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope"}}"#;
        let resp = server.handle_line(req).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], ERROR_INVALID_PARAMS);
    }

    #[test]
    fn notification_gets_no_response() {
        let server = McpServer::new(test_registry());
        assert!(server
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());
    }

    #[test]
    fn unknown_notification_gets_no_response() {
        // JSON-RPC：通知（无 id）绝不响应——DSH 客户端会发 notifications/cancelled
        let server = McpServer::new(test_registry());
        assert!(server
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#)
            .is_none());
    }

    #[test]
    fn unknown_method_returns_error() {
        let server = McpServer::new(test_registry());
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":5,"method":"bogus"}"#)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], ERROR_METHOD_NOT_FOUND);
    }

    #[test]
    fn parse_error_returns_error() {
        let server = McpServer::new(test_registry());
        let resp = server.handle_line("not json at all").unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], ERROR_PARSE);
    }

    #[test]
    fn serve_roundtrips_over_memory_io() {
        let server = McpServer::new(test_registry());
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
        );
        let out = SharedBuf::default();
        server
            .serve(std::io::Cursor::new(input), out.clone())
            .unwrap();
        let s = out.string();
        assert!(s.contains("\"result\":{}"), "ping response: {s}");
        assert!(s.contains("\"tools\""), "tools/list response: {s}");
        // 两行请求 → 两行响应
        assert_eq!(s.lines().count(), 2);
    }

    #[test]
    fn serve_skips_blank_lines() {
        let server = McpServer::new(test_registry());
        let input = "\n\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n";
        let out = SharedBuf::default();
        server
            .serve(std::io::Cursor::new(input), out.clone())
            .unwrap();
        assert_eq!(out.string().lines().count(), 1);
    }

    // ─── 进度通知（notifications/progress） ────────────

    use std::sync::Arc;

    use lilyco_core::registry::Handler;

    /// 携带自定义 handler 的注册表：执行时上报两次 Tick + 一次 Done
    fn progress_registry() -> Registry {
        let mut reg = Registry::new();
        let handler: Handler = Arc::new(|ctx, _args| {
            ctx.tick(1, Some(2), "step 1");
            ctx.tick(2, Some(2), "step 2");
            let r = serde_json::json!({ "ok": true });
            ctx.done(r.clone(), 3);
            Ok(r)
        });
        let schema = CommandSchema {
            name: "progress".into(),
            about: "progress test".into(),
            args: vec![],
            subcommands: vec![],
        };
        reg.register(RegisteredCommand::new("progress", schema).with_handler(handler))
            .unwrap();
        reg
    }

    fn progress_call_req(token_json: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{{"_meta":{{"progressToken":{token_json}}},"name":"progress","arguments":{{}}}}}}"#
        )
    }

    #[test]
    fn progress_token_emits_notifications() {
        let server = McpServer::new(progress_registry());
        let mut notifications = Vec::new();
        let resp = server
            .handle_line_with_sink(&progress_call_req("\"tok-1\""), &mut |n| {
                notifications.push(n.to_string())
            })
            .unwrap();

        // 两次 Tick → 两条通知，内容与进度一一对应
        assert_eq!(notifications.len(), 2, "notifications: {notifications:?}");
        let first: serde_json::Value = serde_json::from_str(&notifications[0]).unwrap();
        assert_eq!(first["jsonrpc"], "2.0");
        assert_eq!(first["method"], "notifications/progress");
        assert_eq!(first["params"]["progressToken"], "tok-1");
        assert_eq!(first["params"]["progress"], 1.0);
        assert_eq!(first["params"]["total"], 2);
        assert_eq!(first["params"]["message"], "step 1");
        let second: serde_json::Value = serde_json::from_str(&notifications[1]).unwrap();
        assert_eq!(second["params"]["progress"], 2.0);
        assert_eq!(second["params"]["message"], "step 2");

        // 最终响应仍是合法 tools/call 结果
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["id"], 9);
        assert_eq!(v["result"]["isError"], false);
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("ok"));
    }

    #[test]
    fn progress_token_can_be_number() {
        let server = McpServer::new(progress_registry());
        let mut notifications = Vec::new();
        server
            .handle_line_with_sink(&progress_call_req("42"), &mut |n| {
                notifications.push(n.to_string())
            })
            .unwrap();
        let first: serde_json::Value = serde_json::from_str(&notifications[0]).unwrap();
        assert_eq!(first["params"]["progressToken"], 42);
    }

    #[test]
    fn no_progress_token_emits_no_notifications() {
        let server = McpServer::new(progress_registry());
        let mut notifications = Vec::new();
        let resp = server
            .handle_line_with_sink(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"progress","arguments":{}}}"#,
                &mut |n| notifications.push(n.to_string()),
            )
            .unwrap();
        assert!(notifications.is_empty(), "no token → no notifications");
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], false);
    }

    #[test]
    fn plain_handle_line_never_notifies() {
        // 兼容入口：handle_line 等价于空 sink
        let server = McpServer::new(progress_registry());
        let resp = server.handle_line(&progress_call_req("\"t\"")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], false);
    }

    #[test]
    fn error_tool_with_progress_still_returns_error_response() {
        let mut reg = Registry::new();
        let handler: Handler = Arc::new(|ctx, _args| {
            ctx.tick(1, None, "working");
            Err(AppError::Runtime("boom".into()))
        });
        let schema = CommandSchema {
            name: "fail".into(),
            about: "always fails".into(),
            args: vec![],
            subcommands: vec![],
        };
        reg.register(RegisteredCommand::new("fail", schema).with_handler(handler))
            .unwrap();

        let server = McpServer::new(reg);
        let mut notifications = Vec::new();
        let resp = server
            .handle_line_with_sink(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"t"},"name":"fail","arguments":{}}}"#,
                &mut |n| notifications.push(n.to_string()),
            )
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], true);
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("boom"));
    }

    #[test]
    fn serve_streams_notifications_before_response() {
        let server = McpServer::new(progress_registry());
        let input = format!("{}\n", progress_call_req("\"t\""));
        let out = SharedBuf::default();
        server
            .serve(std::io::Cursor::new(input), out.clone())
            .unwrap();
        // tools/call 在 worker 线程执行，serve 读到 EOF 即返回 —— 轮询等待全部输出
        out.wait_for("notifications/progress", Duration::from_secs(5));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let stdout = loop {
            let s = out.string();
            if s.lines().count() >= 3 {
                break s;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "expected 3 output lines, got: {s}"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), 3, "2 notifications + 1 response: {lines:?}");

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["method"], "notifications/progress");
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["method"], "notifications/progress");
        let last: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(last["result"]["isError"], false);
    }

    #[test]
    fn tools_call_missing_required_arg_is_invalid_params() {
        let server = McpServer::new(test_registry());
        let req = r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"echo","arguments":{}}}"#;
        let resp = server.handle_line(req).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], ERROR_INVALID_PARAMS);
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("缺少必填参数"),
            "schema validation error expected: {v}"
        );
    }

    #[test]
    fn tools_call_wrong_arg_type_is_invalid_params() {
        let server = McpServer::new(test_registry());
        let req = r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"echo","arguments":{"text":42}}}"#;
        let resp = server.handle_line(req).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], ERROR_INVALID_PARAMS);
    }

    #[test]
    fn tools_call_validation_rejects_before_execution() {
        // 携带 progressToken 的请求同样先过校验，不产生任何通知
        let server = McpServer::new(progress_registry());
        let mut notifications = Vec::new();
        let req = r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"_meta":{"progressToken":"t"},"name":"nonexistent","arguments":{}}}"#;
        let resp = server
            .handle_line_with_sink(req, &mut |n| notifications.push(n.to_string()))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], ERROR_INVALID_PARAMS);
        assert!(notifications.is_empty());
    }

    #[test]
    fn progress_notification_omits_none_fields() {
        let line = progress_notification(&serde_json::json!("t"), 3.0, None, None);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(v["params"].get("total").is_none());
        assert!(v["params"].get("message").is_none());
        assert_eq!(v["params"]["progress"], 3.0);
    }

    // ─── serve 双向测试基建（共享输出 + 按需输入管道） ──

    use std::sync::Condvar;

    /// 线程共享输出：serve 的 worker 线程与主循环并发写
    #[derive(Default, Clone)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl SharedBuf {
        fn string(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
        /// 轮询直到输出包含 needle（worker 线程异步写，不可即时断言）
        fn wait_for(&self, needle: &str, timeout: Duration) -> String {
            let deadline = std::time::Instant::now() + timeout;
            loop {
                let s = self.string();
                if s.contains(needle) {
                    return s;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "timeout waiting for {needle:?}; got: {s}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// 内存管道：客户端线程按需写入，server 端 Read 阻塞等待。
    /// 用于确定性 e2e —— 客户端必须**看到**服务端的反向请求后才能回包
    /// （预置输入会让响应先于 worker 注册 pending 到达，真实 stdio 无此序）。
    #[derive(Default)]
    struct Pipe(Mutex<Vec<u8>>, Condvar, AtomicBool);

    impl Pipe {
        fn push(&self, line: &str) {
            let mut g = self.0.lock().unwrap();
            g.extend_from_slice(line.as_bytes());
            self.1.notify_all();
        }
        fn close(&self) {
            self.2.store(true, Ordering::Relaxed);
            self.1.notify_all();
        }
    }

    impl Pipe {
        /// 阻塞读取（内部可变性：Arc<Pipe> 共享下也可调用）
        fn read_inner(&self, out: &mut [u8]) -> std::io::Result<usize> {
            let mut g = self.0.lock().unwrap();
            loop {
                if !g.is_empty() {
                    let n = out.len().min(g.len());
                    out[..n].copy_from_slice(&g[..n]);
                    g.drain(..n);
                    return Ok(n);
                }
                if self.2.load(Ordering::Relaxed) {
                    return Ok(0); // EOF
                }
                g = self.1.wait(g).unwrap();
            }
        }
    }

    impl std::io::Read for Pipe {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            self.read_inner(out)
        }
    }

    /// `Arc<Pipe>` 的 Read 句柄（serve 需要 BufRead —— 由外层 BufReader 提供）
    struct PipeHandle(Arc<Pipe>);

    impl std::io::Read for PipeHandle {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            self.0.read_inner(out)
        }
    }

    /// 执行中反向采样 / 取 roots 的测试命令
    fn bridge_registry(sample: bool) -> Registry {
        let mut reg = Registry::new();
        let handler: Handler = Arc::new(move |ctx, _args| {
            let r = if sample {
                let reply = ctx.sample("给我一个字：好", 64)?;
                serde_json::json!({ "sampled": reply })
            } else {
                let roots = ctx.roots()?;
                serde_json::json!({
                    "roots": roots.iter().map(|(uri, name)| serde_json::json!({"uri": uri, "name": name})).collect::<Vec<_>>()
                })
            };
            ctx.done(r.clone(), 0);
            Ok(r)
        });
        let schema = CommandSchema {
            name: "needs-host".into(),
            about: "needs host bridge".into(),
            args: vec![],
            subcommands: vec![],
        };
        reg.register(RegisteredCommand::new("needs-host", schema).with_handler(handler))
            .unwrap();
        reg
    }

    #[test]
    fn sample_without_host_bridge_is_guided_error() {
        // handle_line 路径无宿主桥（CLI/直调场景）→ 错误消息带指引
        let server = McpServer::new(bridge_registry(true));
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"needs-host","arguments":{}}}"#)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], true);
        let msg = v["result"]["content"][0]["text"].as_str().unwrap();
        assert!(msg.contains("不支持 LLM 采样"), "{msg}");
    }

    #[test]
    fn serve_e2e_sampling_roundtrip() {
        let server = Arc::new(McpServer::new(bridge_registry(true)));
        let out = SharedBuf::default();
        let pipe = Arc::new(Pipe::default());
        let (done_tx, done_rx) = mpsc::channel();

        // 服务端线程：EOF（pipe.close）后退出
        {
            let server = Arc::clone(&server);
            let pipe = Arc::clone(&pipe);
            let out = out.clone();
            std::thread::spawn(move || {
                let _ = server.serve(BufReader::new(PipeHandle(Arc::clone(&pipe))), out);
                let _ = done_tx.send(());
            });
        }

        // 客户端线程：initialize（声明 sampling）→ tools/call
        // →（看到服务端 sampling/createMessage 后）回包 → 等结果 → 关闭
        let client = {
            let pipe = Arc::clone(&pipe);
            let out = out.clone();
            std::thread::spawn(move || {
                pipe.push(concat!(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"sampling":{}}}}"#,
                    "\n",
                ));
                pipe.push(concat!(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"needs-host","arguments":{}}}"#,
                    "\n",
                ));
                out.wait_for("sampling/createMessage", Duration::from_secs(5));
                pipe.push(concat!(
                    r#"{"jsonrpc":"2.0","id":"srv-1","result":{"role":"assistant","content":{"type":"text","text":"好"}}}"#,
                    "\n",
                ));
                out.wait_for("sampled", Duration::from_secs(5));
                pipe.close();
            })
        };

        let final_out = out.wait_for("sampled", Duration::from_secs(5));
        assert!(
            final_out.contains("好"),
            "sampled text should round-trip: {final_out}"
        );
        client.join().unwrap();
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("serve exits after EOF");
    }

    #[test]
    fn serve_e2e_sampling_requires_client_capability() {
        let server = Arc::new(McpServer::new(bridge_registry(true)));
        let out = SharedBuf::default();
        let pipe = Arc::new(Pipe::default());

        {
            let server = Arc::clone(&server);
            let pipe = Arc::clone(&pipe);
            let out = out.clone();
            std::thread::spawn(move || {
                let _ = server.serve(BufReader::new(PipeHandle(Arc::clone(&pipe))), out);
            });
        }

        // initialize 不声明 sampling → tools/call 内采样报带指引的错误
        pipe.push(concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#,
            "\n",
        ));
        pipe.push(concat!(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"needs-host","arguments":{}}}"#,
            "\n",
        ));
        let s = out.wait_for("isError", Duration::from_secs(5));
        assert!(
            s.contains("未声明 sampling"),
            "capability gate message: {s}"
        );
        pipe.close();
    }

    #[test]
    fn serve_e2e_roots_roundtrip() {
        let server = Arc::new(McpServer::new(bridge_registry(false)));
        let out = SharedBuf::default();
        let pipe = Arc::new(Pipe::default());

        {
            let server = Arc::clone(&server);
            let pipe = Arc::clone(&pipe);
            let out = out.clone();
            std::thread::spawn(move || {
                let _ = server.serve(BufReader::new(PipeHandle(Arc::clone(&pipe))), out);
            });
        }

        let client = {
            let pipe = Arc::clone(&pipe);
            let out = out.clone();
            std::thread::spawn(move || {
                pipe.push(concat!(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"roots":{}}}}"#,
                    "\n",
                ));
                pipe.push(concat!(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"needs-host","arguments":{}}}"#,
                    "\n",
                ));
                out.wait_for("roots/list", Duration::from_secs(5));
                pipe.push(concat!(
                    r#"{"jsonrpc":"2.0","id":"srv-1","result":{"roots":[{"uri":"file:///workspace","name":"w"}]}}"#,
                    "\n",
                ));
                out.wait_for("file:///workspace", Duration::from_secs(5));
                pipe.close();
            })
        };

        // 等 roots 的回程值（"roots" 会先匹配到服务端自己的 roots/list 请求行，不能用）
        let final_out = out.wait_for("file:///workspace", Duration::from_secs(5));
        assert!(
            final_out.contains("file:///workspace"),
            "roots should round-trip: {final_out}"
        );
        client.join().unwrap();
    }
}
