//! # Lilyco Web GUI — axum 服务器 + 内嵌单页控制台
//!
//! 设计基调（对齐 docs/WEBUI_EVALUATION.md 决策）：
//! - **零外部 UI 依赖**：自研 ~12KB 设计系统 CSS（ChatGPT/Codex 级观感），
//!   Layui（480KB，95% 能力闲置）退役；vanilla JS 架构保留
//! - **文件输入组件**：Path 参数支持拖拽 / 点击选择，浏览器端读文件 →
//!   `POST /upload` 暂存到服务端临时目录 → 回填绝对路径（服务端与浏览器同机）
//! - **安全**：html_escape 全插值点、CSP/nosniff 响应头、动态内容一律 textContent 渲染；
//!   POST /run /upload /cancel 全部要求 token + 回环 Origin
//! - **取消**：`POST /cancel/{sid}` 置取消标志（命令侧经 ctx.is_cancelled() 响应）
//!
//! 端点（与 scripts/acceptance/web_probe.py 验收契约兼容）：
//! - `GET  /`                首页（?cmd= 多命令切换）
//! - `POST /run`             执行 → `{session_id}`
//! - `GET  /progress/{sid}`  SSE 进度流（started/tick/log/telemetry/done/error）
//! - `POST /upload`          文件暂存 → `{path,name,size}`
//! - `POST /cancel/{sid}`    请求取消

use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::{
    extract::{Path, Query, Request as AxumRequest, State},
    http::{header, HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use rand::distributions::Alphanumeric;
use rand::Rng;
use tokio::sync::Mutex;

use lilyco_core::executor;
use lilyco_core::registry::{Handler, RegisteredCommand, Registry};
use lilyco_core::schema::{ArgKind, CommandSchema};
use lilyco_core::{App, AppError, Progress};

pub const TOKEN_HEADER: &str = "X-Lilyco-Token";

/// 上传大小上限（base64 编码前）
const MAX_UPLOAD_BYTES: usize = 200 * 1024 * 1024;
/// 需要令牌 + 回环 Origin 校验的 POST 端点
const PROTECTED_POST: &[&str] = &["/run", "/upload", "/cancel", "/pick"];

fn generate_id() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    let host = if let Some(rest) = host.strip_prefix('[') {
        // IPv6 bracket notation: "[::1]:8080"
        rest.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))?;
    let host = rest.split(['/', ':', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

/// HTML 转义：所有插值进 HTML 模板的动态内容必须先过这里
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 手写 base64 解码（标准字母表，容忍空白与缺省 padding）——零新增依赖
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const INVALID: u8 = 0xFF;
    fn val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => INVALID,
        }
    }
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3 + 3);
    let mut acc: u32 = 0;
    let mut nbits: u32 = 0;
    for &b in &bytes {
        let v = val(b);
        if v == INVALID {
            return None;
        }
        acc = (acc << 6) | v as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((acc >> nbits) as u8);
        }
    }
    // 残余 bits 全 0 才合法（非 0 说明编码被截断/损坏）
    if nbits >= 6 && (acc & ((1 << nbits) - 1)) != 0 {
        return None;
    }
    Some(out)
}

/// 文件名净化：保留字母/数字（含中文）与 `._-`，其余替换为 '_'；拒绝路径穿越
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .take(120)
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.replace("..", "_");
    if cleaned.is_empty() || cleaned == "." {
        String::new()
    } else {
        cleaned
    }
}

// ── GuiRenderer ───────────────────────────────────────────

pub struct GuiRenderer {
    port: u16,
}

/// Runner receives args + a Sender to stream progress events (tick/log/done/error).
/// The sender is connected to the SSE endpoint — messages appear in the browser in real time.
pub type RunnerFn = Arc<
    dyn Fn(
            HashMap<String, serde_json::Value>,
            tokio::sync::mpsc::Sender<serde_json::Value>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

impl GuiRenderer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    pub async fn serve(&self, schema: CommandSchema, runner: RunnerFn) {
        let state = Arc::new(AppState {
            schema: Arc::new(schema),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner,
            token: generate_id(),
        });
        self.serve_state(state).await;
    }

    /// 多命令形态：把整个 `Registry` 暴露为 Web 控制台
    ///
    /// `GET /?cmd=xxx` 按命令渲染表单（页头下拉切换）；隐藏命令不在下拉中，
    /// 与 CLI help / MCP tools/list 语义一致。执行走 Registry 内的 handler。
    pub async fn serve_registry(&self, registry: Registry) {
        let default_schema = registry
            .visible()
            .next()
            .expect("serve_registry: registry has no visible commands")
            .schema
            .clone();
        let state = Arc::new(AppState {
            schema: Arc::new(default_schema),
            registry: Some(Arc::new(registry)),
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            // registry 模式的执行路径在 run_handler 内按 ?cmd 分发，不走这里
            runner: Arc::new(|_, _| Box::pin(async {})),
            token: generate_id(),
        });
        self.serve_state(state).await;
    }

    async fn serve_state(&self, state: Arc<AppState>) {
        let app = Router::new()
            .route("/", get(index))
            .route("/run", post(run_handler))
            .route("/progress/{id}", get(progress_handler))
            .route("/upload", post(upload_handler))
            .route("/cancel/{id}", post(cancel_handler))
            .route("/pick", post(pick_handler))
            .route_layer(middleware::from_fn_with_state(state.clone(), security_mw))
            .with_state(state);

        // 只监听本机回环地址：本地 GUI 工具不需要暴露到局域网
        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", self.port))
            .await
            .unwrap();
        let url = format!("http://localhost:{}", self.port);
        eprintln!("Lilyco GUI ready: {url}");

        // Auto-open browser
        if let Err(e) = webbrowser::open(&url) {
            eprintln!("  (could not open browser: {e})");
        }

        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                tokio::signal::ctrl_c().await.ok();
                eprintln!("\nShutting down...");
            })
            .await
            .unwrap();
    }

    /// Serve with a concrete `App` type. Auto-wires `from_args` + `run`
    /// and streams progress events to the browser via SSE.
    /// Eliminates the need to manually construct a `RunnerFn` closure.
    pub async fn serve_app<A>(&self, schema: CommandSchema)
    where
        A: App + Send + 'static,
    {
        let runner: RunnerFn = Arc::new(move |args, gui_tx| {
            Box::pin(async move {
                // 执行语义交给 core::executor（与 CLI / TUI / MCP 共享同一宿主）
                let args_value = serde_json::to_value(&args).unwrap_or(serde_json::json!({}));
                let handler: Handler = Arc::new(move |ctx, args| {
                    let obj = args
                        .as_object()
                        .ok_or_else(|| AppError::InvalidArg("args must be a JSON object".into()))?;
                    let map: std::collections::HashMap<String, serde_json::Value> =
                        obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    let app = A::from_args(&map)?;
                    app.run(ctx)
                });
                run_progress(handler, args_value, gui_tx).await;
            })
        });
        self.serve(schema, runner).await;
    }
}

