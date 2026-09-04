use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, Request as AxumRequest, State},
    http::{HeaderMap, Method, StatusCode},
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

// ── State ──────────────────────────────────────────────────

struct AppState {
    schema: Arc<CommandSchema>,
    /// 多命令模式（`serve_registry`）：整张注册表
    registry: Option<Arc<Registry>>,
    sessions: Mutex<HashMap<String, tokio::sync::mpsc::Receiver<serde_json::Value>>>,
    runner: RunnerFn,
    token: String,
}

// ── Security middleware ───────────────────────────────────
//
// 防御 DNS rebinding / CSRF：
// 1. 所有请求的 Host 必须是回环地址（rebinding 时 Host 仍是攻击者域名，会被拒绝）
// 2. POST /run 的 Origin 若非回环地址则拒绝
// 3. POST /run 必须携带本次启动随机生成的 X-Lilyco-Token

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

    if request.method() == Method::POST && request.uri().path() == "/run" {
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

// ── HTML ───────────────────────────────────────────────────

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
) -> Html<String> {
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
                let sel = if c.schema.name == schema.name {
                    " selected"
                } else {
                    ""
                };
                opts.push_str(&format!(
                    "<option value=\"{}\"{}>{}</option>",
                    c.schema.name, sel, c.schema.name
                ));
            }
            cmd_nav = format!(
                " <select style=\"margin-left:12px;font-size:14px;padding:4px\" onchange=\"if(this.value)location='/?cmd='+encodeURIComponent(this.value)\">{opts}</select>"
            );
        }
    }

    let mut fields_html = String::new();
    let mut field_js_meta = String::new();

    for (i, arg) in schema.args.iter().enumerate() {
        if i > 0 {
            field_js_meta.push(',');
        }
        field_js_meta.push_str(&format!(
            "{{name:\"{}\",kind:\"{}\"}}",
            arg.name,
            kind_name(&arg.kind)
        ));

        let req_mark = if arg.required {
            "<span class=\"req-mark\">*</span>"
        } else {
            ""
        };
        let label = format!("{}{}", arg.about, req_mark);

        let widget = match &arg.kind {
            ArgKind::Flag => {
                let ck = matches!(&arg.default, Some(serde_json::Value::Bool(true)))
                    .then_some(" checked")
                    .unwrap_or("");
                format!(
                    "<input type=\"checkbox\" id=\"field-{}\"{} lay-skin=\"primary\" title=\"{}\">",
                    arg.name, ck, arg.about
                )
            }
            ArgKind::Text | ArgKind::Path { .. } => {
                let dv = arg.default.as_ref().and_then(|d| d.as_str()).unwrap_or("");
                let req_a = if arg.required { " required" } else { "" };
                format!("<input type=\"text\" id=\"field-{}\" placeholder=\"{}\"{}{} value=\"{}\" class=\"layui-input\">",
                    arg.name, arg.about, req_a, if arg.required {" lay-reqtext=\"required\""} else {""}, dv)
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
                let req_a = if arg.required { " required" } else { "" };
                format!("<input type=\"number\" id=\"field-{}\" value=\"{}\" step=\"any\"{}{}{} class=\"layui-input\">",
                    arg.name, dv, min_a, max_a, req_a)
            }
            ArgKind::Enum { values } => {
                let mut opts = String::new();
                for v in values {
                    let sel = if arg.default.as_ref().and_then(|d| d.as_str()) == Some(v.as_str()) {
                        " selected"
                    } else {
                        ""
                    };
                    opts.push_str(&format!("<option value=\"{v}\"{sel}>{v}</option>"));
                }
                format!(
                    "<select id=\"field-{}\" lay-search=\"\">{}</select>",
                    arg.name, opts
                )
            }
            ArgKind::List { .. } => {
                let mut inputs = String::new();
                for j in 0..3 {
                    inputs.push_str(&format!(
                        "<input type=\"text\" id=\"field-{}-{}\" placeholder=\"{}\" class=\"layui-input\" style=\"margin-bottom:6px\">",
                        arg.name, j, arg.about
                    ));
                }
                format!("<div>{inputs}</div>")
            }
        };

        if matches!(&arg.kind, ArgKind::Flag) {
            fields_html.push_str(&format!(
                "<div class=\"layui-form-item\"><div class=\"layui-input-block\">{}</div></div>\n",
                widget
            ));
        } else {
            fields_html.push_str(&format!(
                "<div class=\"layui-form-item\"><label class=\"layui-form-label\">{}</label><div class=\"layui-input-block\">{}</div></div>\n",
                label, widget
            ));
        }
    }

    Html(
        HTML_TEMPLATE
            .replace("{layui_css}", include_str!("../assets/layui.css"))
            .replace("{about}", &schema.about)
            .replace("{cmd_name}", &schema.name)
            .replace("{cmd_nav}", &cmd_nav)
            .replace("{cmd_js}", &schema.name)
            .replace("{fields_html}", &fields_html)
            .replace("{field_js_meta}", &field_js_meta)
            .replace("{token}", &state.token),
    )
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

const HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html><head>
<meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<meta name="lilyco-token" content="{token}">
<title>{cmd_name} · Web 控制台</title>
<style>{layui_css}</style>
<style>
/* ── Flex 布局（自有，不依赖外部 CSS）─ 彻底避免 absolute/fixed 定位导致的重叠 ── */
/* 全局：禁用一切 absolute/fixed 定位元素 */
*{position:static !important}
.layui-progress{position:relative !important}
.layui-progress-bar{position:relative !important}
.layui-input-block .layui-input, .layui-input, .layui-select, select, .layui-textarea{position:relative !important}
.layui-tooltip, .layui-dropdown, .layui-nav-child, .layui-table-tips{display:none !important}

body{background:#f2f3f5;padding:20px}
.main-card{max-width:820px;margin:0 auto;background:#fff;border-radius:6px;box-shadow:0 1px 6px rgba(0,0,0,.08);overflow:hidden}
.main-card-body, .layui-card-body{padding:20px 24px 24px}
.layui-card-header{padding:16px 24px;font-size:18px;font-weight:bold;border-bottom:1px solid #eee}

/* 表单：每个字段一行，label 与输入框用 flex 并排 */
.layui-form-item{display:flex;align-items:flex-start;margin-bottom:16px;gap:12px}
.layui-form-item .layui-form-label{flex:0 0 190px;min-width:190px;text-align:left;white-space:normal;line-height:1.4;padding:9px 0;box-sizing:border-box;color:#333}
.layui-form-item .layui-input-block{flex:1 1 auto;min-width:0}
.layui-form-label .req{color:#ff5722;margin-left:2px}
.cli-preview{font-family:Consolas,monospace;font-size:13px;color:#16b777;background:#2f363d;padding:12px 16px;border-radius:4px;word-break:break-all;margin-top:16px;min-height:20px;position:relative !important}
.cli-preview::before{content:"$ ";color:#8b949e}
#out{margin-top:20px}
#log{max-height:260px;overflow-y:auto;font-size:13px;background:#2f363d;color:#e1e4e8;padding:12px;border-radius:4px;white-space:pre-wrap;font-family:Consolas,monospace;position:relative !important}
#log .err{color:#ff5722}
#result{font-size:13px;margin-top:12px;overflow-x:auto;background:#f8f8f8;padding:12px;border-radius:4px}
.req-mark{color:#ff5722;margin-left:2px}
@media(max-width:640px){.layui-form-item{flex-direction:column;gap:4px}.layui-form-item .layui-form-label{flex:0 0 auto;min-width:0}}
</style></head><body>
<div class="layui-card main-card">
<div class="layui-card-header" style="font-size:18px;font-weight:bold">{cmd_name}{cmd_nav}</div>
<div class="layui-card-body">
<details class="about" id="about"><summary>关于 {cmd_name}（点击展开）</summary><p style="color:#666;margin:10px 0 0">{about}</p></details>
<form class="layui-form" id="form" lay-filter="form">
{fields_html}
<div style="margin-top:24px;display:flex;gap:12px">
<button type="submit" class="layui-btn layui-btn-normal">▶ Run</button>
<button type="button" class="layui-btn layui-btn-primary" onclick="copyCmd()">📋 Copy CLI</button>
</div>
<div class="cli-preview" id="preview">{cmd_name}</div>
</form>
<div id="out" style="display:none">
<fieldset class="layui-elem-field layui-field-title" style="margin-top:24px"><legend>Output</legend></fieldset>
<div class="layui-progress" lay-showpercent="true" lay-filter="pbar-wrap" style="margin-bottom:12px"><div class="layui-progress-bar" lay-percent="0%"><span class="layui-progress-text">0%</span></div></div>
<div id="log"></div>
<pre id="result"></pre>
</div>
</div>
</div>
<script src="https://cdn.staticfile.net/layui/2.9.8/layui.js"></script>
<script>
layui.use(['element','form'],function(){
var element=layui.element,form=layui.form;
const TOKEN=document.querySelector('meta[name="lilyco-token"]').content;
const CMD="{cmd_js}";
const SCHEMA={name:"{cmd_name}",args:[{field_js_meta}]};
const preview=document.getElementById("preview");
const out=document.getElementById("out");
const logEl=document.getElementById("log");
const resultEl=document.getElementById("result");
function updatePreview(){
var parts=[SCHEMA.name];
for(var a of SCHEMA.args){
var el=document.getElementById("field-"+a.name);if(!el)continue;
if(a.kind==="Flag"){if(el.checked)parts.push("--"+a.name)}
else if(a.kind==="List"){document.querySelectorAll("[id^=field-"+a.name+"-]").forEach(function(inp){if(inp.value)parts.push("--"+a.name+" "+inp.value)})}
else{if(el.value){var v=a.kind==="Path"&&el.value.includes(" ")?'"'+el.value+'"':el.value;parts.push("--"+a.name+" "+v)}}}
preview.textContent=parts.join(" ")}
document.querySelectorAll("input,select").forEach(function(el){el.addEventListener("input",updatePreview)});
document.getElementById("form").addEventListener("submit",async function(e){
e.preventDefault();out.style.display="block";logEl.innerHTML="";resultEl.textContent="";
var data={};
for(var a of SCHEMA.args){
if(a.kind==="List"){data[a.name]=[];document.querySelectorAll("[id^=field-"+a.name+"-]").forEach(function(inp){if(inp.value)data[a.name].push(inp.value)})}
else{var el=document.getElementById("field-"+a.name);
if(a.kind==="Flag")data[a.name]=el.checked;
else if(el.value==="")data[a.name]=null;
else if(a.kind==="Number"){var n=Number(el.value);data[a.name]=isNaN(n)?el.value:n}
else data[a.name]=el.value}}
var sid;
try{var resp=await fetch("/run",{method:"POST",headers:{"Content-Type":"application/json","X-Lilyco-Token":TOKEN},body:JSON.stringify({args:data,cmd:CMD})});
if(!resp.ok){var t=await resp.text();logEl.innerHTML="<span class=err>"+(t||("Server error: "+resp.status))+"</span>";return;}
var j=await resp.json();sid=j.session_id;}catch(err){logEl.innerHTML+="<span class=err>Network error: "+err+"</span>";return}
var es=new EventSource("/progress/"+sid);
es.onmessage=function(ev){var p=JSON.parse(ev.data);
if(p.type==="started"){logEl.innerHTML+=(p.message||"Running...")+"\n"}
else if(p.type==="tick"){var pct=(p.percent*100).toFixed(0);element.progress('pbar-wrap',pct+'%');logEl.innerHTML+=(p.message||"")+"\n"}
else if(p.type==="log"){logEl.innerHTML+="["+(p.level||"info")+"] "+(p.message||"")+"\n"}
else if(p.type==="done"){element.progress('pbar-wrap','100%');resultEl.textContent=JSON.stringify(p.result,null,2);es.close()}
else if(p.type==="error"){logEl.innerHTML+="<span class=err>ERROR: "+(p.message||"")+"</span>\n";es.close()}
logEl.scrollTop=logEl.scrollHeight};
es.onerror=function(){es.close()};
});
function copyCmd(){navigator.clipboard.writeText(preview.textContent)}
});</script></body></html>"#;

// ── Handlers ───────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct RunRequest {
    args: HashMap<String, serde_json::Value>,
    /// 多命令模式：要执行的命令名（单命令模式忽略）
    #[serde(default)]
    cmd: Option<String>,
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
        tokio::spawn(async move { run_progress(handler, args_value, tx).await });
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
        }
    }

    fn test_state() -> Arc<AppState> {
        let runner: RunnerFn = Arc::new(|_args, _tx| Box::pin(async {}));
        Arc::new(AppState {
            schema: Arc::new(schema_with_required_number()),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
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
            }),
            registry: Some(Arc::new(two_command_registry())),
            sessions: Mutex::new(HashMap::new()),
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
        assert!(resp.0.contains("ping"), "fallback to first visible");
        assert!(resp.0.contains("select"), "nav dropdown expected");
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
        let state = registry_state();
        // secret 无 handler 但可见性为默认 —— 这里把它当可见命令调用
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::new(
            "noh",
            CommandSchema {
                name: "noh".into(),
                about: "no handler".into(),
                args: vec![],
                subcommands: vec![],
            },
        ))
        .unwrap();
        let state2 = Arc::new(AppState {
            schema: Arc::new(CommandSchema {
                name: "noh".into(),
                about: "no handler".into(),
                args: vec![],
                subcommands: vec![],
            }),
            registry: Some(Arc::new(reg)),
            sessions: Mutex::new(HashMap::new()),
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
}
