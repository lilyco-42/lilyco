use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse,
    },
    routing::{get, post},
    Router,
};
use tokio::sync::Mutex;

use lilyco_core::schema::{ArgKind, CommandSchema};

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
    pub fn new(port: u16) -> Self { Self { port } }

    pub async fn serve(&self, schema: CommandSchema, runner: RunnerFn) {
        let state = Arc::new(AppState {
            schema: Arc::new(schema),
            sessions: Mutex::new(HashMap::new()),
            runner,
        });

        let app = Router::new()
            .route("/", get(index))
            .route("/run", post(run_handler))
            .route("/progress/{id}", get(progress_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await.unwrap();
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
}

// ── State ──────────────────────────────────────────────────

struct AppState {
    schema: Arc<CommandSchema>,
    sessions: Mutex<HashMap<String, tokio::sync::mpsc::Receiver<serde_json::Value>>>,
    runner: RunnerFn,
}

// ── HTML ───────────────────────────────────────────────────

async fn index(State(state): State<Arc<AppState>>) -> Html<String> {
    let schema = &state.schema;
    let mut fields_html = String::new();
    let mut field_js_meta = String::new();

    for (i, arg) in schema.args.iter().enumerate() {
        if i > 0 { field_js_meta.push(','); }
        field_js_meta.push_str(&format!("{{name:\"{}\",kind:\"{}\"}}", arg.name, kind_name(&arg.kind)));

        let req_mark = if arg.required { "<span class=\"req-mark\">*</span>" } else { "" };
        let label = format!("{}{}", arg.about, req_mark);

        let widget = match &arg.kind {
            ArgKind::Flag => {
                let ck = matches!(&arg.default, Some(serde_json::Value::Bool(true))).then_some(" checked").unwrap_or("");
                format!("<input type=\"checkbox\" id=\"field-{}\"{} lay-skin=\"primary\" title=\"{}\">",
                    arg.name, ck, arg.about)
            }
            ArgKind::Text | ArgKind::Path { .. } => {
                let dv = arg.default.as_ref().and_then(|d| d.as_str()).unwrap_or("");
                format!("<input type=\"text\" id=\"field-{}\" placeholder=\"{}\"{} value=\"{}\" class=\"layui-input\">",
                    arg.name, arg.about, if arg.required {" lay-reqtext=\"required\""} else {""}, dv)
            }
            ArgKind::Number { min, max } => {
                let dv = arg.default.as_ref().and_then(|d| d.as_f64())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| min.map(|m| m.to_string()).unwrap_or_default());
                let min_a = min.map(|m| format!(" min=\"{m}\"")).unwrap_or_default();
                let max_a = max.map(|m| format!(" max=\"{m}\"")).unwrap_or_default();
                format!("<input type=\"number\" id=\"field-{}\" value=\"{}\"{}{} class=\"layui-input\">",
                    arg.name, dv, min_a, max_a)
            }
            ArgKind::Enum { values } => {
                let mut opts = String::new();
                for v in values {
                    let sel = if arg.default.as_ref().and_then(|d| d.as_str()) == Some(v.as_str()) { " selected" } else { "" };
                    opts.push_str(&format!("<option value=\"{v}\"{sel}>{v}</option>"));
                }
                format!("<select id=\"field-{}\" lay-search=\"\">{}</select>", arg.name, opts)
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

    Html(HTML_TEMPLATE
        .replace("{title}", &format!("{} — {}", schema.name, schema.about))
        .replace("{about}", &schema.about)
        .replace("{cmd_name}", &schema.name)
        .replace("{fields_html}", &fields_html)
        .replace("{field_js_meta}", &field_js_meta))
}

fn kind_name(kind: &ArgKind) -> &'static str {
    match kind {
        ArgKind::Flag => "Flag", ArgKind::Text => "Text",
        ArgKind::Number { .. } => "Number", ArgKind::Enum { .. } => "Enum",
        ArgKind::Path { .. } => "Path", ArgKind::List { .. } => "List",
    }
}

const HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html><head>
<meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>{title}</title>
<link rel="stylesheet" href="https://unpkg.com/layui@2.9.8/dist/css/layui.css">
<style>
body{{background:#f2f3f5;padding:20px}}
.main-card{{max-width:820px;margin:0 auto}}
.cli-preview{{font-family:Consolas,monospace;font-size:13px;color:#16b777;background:#2f363d;padding:12px 16px;border-radius:4px;word-break:break-all;margin-top:16px;min-height:20px}}
.cli-preview::before{{content:"$ ";color:#8b949e}}
#out{{margin-top:20px}}
#log{{max-height:260px;overflow-y:auto;font-size:13px;background:#2f363d;color:#e1e4e8;padding:12px;border-radius:4px;white-space:pre-wrap;font-family:Consolas,monospace}}
#log .err{{color:#ff5722}}
#result{{font-size:13px;margin-top:12px;overflow-x:auto;background:#f8f8f8;padding:12px;border-radius:4px}}
.req-mark{{color:#ff5722;margin-left:2px}}
</style></head><body>
<div class="layui-card main-card">
<div class="layui-card-header" style="font-size:18px;font-weight:bold">{{title}}</div>
<div class="layui-card-body">
<p style="color:#666;margin-bottom:20px">{{about}}</p>
<form class="layui-form" id="form" lay-filter="form">
{{fields_html}}
<div style="margin-top:24px;display:flex;gap:12px">
<button type="submit" class="layui-btn layui-btn-normal">▶ Run</button>
<button type="button" class="layui-btn layui-btn-primary" onclick="copyCmd()">📋 Copy CLI</button>
</div>
<div class="cli-preview" id="preview">{{cmd_name}}</div>
</form>
<div id="out" style="display:none">
<fieldset class="layui-elem-field layui-field-title" style="margin-top:24px"><legend>Output</legend></fieldset>
<div class="layui-progress" lay-showpercent="true" lay-filter="pbar-wrap" style="margin-bottom:12px"><div class="layui-progress-bar" lay-percent="0%"><span class="layui-progress-text">0%</span></div></div>
<div id="log"></div>
<pre id="result"></pre>
</div>
</div>
</div>
<script src="https://unpkg.com/layui@2.9.8/dist/layui.js"></script>
<script>
layui.use(['element','form'],function(){{var element=layui.element,form=layui.form;
const SCHEMA={{name:"{{cmd_name}}",args:[{{field_js_meta}}]}};
const preview=document.getElementById("preview");
const out=document.getElementById("out");
const logEl=document.getElementById("log");
const resultEl=document.getElementById("result");
function updatePreview(){{var parts=[SCHEMA.name];
for(var a of SCHEMA.args){{var el=document.getElementById("field-"+a.name);if(!el)continue;
if(a.kind==="Flag"){{if(el.checked)parts.push("--"+a.name)}}
else if(a.kind==="List"){{document.querySelectorAll("[id^=field-"+a.name+"-]").forEach(function(inp){{if(inp.value)parts.push("--"+a.name+" "+inp.value)}})}}
else{{if(el.value){{var v=a.kind==="Path"&&el.value.includes(" ")?'"'+el.value+'"':el.value;parts.push("--"+a.name+" "+v)}}}}}}
preview.textContent=parts.join(" ")}}
document.querySelectorAll("input,select").forEach(function(el){{el.addEventListener("input",updatePreview)}});
document.getElementById("form").addEventListener("submit",async function(e){{e.preventDefault();out.style.display="block";logEl.innerHTML="";resultEl.textContent="";
var data={{}};
for(var a of SCHEMA.args){{if(a.kind==="List"){{data[a.name]=[];document.querySelectorAll("[id^=field-"+a.name+"-]").forEach(function(inp){{if(inp.value)data[a.name].push(inp.value)}})}}
else{{var el=document.getElementById("field-"+a.name);if(a.kind==="Flag")data[a.name]=el.checked;else data[a.name]=el.value}}}}
var sid=Math.random().toString(36).slice(2);
try{var resp=await fetch("/run",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({session_id:sid,args:data})});
if(!resp.ok){logEl.innerHTML="<span class=err>Server error: "+resp.status+"</span>";return;}}catch(err){logEl.innerHTML+="<span class=err>Network error: "+err+"</span>";return}
var es=new EventSource("/progress/"+sid);
es.onmessage=function(ev){var p=JSON.parse(ev.data);
if(p.type==="started"){logEl.innerHTML+=(p.message||"Running...")+"
"}
else if(p.type==="tick"){var pct=(p.percent*100).toFixed(0);element.progress('pbar-wrap',pct+'%');logEl.innerHTML+=(p.message||"")+"
"}
else if(p.type==="log"){logEl.innerHTML+="["+(p.level||"info")+"] "+(p.message||"")+"
"}
else if(p.type==="done"){element.progress('pbar-wrap','100%');resultEl.textContent=JSON.stringify(p.result,null,2);es.close()}
else if(p.type==="error"){logEl.innerHTML+="<span class=err>ERROR: "+(p.message||"")+"</span>
";es.close()}
logEl.scrollTop=logEl.scrollHeight};
es.onerror=function(){es.close()}});
function copyCmd(){navigator.clipboard.writeText(preview.textContent)}});
</script></body></html>"#;

// ── Handlers ───────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct RunRequest {
    session_id: String,
    args: HashMap<String, serde_json::Value>,
}

async fn run_handler(
    State(state): State<Arc<AppState>>,
    axum::Json(req): axum::Json<RunRequest>,
) -> impl IntoResponse {
    let (tx, rx) = tokio::sync::mpsc::channel::<serde_json::Value>(128);
    {
        let mut sessions = state.sessions.lock().await;
        sessions.insert(req.session_id, rx);
    }

    let runner = state.runner.clone();

    tokio::spawn(async move {
        runner(req.args, tx).await;
    });

    "OK"
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