/// 执行 handler 并把进度事件流式转发到 SSE 通道（单命令 / 多命令共用）
async fn run_progress(
    handler: Handler,
    args: serde_json::Value,
    gui_tx: tokio::sync::mpsc::Sender<serde_json::Value>,
) {
    let task = executor::spawn(handler, args);
    for event in task.rx {
        let json = serde_json::to_value(&event).unwrap();
        if gui_tx.send(json).await.is_err() {
            break;
        }
        if matches!(event, Progress::Done { .. } | Progress::Error { .. }) {
            break;
        }
    }
    if let Ok(Err(e)) = task.handle.join() {
        let _ = gui_tx
            .send(serde_json::json!({
                "type": "error", "code": 1,
                "message": e.to_string(), "kind": null
            }))
            .await;
    }
}

/// 注册表模式的执行循环：额外把取消句柄登记到 `cancels`（/cancel/{sid} 用），
/// 终态后清理。
async fn run_progress_registry(
    state: Arc<AppState>,
    sid: String,
    handler: Handler,
    args: serde_json::Value,
    gui_tx: tokio::sync::mpsc::Sender<serde_json::Value>,
) {
    let task = executor::spawn(handler, args);
    state
        .cancels
        .lock()
        .await
        .insert(sid.clone(), Arc::clone(&task.cancel));
    for event in task.rx {
        let json = serde_json::to_value(&event).unwrap();
        if gui_tx.send(json).await.is_err() {
            break;
        }
        if matches!(event, Progress::Done { .. } | Progress::Error { .. }) {
            break;
        }
    }
    state.cancels.lock().await.remove(&sid);
    if let Ok(Err(e)) = task.handle.join() {
        let _ = gui_tx
            .send(serde_json::json!({
                "type": "error", "code": 1,
                "message": e.to_string(), "kind": null
            }))
            .await;
    }
}

// ── State ──────────────────────────────────────────────────

struct AppState {
    schema: Arc<CommandSchema>,
    /// 多命令模式（`serve_registry`）：整张注册表
    registry: Option<Arc<Registry>>,
    sessions: Mutex<HashMap<String, tokio::sync::mpsc::Receiver<serde_json::Value>>>,
    /// sid → 取消标志（registry 模式 /cancel 端点用；终态后移除）
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    runner: RunnerFn,
    token: String,
}

// ── Security middleware ───────────────────────────────────
//
// 防御 DNS rebinding / CSRF：
// 1. 所有请求的 Host 必须是回环地址（rebinding 时 Host 仍是攻击者域名，会被拒绝）
// 2. 受保护 POST（/run /upload /cancel）的 Origin 若非回环地址则拒绝
// 3. 受保护 POST 必须携带本次启动随机生成的 X-Lilyco-Token

async fn security_mw(
    State(state): State<Arc<AppState>>,
    request: AxumRequest,
    next: Next,
) -> Response {
    let headers = request.headers();

    let host = header_str(headers, "host").unwrap_or_default();
    if !is_loopback_host(host) {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    let path = request.uri().path();
    if request.method() == Method::POST && PROTECTED_POST.contains(&path) {
        if let Some(origin) = header_str(headers, "origin") {
            let ok = origin_host(origin).map(is_loopback_host).unwrap_or(false);
            if !ok {
                return (StatusCode::FORBIDDEN, "Forbidden").into_response();
            }
        }
        let token = header_str(headers, TOKEN_HEADER).unwrap_or_default();
        if !constant_time_eq(token, &state.token) {
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    }

    next.run(request).await
}

// ── HTML 渲染 ──────────────────────────────────────────────

/// 按查询参数挑出要渲染的命令 schema（多命令模式）
///
/// `want` 必须命中可见命令（`registry.get` 含别名解析；隐藏命令不可导航），
/// 否则回退第一个可见命令。
fn pick_command<'r>(registry: &'r Registry, want: Option<&str>) -> &'r RegisteredCommand {
    want.and_then(|n| registry.get(n))
        .filter(|c| !c.hidden)
        .or_else(|| registry.visible().next())
        .expect("pick_command: registry has no visible commands")
}

