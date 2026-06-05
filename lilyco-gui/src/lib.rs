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

use lilyco_core::prelude::*;
use lilyco_core::schema::{ArgKind, CommandSchema};

// ── GuiRenderer ───────────────────────────────────────────

pub struct GuiRenderer {
    port: u16,
}

pub type RunnerFn = Arc<
    dyn Fn(HashMap<String, serde_json::Value>) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, AppError>> + Send>>
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

        axum::serve(listener, app).await.unwrap();
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

        let req = if arg.required { " required" } else { "" };
        let req_label = if arg.required { " class=\"req\"" } else { "" };

        let widget = match &arg.kind {
            ArgKind::Flag => {
                let ck = matches!(&arg.default, Some(serde_json::Value::Bool(true))).then_some(" checked").unwrap_or("");
                format!("<input type=\"checkbox\" id=\"field-{}\"{}>", arg.name, ck)
            }
            ArgKind::Text | ArgKind::Path { .. } => {
                let dv = arg.default.as_ref().and_then(|d| d.as_str()).unwrap_or("");
                format!("<input type=\"text\" id=\"field-{}\" placeholder=\"{}\"{} value=\"{}\">", arg.name, arg.about, req, dv)
            }
            ArgKind::Number { min, max } => {
                let dv = arg.default.as_ref().and_then(|d| d.as_f64())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| min.map(|m| m.to_string()).unwrap_or_default());
                let min_a = min.map(|m| format!(" min=\"{m}\"")).unwrap_or_default();
                let max_a = max.map(|m| format!(" max=\"{m}\"")).unwrap_or_default();
                format!("<input type=\"number\" id=\"field-{}\"{} value=\"{}\"{}{}>", arg.name, req, dv, min_a, max_a)
            }
            ArgKind::Enum { values } => {
                let mut opts = String::new();
                for v in values {
                    let sel = if arg.default.as_ref().and_then(|d| d.as_str()) == Some(v.as_str()) { " selected" } else { "" };
                    opts.push_str(&format!("<option value=\"{v}\"{sel}>{v}</option>"));
                }
                format!("<select id=\"field-{}\"{}>{}</select>", arg.name, req, opts)
            }
            ArgKind::List { .. } => {
                let mut inputs = String::new();
                for j in 0..3 {
                    inputs.push_str(&format!(
                        "<input type=\"text\" id=\"field-{}-{}\" placeholder=\"{} #{}\" style=\"margin-bottom:4px\">",
                        arg.name, j, arg.about, j + 1
                    ));
                }
                format!("<div style=\"display:flex;flex-direction:column;gap:4px;flex:1\">{inputs}</div>")
            }
        };

        fields_html.push_str(&format!(
            "<div class=\"field\"><label for=\"field-{}\"{}>{}</label>{}</div>\n",
            arg.name, req_label, arg.about, widget
        ));
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
<html lang="en"><head>
<meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>{title}</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:-apple-system,BlinkMacSystemFont,sans-serif;background:#0f1117;color:#e1e4e8;max-width:760px;margin:48px auto;padding:0 24px}
h1{font-size:20px;margin-bottom:4px}
.about{color:#8b949e;margin-bottom:28px;font-size:14px}
.field{display:flex;align-items:center;margin-bottom:14px;gap:12px}
.field label{width:150px;text-align:right;font-size:14px;flex-shrink:0}
.field label.req::after{content:" *";color:#f85149}
.field input,.field select{flex:1;padding:8px 12px;background:#21262d;border:1px solid #30363d;border-radius:6px;color:#e1e4e8;font-size:14px}
.field input:focus,.field select:focus{outline:none;border-color:#58a6ff;box-shadow:0 0 0 1px #58a6ff44}
.actions{margin-top:24px;display:flex;gap:12px}
.actions button{padding:10px 24px;border-radius:6px;font-size:14px;cursor:pointer;border:none}
.btn-run{background:#238636;color:#fff}.btn-run:hover{background:#2ea043}
.btn-copy{background:#21262d;color:#c9d1d9;border:1px solid #30363d}
.cli-preview{margin-top:14px;padding:10px 14px;background:#161b22;border-radius:6px;font-family:monospace;font-size:13px;color:#7ee787;word-break:break-all}
.output{margin-top:28px}
.output h3{font-size:16px;margin-bottom:10px}
.progress-bar{width:100%;height:8px;background:#21262d;border-radius:4px;overflow:hidden;margin-bottom:10px}
.progress-bar .fill{height:100%;background:#238636;transition:width .3s;border-radius:4px;width:0%}
.log{max-height:220px;overflow-y:auto;font-size:12px;color:#8b949e;background:#161b22;padding:10px;border-radius:6px;white-space:pre-wrap}
.log .err{color:#f85149}
</style></head><body>
<h1>{title}</h1>
<p class="about">{about}</p>
<form id="form">
{fields_html}
<div class="actions">
<button type="submit" class="btn-run">▶ Run</button>
<button type="button" class="btn-copy" onclick="copyCmd()">📋 Copy CLI</button>
</div>
<div class="cli-preview" id="preview">$ {cmd_name}</div>
</form>
<div class="output" id="out" style="display:none">
<h3>Output</h3>
<div class="progress-bar"><div class="fill" id="pbar"></div></div>
<div class="log" id="log"></div>
<pre id="result" style="color:#e1e4e8;font-size:13px;margin-top:10px;overflow-x:auto"></pre>
</div>
<script>
const SCHEMA={name:"{cmd_name}",args:[{field_js_meta}]};
const form=document.getElementById("form");
const preview=document.getElementById("preview");
const out=document.getElementById("out");
const pbar=document.getElementById("pbar");
const logEl=document.getElementById("log");
const resultEl=document.getElementById("result");
function updatePreview(){var parts=[SCHEMA.name];
for(var a of SCHEMA.args){var el=document.getElementById("field-"+a.name);if(!el)continue;
if(a.kind==="Flag"){if(el.checked)parts.push("--"+a.name)}
else if(a.kind==="List"){document.querySelectorAll("[id^=field-"+a.name+"-]").forEach(function(inp){if(inp.value)parts.push("--"+a.name+" "+inp.value)})}
else{if(el.value){var v=a.kind==="Path"&&el.value.includes(" ")?'"'+el.value+'"':el.value;parts.push("--"+a.name+" "+v)}}}
preview.textContent="$ "+parts.join(" ")}
document.querySelectorAll("input,select").forEach(function(el){el.addEventListener("input",updatePreview)});
form.addEventListener("submit",async function(e){e.preventDefault();out.style.display="block";pbar.style.width="0%";logEl.innerHTML="";resultEl.textContent="";
var data={};
for(var a of SCHEMA.args){if(a.kind==="List"){data[a.name]=[];document.querySelectorAll("[id^=field-"+a.name+"-]").forEach(function(inp){if(inp.value)data[a.name].push(inp.value)})}
else{var el=document.getElementById("field-"+a.name);if(a.kind==="Flag")data[a.name]=el.checked;else data[a.name]=el.value}}
var sid=Math.random().toString(36).slice(2);
var es=new EventSource("/progress/"+sid);
es.onmessage=function(ev){var p=JSON.parse(ev.data);
if(p.type==="tick"){pbar.style.width=(p.percent*100)+"%";logEl.innerHTML+=(p.message||"")+"\n"}
else if(p.type==="log"){logEl.innerHTML+="["+(p.level||"info")+"] "+(p.message||"")+"\n"}
else if(p.type==="done"){resultEl.textContent=JSON.stringify(p.result,null,2);es.close()}
else if(p.type==="error"){logEl.innerHTML+="<span class=err>ERROR: "+(p.message||"")+"</span>\n";es.close()}
logEl.scrollTop=logEl.scrollHeight};
es.onerror=function(){es.close()};
try{await fetch("/run",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({session_id:sid,args:data})})}catch(err){logEl.innerHTML+="<span class=err>Failed: "+err+"</span>\n"}
});
function copyCmd(){navigator.clipboard.writeText(preview.textContent.slice(2))}
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
        let result = runner(req.args).await;
        let msg = match result {
            Ok(val) => serde_json::json!({"type":"done","result":val,"duration_ms":0}),
            Err(e) => serde_json::json!({"type":"error","code":1,"message":e.to_string()}),
        };
        let _ = tx.send(msg).await;
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