async fn index(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    // 多命令模式：?cmd= 决定渲染哪个命令的表单
    let schema = match &state.registry {
        Some(reg) => pick_command(reg, params.get("cmd").map(|x| x.as_str()))
            .schema
            .clone(),
        None => state.schema.as_ref().clone(),
    };

    // 命令切换下拉（可见命令 > 1 时出现）
    let mut cmd_nav = String::new();
    if let Some(reg) = &state.registry {
        let visible: Vec<&RegisteredCommand> = reg.visible().collect();
        if visible.len() > 1 {
            let mut opts = String::new();
            for c in &visible {
                let name = html_escape(&c.schema.name);
                let sel = if c.schema.name == schema.name {
                    " selected"
                } else {
                    ""
                };
                opts.push_str(&format!("<option value=\"{name}\"{sel}>{name}</option>"));
            }
            cmd_nav = format!(
                "<select id=\"cmd-nav\" aria-label=\"切换命令\" onchange=\"if(this.value)location='/?cmd='+encodeURIComponent(this.value)\">{opts}</select>"
            );
        }
    }

    let mut fields_html = String::new();
    let mut field_meta: Vec<serde_json::Value> = Vec::new();

    for arg in &schema.args {
        field_meta.push(serde_json::json!({
            "name": arg.name,
            "kind": kind_name(&arg.kind),
            "required": arg.required,
        }));

        let esc_name = html_escape(&arg.name);
        let req_mark = if arg.required {
            "<span class=\"req-mark\" aria-hidden=\"true\">*</span>"
        } else {
            ""
        };
        let label = format!("{}{}", html_escape(&arg.about), req_mark);
        let req_a = if arg.required { " required" } else { "" };

        let widget = match &arg.kind {
            ArgKind::Flag => {
                let ck = matches!(&arg.default, Some(serde_json::Value::Bool(true)))
                    .then_some(" checked")
                    .unwrap_or("");
                format!(
                    "<label class=\"flag-row\"><input type=\"checkbox\" id=\"field-{esc_name}\"{ck}> \
                     <span>{label}</span></label>"
                )
            }
            ArgKind::Text => {
                let dv = arg.default.as_ref().and_then(|d| d.as_str()).unwrap_or("");
                format!(
                    "<input type=\"text\" id=\"field-{esc_name}\" placeholder=\"{}\"{req_a} value=\"{}\">",
                    html_escape(&arg.about),
                    html_escape(dv),
                )
            }
            ArgKind::Path { must_exist } => {
                // 文件输入组件：手动路径 + 拖拽/点击上传（上传后回填服务端暂存绝对路径）
                let dv = arg.default.as_ref().and_then(|d| d.as_str()).unwrap_or("");
                let must_attr = if *must_exist { "1" } else { "0" };
                let hint = if *must_exist {
                    "拖拽文件到此处，或点击选择 —— 上传后自动回填服务端路径"
                } else {
                    "可选：拖拽文件上传并回填路径，或直接手填"
                };
                // 「本机」按钮：让服务端弹系统选择器，回填磁盘上的原始路径（不上传副本）。
                // 没开 pick 特性的构建不画它，免得点了只得到一句报错。
                let pick_btn = if cfg!(feature = "pick") {
                    format!(
                        "<button type=\"button\" class=\"btn-icon dz-pick\" data-pick=\"{esc_name}\" \
                         aria-label=\"用系统选择器挑本机文件\" title=\"用系统选择器挑本机文件（只回填真实路径，不上传副本）\">本机</button>"
                    )
                } else {
                    String::new()
                };
                format!(
                    "<input type=\"text\" id=\"field-{esc_name}\" class=\"mono\" placeholder=\"{}\"{req_a} value=\"{}\" spellcheck=\"false\">\
                     <div class=\"dropzone\" data-target=\"field-{esc_name}\" data-must-exist=\"{must_attr}\" tabindex=\"0\" role=\"button\" aria-label=\"上传文件\">\
                     <input type=\"file\" class=\"visually-hidden\" data-file-for=\"field-{esc_name}\" tabindex=\"-1\">\
                     <span class=\"dz-icon\">⇪</span><span class=\"dz-hint\">{hint}</span>\
                     <span class=\"dz-status\" id=\"up-{esc_name}\" aria-live=\"polite\"></span>\
                     <span class=\"file-chip\" id=\"chip-{esc_name}\" hidden></span>{pick_btn}</div>",
                    html_escape(&arg.about),
                    html_escape(dv),
                )
            }
            ArgKind::Number { min, max } => {
                let dv = arg
                    .default
                    .as_ref()
                    .and_then(|d| d.as_f64())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| min.map(|m| m.to_string()).unwrap_or_default());
                let min_a = min.map(|m| format!(" min=\"{m}\"")).unwrap_or_default();
                let max_a = max.map(|m| format!(" max=\"{m}\"")).unwrap_or_default();
                format!(
                    "<input type=\"number\" id=\"field-{esc_name}\" value=\"{}\" step=\"any\"{min_a}{max_a}{req_a}>",
                    html_escape(&dv),
                )
            }
            ArgKind::Enum { values } => {
                let mut opts = String::new();
                for v in values {
                    let ev = html_escape(v);
                    let sel = if arg.default.as_ref().and_then(|d| d.as_str()) == Some(v.as_str()) {
                        " selected"
                    } else {
                        ""
                    };
                    opts.push_str(&format!("<option value=\"{ev}\"{sel}>{ev}</option>"));
                }
                format!("<select id=\"field-{esc_name}\"{req_a}>{opts}</select>")
            }
            ArgKind::List { .. } => {
                // 动态行：默认 2 行 + 增删按钮（收集时按 data-list 前缀聚合）
                format!(
                    "<div class=\"list-rows\" id=\"list-{esc_name}\" data-list=\"{esc_name}\">\
                     <div class=\"list-row\"><input type=\"text\" class=\"mono\" data-list-item=\"{esc_name}\" placeholder=\"{}\"><button type=\"button\" class=\"btn-icon row-del\" aria-label=\"删除该行\" onclick=\"this.closest('.list-row').remove()\">✕</button></div>\
                     <div class=\"list-row\"><input type=\"text\" class=\"mono\" data-list-item=\"{esc_name}\" placeholder=\"{}\"><button type=\"button\" class=\"btn-icon row-del\" aria-label=\"删除该行\" onclick=\"this.closest('.list-row').remove()\">✕</button></div>\
                     </div>\
                     <button type=\"button\" class=\"btn-icon list-add\" data-list-add=\"{esc_name}\">＋ 添加一项</button>",
                    html_escape(&arg.about),
                    html_escape(&arg.about),
                )
            }
        };

        let flag_layout = matches!(&arg.kind, ArgKind::Flag);
        if flag_layout {
            fields_html.push_str(&format!("<div class=\"field field-flag\">{widget}</div>\n"));
        } else if matches!(&arg.kind, ArgKind::Path { .. }) {
            fields_html.push_str(&format!(
                "<div class=\"field\"><label for=\"field-{esc_name}\">{label}</label>{widget}</div>\n"
            ));
        } else {
            fields_html.push_str(&format!(
                "<div class=\"field\"><label for=\"field-{esc_name}\">{label}</label>{widget}</div>\n"
            ));
        }
    }

    // JSON 元数据（含 </script> 防 breakout 转义）
    let meta_json = serde_json::to_string(&field_meta)
        .unwrap_or_else(|_| "[]".into())
        .replace("</", "<\\/");
    let cmd_json = serde_json::json!(schema.name).to_string();
    let about_html = html_escape(&schema.about);
    let cmd_html = html_escape(&schema.name);

    let html = HTML_TEMPLATE
        .replace("__CSS__", include_str!("../assets/app.css"))
        .replace("__CMD_NAV__", &cmd_nav)
        .replace("__FIELDS__", &fields_html)
        .replace("__ABOUT__", &about_html)
        .replace("__CMD_NAME__", &cmd_html)
        .replace("__CMD_JS__", &cmd_json)
        .replace("__META__", &meta_json)
        // token 最后注入且 token 为随机字母数字，无碰撞风险
        .replace("__TOKEN__", &state.token);

    let mut resp = Html(html).into_response();
    let headers = resp.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
         connect-src 'self'; img-src data:; base-uri 'none'"
            .parse()
            .unwrap(),
    );
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    resp
}

fn kind_name(kind: &ArgKind) -> &'static str {
    match kind {
        ArgKind::Flag => "Flag",
        ArgKind::Text => "Text",
        ArgKind::Number { .. } => "Number",
        ArgKind::Enum { .. } => "Enum",
        ArgKind::Path { .. } => "Path",
        ArgKind::List { .. } => "List",
    }
}

/// 页面模板。占位符用 `__X__` 哨兵（避免与动态内容中的 `{...}` 冲突）；
/// 动态内容在服务端全部 html_escape / JSON 编码后才插入。
const HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1.0">
<meta name="lilyco-token" content="__TOKEN__">
<title>__CMD_NAME__ · lilyco 控制台</title>
<style>__CSS__</style>
</head>
<body>
<header class="topbar">
  <div class="brand"><span class="brand-mark">◆</span> lilyco <span class="brand-sub">Web 控制台</span></div>
  __CMD_NAV__
  <button type="button" id="theme-toggle" class="icon-btn" aria-label="切换深浅主题">◐</button>
</header>

<main class="page">
<section class="card">
  <h1 class="card-title">__CMD_NAME__</h1>
  <p class="card-about">__ABOUT__</p>

  <form id="form" novalidate>
__FIELDS__
    <div class="actions">
      <button type="submit" id="run-btn" class="btn primary"><span class="btn-label">▶ 运行</span></button>
      <button type="button" id="cancel-btn" class="btn danger" hidden>■ 取消</button>
      <button type="button" id="copy-cli" class="btn ghost">复制 CLI</button>
    </div>
    <div class="cli-preview mono" id="preview"></div>
  </form>

  <section id="out" hidden aria-live="polite">
    <div class="out-head">
      <h2>输出</h2>
      <div class="progress" id="progress" data-indet="1"><div class="progress-bar" id="progress-bar"></div></div>
    </div>
    <div id="log" class="terminal" aria-live="polite"></div>
    <div class="result-wrap" id="result-wrap" hidden>
      <div class="result-head"><span>结果</span><button type="button" id="copy-result" class="btn-icon">复制</button></div>
      <pre id="result" class="mono"></pre>
    </div>
  </section>
</section>
<footer class="foot">lilyco · 本地回环会话 · 令牌随进程轮换</footer>
</main>

<script>
const TOKEN=document.querySelector('meta[name="lilyco-token"]').content;
const CMD=__CMD_JS__;
const SCHEMA_ARGS=__META__;
"use strict";
const $=id=>document.getElementById(id);
const esc=s=>{const d=document.createElement("div");d.textContent=String(s);return d.innerHTML};

// ── 主题（系统偏好 + 手动切换持久化）──
const root=document.documentElement;
const savedTheme=localStorage.getItem("lilyco-theme");
if(savedTheme)root.dataset.theme=savedTheme;
$("theme-toggle").addEventListener("click",()=>{
  const cur=root.dataset.theme||(matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light");
  const next=cur==="dark"?"light":"dark";
  root.dataset.theme=next;localStorage.setItem("lilyco-theme",next);
});

// ── CLI 预览 ──
function fieldVal(a){
  if(a.kind==="Flag")return $( "field-"+a.name).checked?"__flag__":null;
  if(a.kind==="List"){return Array.from(document.querySelectorAll('[data-list-item="'+a.name+'"]')).map(i=>i.value).filter(v=>v!=="")}
  const el=$("field-"+a.name);
  if(!el||el.value==="")return null;
  if(a.kind==="Number"){const n=Number(el.value);return isNaN(n)?el.value:n}
  return el.value;
}
function updatePreview(){
  const parts=[CMD];
  for(const a of SCHEMA_ARGS){
    const v=fieldVal(a);
    if(v===null)continue;
    if(v==="__flag__"){parts.push("--"+a.name);continue}
    if(Array.isArray(v)){v.forEach(x=>parts.push("--"+a.name+" "+(String(x).includes(" ")?'"'+x+'"':x)));continue}
    parts.push("--"+a.name+" "+(String(v).includes(" ")||a.kind==="Path"?'"'+v+'"':v));
  }
  $("preview").textContent=parts.join(" ");
}
document.getElementById("form").addEventListener("input",updatePreview);
updatePreview();

// ── 原生选择器：让服务端弹系统对话框，回填磁盘上的原始路径（不上传副本） ──
// 与上面的拖拽区是两条路：上传拿到的是服务端暂存副本，这里拿到文件本来的路径。
// 对话框可能被压在浏览器后面（Windows 不让后台进程抢前台），所以状态里写明「看任务栏」。
document.querySelectorAll("[data-pick]").forEach(btn=>btn.addEventListener("click",async()=>{
  const name=btn.getAttribute("data-pick"),field=$("field-"+name),status=$("up-"+name);
  if(!field)return;
  btn.disabled=true;
  if(status)status.textContent="等待系统对话框…（可能被压在后面，看一下任务栏）";
  try{
    const resp=await fetch("/pick",{method:"POST",headers:{"Content-Type":"application/json","X-Lilyco-Token":TOKEN},body:"{}"});
    const j=await resp.json();
    if(j.path){field.value=j.path;updatePreview();if(status)status.textContent="已选择："+j.path}
    else if(status)status.textContent=j.error||"已取消";
  }catch(err){if(status)status.textContent="选择框失败："+err}
  finally{btn.disabled=false}
}));

// ── 文件输入组件：拖拽 / 点击选择 → /upload 暂存 → 回填绝对路径 ──
function fmtSize(n){return n>1048576?(n/1048576).toFixed(1)+" MB":n>1024?(n/1024).toFixed(1)+" KB":n+" B"}
async function uploadFile(file,dz){
  const name=dz.dataset.target,status=dz.querySelector(".dz-status"),chip=dz.querySelector(".file-chip"),input=$("field-"+name);
  status.textContent="上传中 "+file.name+"（"+fmtSize(file.size)+"）…";status.className="dz-status busy";
  try{
    const dataUrl=await new Promise((res,rej)=>{const r=new FileReader();r.onload=()=>res(r.result);r.onerror=()=>rej(r.error||new Error("读取失败"));r.readAsDataURL(file)});
    const b64=dataUrl.split(",",2)[1]||"";
    const resp=await fetch("/upload",{method:"POST",headers:{"Content-Type":"application/json","X-Lilyco-Token":TOKEN},body:JSON.stringify({name:file.name,dataB64:b64})});
    if(!resp.ok){throw new Error((await resp.text())||("HTTP "+resp.status))}
    const j=await resp.json();
    input.value=j.path;
    chip.textContent=file.name+" · "+fmtSize(j.size)+"  ✕";
    chip.hidden=false;status.textContent="已就绪：服务端路径已回填";status.className="dz-status ok";
    updatePreview();
    chip.onclick=()=>{input.value="";chip.hidden=true;status.textContent="";updatePreview()};
  }catch(err){status.textContent="上传失败："+err.message;status.className="dz-status err"}
}
document.querySelectorAll(".dropzone").forEach(dz=>{
  const fi=dz.querySelector("input[type=file]");
  dz.addEventListener("click",()=>fi.click());
  dz.addEventListener("keydown",e=>{if(e.key==="Enter"||e.key===" "){e.preventDefault();fi.click()}});
  fi.addEventListener("change",()=>{if(fi.files[0])uploadFile(fi.files[0],dz)});
  dz.addEventListener("dragover",e=>{e.preventDefault();dz.classList.add("drag-over")});
  dz.addEventListener("dragleave",()=>dz.classList.remove("drag-over"));
  dz.addEventListener("drop",e=>{e.preventDefault();dz.classList.remove("drag-over");
    const f=e.dataTransfer.files&&e.dataTransfer.files[0];if(f)uploadFile(f,dz)});
});

// ── List 增删行 ──
document.querySelectorAll("[data-list-add]").forEach(btn=>btn.addEventListener("click",()=>{
  const name=btn.dataset.listAdd,rows=$("list-"+name);
  const row=document.createElement("div");row.className="list-row";
  const inp=document.createElement("input");inp.type="text";inp.className="mono";inp.dataset.listItem=name;
  const del=document.createElement("button");del.type="button";del.className="btn-icon row-del";del.textContent="✕";del.setAttribute("aria-label","删除该行");
  del.addEventListener("click",()=>row.remove());
  inp.addEventListener("input",updatePreview);
  row.append(inp,del);rows.append(row);inp.focus();
}));

// ── 运行 / SSE / 取消 ──
const out=$("out"),logEl=$("log"),bar=$("progress"),barEl=$("progress-bar"),resultWrap=$("result-wrap"),resultEl=$("result");
let es=null,activeSid=null;
function setProgress(p){ // p: null=不确定, 0..1
  if(p===null){bar.dataset.indet="1";barEl.style.width="40%"}
  else{bar.dataset.indet="0";barEl.style.width=Math.max(0,Math.min(100,p*100)).toFixed(1)+"%"}
}
function logLine(cls,text){
  const d=document.createElement("div");d.className="line"+(cls?" "+cls:"");d.textContent=text;
  logEl.appendChild(d);logEl.scrollTop=logEl.scrollHeight;
}
function setBusy(b){
  $("run-btn").disabled=b;$("run-btn").classList.toggle("loading",b);
  $("cancel-btn").hidden=!b;
}
document.getElementById("form").addEventListener("submit",async e=>{
  e.preventDefault();
  const data={};
  for(const a of SCHEMA_ARGS){
    if(a.kind==="Flag"){data[a.name]=$("field-"+a.name).checked;continue}
    if(a.kind==="List"){data[a.name]=Array.from(document.querySelectorAll('[data-list-item="'+a.name+'"]')).map(i=>i.value).filter(v=>v!=="");continue}
    const el=$("field-"+a.name),v=el?el.value:"";
    data[a.name]=v===""?null:(a.kind==="Number"&&(n=>!isNaN(n)&&v!=="")(Number(v))?Number(v):v);
  }
  out.hidden=false;logEl.textContent="";resultWrap.hidden=true;setProgress(null);setBusy(true);
  let sid;
  try{
    const resp=await fetch("/run",{method:"POST",headers:{"Content-Type":"application/json","X-Lilyco-Token":TOKEN},body:JSON.stringify({args:data,cmd:CMD})});
    if(!resp.ok){throw new Error((await resp.text())||("HTTP "+resp.status))}
    sid=(await resp.json()).session_id;activeSid=sid;
  }catch(err){logLine("err","请求失败："+err.message);setBusy(false);return}
  es=new EventSource("/progress/"+sid);
  es.onmessage=ev=>{
    let p;try{p=JSON.parse(ev.data)}catch{return}
    if(p.type==="started"){logLine(null,p.message||"运行中…")}
    else if(p.type==="tick"){setProgress(p.percent);if(p.message)logLine(null,p.message)}
    else if(p.type==="log"){logLine(p.level==="error"?"err":p.level==="warn"?"warn":null,"["+(p.level||"info")+"] "+p.message)}
    else if(p.type==="telemetry"){logLine("tele",p.key+" = "+JSON.stringify(p.value))}
    else if(p.type==="done"){setProgress(1);resultEl.textContent=JSON.stringify(p.result,null,2);resultWrap.hidden=false;finish()}
    else if(p.type==="error"){setProgress(0);logLine("err","ERROR: "+p.message);finish()}
  };
  es.onerror=()=>{finish()};
  function finish(){if(es){es.close();es=null}activeSid=null;setBusy(false)}
});
$("cancel-btn").addEventListener("click",async()=>{
  if(!activeSid)return;
  try{await fetch("/cancel/"+activeSid,{method:"POST",headers:{"X-Lilyco-Token":TOKEN}});logLine("warn","已发送取消请求（命令需响应 ctx.is_cancelled）")}catch(e){logLine("err","取消失败："+e.message)}
});
$("copy-result").addEventListener("click",()=>navigator.clipboard.writeText(resultEl.textContent));
$("copy-cli").addEventListener("click",()=>navigator.clipboard.writeText($("preview").textContent));
</script>
</body></html>"#;

// ── Handlers ───────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct RunRequest {
    args: HashMap<String, serde_json::Value>,
    /// 多命令模式：要执行的命令名（单命令模式忽略）
    #[serde(default)]
    cmd: Option<String>,
}

#[derive(serde::Deserialize)]
struct UploadRequest {
    /// 原始文件名（仅用于生成暂存文件名，会被净化）
    name: String,
    /// 文件内容（dataURL 去前缀后的 base64）。
    /// 页面发的是 camelCase `dataB64`，两种写法都得收：只认一种时另一种会整条请求 400，
    /// 而报出来的错是「missing field data_b64」，看的人根本想不到是键名对不上。
    #[serde(alias = "dataB64")]
    data_b64: String,
}

async fn run_handler(State(state): State<Arc<AppState>>, Json(req): Json<RunRequest>) -> Response {
    // 多命令模式：按 req.cmd 从 Registry 取命令执行
    if let Some(reg) = &state.registry {
        // /run 是显式执行：未指定命令 → 默认第一个可见；指定但未知/隐藏 → 400
        // （与 index 的"回退到第一个可见"导航语义不同，执行绝不静默换命令）
        let resolved = match req.cmd.as_deref() {
            None | Some("") => reg.visible().next(),
            Some(name) => reg.get(name).filter(|c| !c.hidden),
        };
        let Some(cmd) = resolved else {
            return (
                StatusCode::BAD_REQUEST,
                format!("unknown command: {}", req.cmd.as_deref().unwrap_or("")),
            )
                .into_response();
        };
        let args_value = serde_json::json!(req.args);
        // 服务端 schema 校验（与单命令模式同一套 validate_args）
        if let Err(e) = cmd.schema.validate_args(&args_value) {
            return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
        }
        let Some(handler) = cmd.handler.clone() else {
            return (
                StatusCode::BAD_REQUEST,
                format!("command `{}` has no handler", cmd.name),
            )
                .into_response();
        };

        let sid = generate_id();
        let (tx, rx) = tokio::sync::mpsc::channel::<serde_json::Value>(128);
        state.sessions.lock().await.insert(sid.clone(), rx);
        let state2 = Arc::clone(&state);
        let sid2 = sid.clone();
        tokio::spawn(async move {
            run_progress_registry(state2, sid2, handler, args_value, tx).await;
        });
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "session_id": sid })),
        )
            .into_response();
    }

    // 单命令模式：服务端 schema 校验（浏览器端的 required/min/max 可被绕过）：
    // CommandSchema::validate_args，三端唯一校验实现
    let args_value = serde_json::json!(req.args);
    if let Err(e) = state.schema.validate_args(&args_value) {
        return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
    }

    let sid = generate_id();
    let (tx, rx) = tokio::sync::mpsc::channel::<serde_json::Value>(128);
    {
        let mut sessions = state.sessions.lock().await;
        sessions.insert(sid.clone(), rx);
    }

    let runner = state.runner.clone();

    tokio::spawn(async move {
        runner(req.args, tx).await;
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({ "session_id": sid })),
    )
        .into_response()
}

/// 对话框标题：既是给用户看的，也是本进程找回那个窗口的查找键
const PICKER_TITLE: &str = "选择文件";

/// 提醒线程的句柄；非 Windows 版本给 None，调用点两边写法一致
struct Raiser(Option<std::thread::JoinHandle<()>>);

impl Raiser {
    fn join(self) {
        if let Some(handle) = self.0 {
            let _ = handle.join();
        }
    }
}

/// Windows 的前台锁定不让后台进程抢焦点，所以不硬抢：按标题轮询到那个顶层窗口后
/// `SetForegroundWindow` 试一次 + `FlashWindowEx` 让任务栏闪。页面提示是主力，闪烁是加成。
#[cfg(all(windows, feature = "pick"))]
fn flash_picker_when_ready(title: &'static str) -> Raiser {
    Raiser(Some(std::thread::spawn(move || {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, FlashWindowEx, SetForegroundWindow, FLASHWINFO, FLASHW_ALL,
            FLASHW_TIMERNOFG,
        };
        let needle: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        // 窗口是另一个线程慢慢建出来的，所以要等一会；建好后闪一次就收工
        for _ in 0..50 {
            // windows-sys 里 PCWSTR 就是裸的 *const u16，结尾 0 由 needle 自己带上
            let hwnd = unsafe { FindWindowW(std::ptr::null(), needle.as_ptr()) };
            if !hwnd.is_null() {
                unsafe {
                    SetForegroundWindow(hwnd);
                    let info = FLASHWINFO {
                        cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                        hwnd,
                        dwFlags: FLASHW_ALL | FLASHW_TIMERNOFG,
                        uCount: 5,
                        dwTimeout: 0,
                    };
                    FlashWindowEx(&info);
                }
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
    })))
}

/// 非 Windows：没有可闪的任务栏格子，页面那条提示仍然管用
#[cfg(not(all(windows, feature = "pick")))]
fn flash_picker_when_ready(_title: &'static str) -> Raiser {
    Raiser(None)
}

/// 让本机进程弹一次系统文件选择框，把**磁盘上的原始路径**回填到页面输入框。
///
/// 与 `/upload` 的分工：拖拽上传走浏览器（拿到的是服务端暂存副本的路径，受 200 MB 上限），
/// 这里走本机对话框（拿到文件本来的路径，不复制、不设体积上限）。浏览器出于安全永远不给
/// 真实路径，所以这个框只能由服务端弹；而四端共用的 handler 收的正是 `path`。
async fn pick_handler() -> Response {
    #[cfg(feature = "pick")]
    {
        // 对话框是阻塞调用：放 blocking 线程里等，弹着的时候页面其他请求照常跑
        let picked = tokio::task::spawn_blocking(|| {
            let raiser = flash_picker_when_ready(PICKER_TITLE);
            let chosen = rfd::FileDialog::new().set_title(PICKER_TITLE).pick_file();
            let _ = raiser.join();
            chosen
        })
        .await;
        match picked {
            // 取消不是错误：path 给 null，页面留着原值不动
            Ok(Some(path)) => Json(serde_json::json!({ "path": path.display().to_string() })),
            Ok(None) => Json(serde_json::json!({ "path": serde_json::Value::Null })),
            Err(error) => Json(serde_json::json!({
                "path": serde_json::Value::Null,
                "error": error.to_string(),
            })),
        }
        .into_response()
    }
    #[cfg(not(feature = "pick"))]
    {
        (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "path": serde_json::Value::Null,
                "error": "这个构建没开 pick 特性：请手填路径，或把文件拖进虚线框上传",
            })),
        )
            .into_response()
    }
}

/// 文件暂存：浏览器端读文件 → base64 → 写入服务端临时目录 → 回填绝对路径。
/// 服务端与浏览器同机（回环限定），命令读到的即用户选的文件。
async fn upload_handler(Json(req): Json<UploadRequest>) -> Response {
    let name = sanitize_filename(&req.name);
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "invalid file name").into_response();
    }
    // base64 膨胀系数 4/3：编码长度粗筛即可挡住超大 body
    if req.data_b64.len() > MAX_UPLOAD_BYTES / 3 * 4 + 1024 {
        return (StatusCode::PAYLOAD_TOO_LARGE, "file too large (max 200MB)").into_response();
    }
    let Some(bytes) = base64_decode(&req.data_b64) else {
        return (StatusCode::BAD_REQUEST, "invalid base64").into_response();
    };
    if bytes.len() > MAX_UPLOAD_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "file too large (max 200MB)").into_response();
    }

    let dir = std::env::temp_dir().join("lilyco-uploads");
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("create upload dir: {e}"),
        )
            .into_response();
    }
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let fname = format!("{}_{}.{}", ms, &generate_id()[..8], name);
    let path = dir.join(&fname);
    if let Err(e) = tokio::fs::write(&path, &bytes).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("write upload: {e}"),
        )
            .into_response();
    }

    Json(serde_json::json!({
        "path": path.to_string_lossy(),
        "name": req.name,
        "size": bytes.len(),
    }))
    .into_response()
}

/// 请求取消：置取消标志；命令侧通过 `ctx.is_cancelled()` 响应
async fn cancel_handler(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let flag = state.cancels.lock().await.get(&id).cloned();
    match flag {
        Some(f) => {
            f.store(true, Ordering::Relaxed);
            Json(serde_json::json!({ "ok": true })).into_response()
        }
        None => (StatusCode::NOT_FOUND, "no such running session").into_response(),
    }
}

async fn progress_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let maybe_rx = { state.sessions.lock().await.remove(&id) };
    let found = maybe_rx.is_some();

    let stream = async_stream::stream! {
        if !found {
            yield Result::<Event, Infallible>::Ok(Event::default().data(
                serde_json::json!({"type":"error","message":"session not found"}).to_string()
            ));
        } else {
            let mut rx = maybe_rx.unwrap();
            yield Ok(Event::default().data(
                serde_json::json!({"type":"started","message":"Running..."}).to_string()
            ));
            while let Some(msg) = rx.recv().await {
                yield Ok(Event::default().data(msg.to_string()));
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco_core::schema::{ArgKind, ArgSchema};

    fn schema_with_required_number() -> CommandSchema {
        CommandSchema {
            name: "demo".into(),
            about: "demo".into(),
            args: vec![
                ArgSchema {
                    name: "quality".into(),
                    about: "质量".into(),
                    kind: ArgKind::Number {
                        min: Some(0.0),
                        max: Some(51.0),
                    },
                    required: false,
                    default: Some(serde_json::json!(23)),
                },
                ArgSchema {
                    name: "input".into(),
                    about: "输入".into(),
                    kind: ArgKind::Text,
                    required: true,
                    default: None,
                },
            ],
            subcommands: vec![],
            safety: lilyco_core::safety::SafetyTier::ReadOnly,
        }
    }

    async fn body_of(resp: Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn test_state() -> Arc<AppState> {
        let runner: RunnerFn = Arc::new(|_args, _tx| Box::pin(async {}));
        Arc::new(AppState {
            schema: Arc::new(schema_with_required_number()),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner,
            token: "test-token".into(),
        })
    }

    // ── 多命令模式 ──

    fn two_command_registry() -> Registry {
        let mut reg = Registry::new();
        let schema_of = |name: &str, about: &str| CommandSchema {
            name: name.into(),
            about: about.into(),
            args: vec![],
            subcommands: vec![],
            safety: lilyco_core::safety::SafetyTier::ReadOnly,
        };
        let ping_handler: Handler = Arc::new(|_ctx, _args| Ok(serde_json::json!({"ok": true})));
        reg.register(
            RegisteredCommand::new("ping", schema_of("ping", "问好")).with_handler(ping_handler),
        )
        .unwrap();
        // 隐藏命令：get 可命中但不可导航（pick_command 会回退）
        reg.register(RegisteredCommand::new("secret", schema_of("secret", "隐藏")).hidden(true))
            .unwrap();
        reg
    }

    fn registry_state() -> Arc<AppState> {
        Arc::new(AppState {
            schema: Arc::new(CommandSchema {
                name: "ping".into(),
                about: "问好".into(),
                args: vec![],
                subcommands: vec![],
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
            }),
            registry: Some(Arc::new(two_command_registry())),
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner: Arc::new(|_, _| Box::pin(async {})),
            token: "test-token".into(),
        })
    }

    #[test]
    fn pick_command_defaults_to_first_visible() {
        let reg = two_command_registry();
        assert_eq!(pick_command(&reg, None).schema.name, "ping");
        assert_eq!(pick_command(&reg, Some("nope")).schema.name, "ping");
    }

    #[test]
    fn pick_command_hidden_falls_back() {
        let reg = two_command_registry();
        assert_eq!(pick_command(&reg, Some("secret")).schema.name, "ping");
    }

    #[tokio::test]
    async fn index_renders_selected_command_and_nav() {
        let state = registry_state();
        let mut params = HashMap::new();
        params.insert("cmd".to_string(), "secret".to_string());
        let resp = index(State(state.clone()), Query(params)).await;
        // hidden 不可导航 → 回退第一个可见命令
        let body = body_of(resp).await;
        assert!(body.contains("ping"), "fallback to first visible");
        assert!(body.contains("select"), "nav dropdown expected");
    }

    /// `/pick`（原生文件选择器）必须和 /run /upload /cancel 同一道闸：
    /// 它会开一个窗口，不能被别的页面远程刷
    #[test]
    fn pick_endpoint_is_token_gated() {
        assert!(
            PROTECTED_POST.contains(&"/pick"),
            "/pick 漏在闸外：{PROTECTED_POST:?}"
        );
    }

    /// Path 字段除了拖拽区还要带「本机」按钮（拿原始路径，不走上传副本）；数字字段不带
    #[tokio::test]
    async fn path_fields_get_the_native_pick_button() {
        let state = Arc::new(AppState {
            schema: Arc::new(CommandSchema {
                name: "pick".into(),
                about: "pick".into(),
                args: vec![
                    ArgSchema {
                        name: "file".into(),
                        about: "文件".into(),
                        kind: ArgKind::Path { must_exist: false },
                        required: true,
                        default: None,
                    },
                    ArgSchema {
                        name: "quality".into(),
                        about: "质量".into(),
                        kind: ArgKind::Number {
                            min: Some(0.0),
                            max: Some(51.0),
                        },
                        required: false,
                        default: Some(serde_json::json!(23)),
                    },
                ],
                subcommands: vec![],
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
            }),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner: Arc::new(|_, _| Box::pin(async {})),
            token: "t".into(),
        });
        let body = body_of(index(State(state), Query(HashMap::new())).await).await;
        assert!(body.contains("dropzone"), "Path 参数要有拖拽区");
        if cfg!(feature = "pick") {
            assert!(
                body.contains("data-pick=\"file\"") && body.contains("fetch(\"/pick\""),
                "原生选择器按钮或它的请求没了"
            );
        }
        assert!(
            !body.contains("data-pick=\"quality\""),
            "非 Path 字段不该挂选择器"
        );
    }

    /// 页面发 camelCase、结构体字段是 snake_case：两种拼写都得收。
    /// 只认一种时拖拽上传会整条 400，而报出来的错是「missing field data_b64」，
    /// 完全看不出是键名对不上（这个坑是实跑截图抓到的）。
    #[test]
    fn upload_request_accepts_both_key_spellings() {
        let camel: UploadRequest =
            serde_json::from_str(r#"{"name":"a.bin","dataB64":"AAA"}"#).expect("camelCase 要能收");
        assert_eq!(camel.data_b64, "AAA");
        let snake: UploadRequest = serde_json::from_str(r#"{"name":"a.bin","data_b64":"BBB"}"#)
            .expect("snake_case 要能收");
        assert_eq!(snake.data_b64, "BBB");
    }

    #[tokio::test]
    async fn registry_run_rejects_unknown_command() {
        let state = registry_state();
        let mut args = HashMap::new();
        args.insert("x".to_string(), serde_json::json!(1));
        let req = RunRequest {
            args,
            cmd: Some("bogus".into()),
        };
        let resp = run_handler(State(state), Json(req)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn registry_run_accepts_valid_command() {
        let state = registry_state();
        let req = RunRequest {
            args: HashMap::new(),
            cmd: Some("ping".into()),
        };
        let resp = run_handler(State(state), Json(req)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn registry_run_without_handler_is_400() {
        // secret 无 handler 但可见性为默认 —— 这里把它当可见命令调用
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::new(
            "noh",
            CommandSchema {
                name: "noh".into(),
                about: "no handler".into(),
                args: vec![],
                subcommands: vec![],
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
            },
        ))
        .unwrap();
        let state2 = Arc::new(AppState {
            schema: Arc::new(CommandSchema {
                name: "noh".into(),
                about: "no handler".into(),
                args: vec![],
                subcommands: vec![],
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
            }),
            registry: Some(Arc::new(reg)),
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner: Arc::new(|_, _| Box::pin(async {})),
            token: "t".into(),
        });
        let req = RunRequest {
            args: HashMap::new(),
            cmd: Some("noh".into()),
        };
        let resp = run_handler(State(state2), Json(req)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn run_rejects_missing_required_arg_with_400() {
        let state = test_state();
        let mut args = HashMap::new();
        args.insert("quality".to_string(), serde_json::json!(30));
        let resp = run_handler(State(state), Json(RunRequest { args, cmd: None })).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn run_rejects_out_of_range_number_with_400() {
        let state = test_state();
        let mut args = HashMap::new();
        args.insert("input".to_string(), serde_json::json!("a.png"));
        args.insert("quality".to_string(), serde_json::json!(99));
        let resp = run_handler(State(state), Json(RunRequest { args, cmd: None })).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn run_accepts_valid_args() {
        let state = test_state();
        let mut args = HashMap::new();
        args.insert("input".to_string(), serde_json::json!("a.png"));
        args.insert("quality".to_string(), serde_json::json!(30));
        let resp = run_handler(State(state), Json(RunRequest { args, cmd: None })).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ── 安全与转义 ──

    #[test]
    fn html_escapes_dangerous_chars() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a\"b'c&d"), "a&quot;b&#39;c&amp;d");
        assert_eq!(html_escape("普通中文"), "普通中文");
    }

    #[test]
    fn base64_known_vectors() {
        // RFC 4648 已知向量
        assert_eq!(base64_decode("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(base64_decode("aGVsbG8"), Some(b"hello".to_vec())); // 无 padding
        assert_eq!(base64_decode("5Lit5paH"), Some("中文".as_bytes().to_vec()));
        assert_eq!(base64_decode("!!!"), None);
        assert_eq!(base64_decode(""), Some(Vec::new()));
    }

    #[test]
    fn sanitize_filename_blocks_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "____etc_passwd");
        assert_eq!(sanitize_filename("报告 最终.pdf"), "报告_最终.pdf");
        assert_eq!(sanitize_filename(""), "");
        assert_eq!(sanitize_filename(".."), "_");
    }

    #[tokio::test]
    async fn index_escapes_malicious_about() {
        let state = Arc::new(AppState {
            schema: Arc::new(CommandSchema {
                name: "evil".into(),
                about: "<script>alert(1)</script>".into(),
                args: vec![ArgSchema {
                    name: "p".into(),
                    about: "\"><img src=x>".into(),
                    kind: ArgKind::Text,
                    required: false,
                    default: Some(serde_json::json!("\"><svg>")),
                }],
                subcommands: vec![],
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
            }),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner: Arc::new(|_, _| Box::pin(async {})),
            token: "t".into(),
        });
        let resp = index(State(state), Query(HashMap::new())).await;
        let body = body_of(resp).await;
        assert!(!body.contains("<script>alert"), "about must be escaped");
        assert!(!body.contains("<img src=x>"), "about must be escaped");
        assert!(!body.contains("\"><svg>"), "default must be escaped");
    }

    #[tokio::test]
    async fn index_renders_dropzone_for_path_args() {
        let state = Arc::new(AppState {
            schema: Arc::new(CommandSchema {
                name: "up".into(),
                about: "upload".into(),
                args: vec![ArgSchema {
                    name: "file".into(),
                    about: "文件".into(),
                    kind: ArgKind::Path { must_exist: true },
                    required: true,
                    default: None,
                }],
                subcommands: vec![],
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
            }),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner: Arc::new(|_, _| Box::pin(async {})),
            token: "t".into(),
        });
        let resp = index(State(state), Query(HashMap::new())).await;
        let body = body_of(resp).await;
        assert!(body.contains("dropzone"), "Path 参数必须有拖拽上传组件");
        assert!(
            body.contains("data-must-exist=\"1\""),
            "must_exist 语义保留"
        );
        assert!(body.contains("type=\"file\""), "必须有文件选择入口");
    }
}
