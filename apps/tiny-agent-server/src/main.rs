//! HTTP backend for Tiny Agent.
//!
//! The UI is deliberately **not** part of this binary's windowing model: this
//! process only speaks HTTP and Server-Sent Events, and any frontend (the
//! bundled page, a React app, a terminal, another agent) is a client.
//!
//! Two consequences worth stating, because they are the point of the split:
//!   * a slow model can never freeze the UI — the agent loop runs on a blocking
//!     thread and pushes events into a channel the request streams;
//!   * the same backend serves the desktop shell, the browser and CI tests.

use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tiny_lilyco::agent::llm::{Llm, ScriptedLlm};
use tiny_lilyco::agent::registry::Registry;
use tiny_lilyco::agent::runtime::{drive, Budget, Host, Outcome};
use tiny_lilyco::agent::tool::{ObsBuf, ObsKind, Tool, ToolError};
use tiny_lilyco::agent::tools::{AstGrep, EditFile, ListDir, ReadFile};
use tokio_stream::wrappers::UnboundedReceiverStream;

/// The page, kept in one file so the server has no asset pipeline.
const INDEX: &str = include_str!("../web/index.html");

/// Everything a request needs.
#[derive(Clone)]
struct App {
    registry: &'static Registry,
}

/// One run request.
#[derive(Deserialize)]
struct RunRequest {
    goal: String,
}

/// One streamed event.
#[derive(Serialize)]
struct EventBody {
    kind: &'static str,
    text: String,
}

/// Start the server on `TINY_AGENT_ADDR` (default `127.0.0.1:8787`).
#[tokio::main]
async fn main() {
    let registry = build_registry(std::env::current_dir().unwrap_or_default().as_path());
    let app = App { registry };
    let router = Router::new()
        .route("/", get(index))
        .route("/api/health", get(health))
        .route("/api/run", post(run))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(Arc::new(app));
    let addr = std::env::var("TINY_AGENT_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("cannot bind the listener");
    println!("tiny-agent-server listening on http://{addr}");
    axum::serve(listener, router).await.expect("server failed");
}

async fn index() -> Html<&'static str> {
    Html(INDEX)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "ok": true, "model": model_label() }))
}

/// Stream one agent run as Server-Sent Events.
async fn run(State(app): State<Arc<App>>, Json(req): Json<RunRequest>) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let registry = app.registry;
    let goal = req.goal;
    let (tx, rx) = std_mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut host = StreamHost {
            registry,
            tx: tx.clone(),
        };
        let mut llm = choose_llm(registry, &goal);
        let outcome = drive(&goal, &mut *llm, registry, &mut host, Budget::new(8, 3));
        let body = EventBody {
            kind: "done",
            text: summary(&outcome),
        };
        if let Ok(json) = serde_json::to_string(&body) {
            let _ = tx.send(json);
        }
    });
    let stream = std_mpsc_into_stream(rx).map(|data| Ok(Event::default().data(data)));
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(15)))
}

/// Bridge a std channel into an async stream without pulling in another runtime.
fn std_mpsc_into_stream(rx: std_mpsc::Receiver<String>) -> UnboundedReceiverStream<String> {
    let (tx, out) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(item) = rx.recv() {
            if tx.send(item).is_err() {
                break;
            }
        }
    });
    UnboundedReceiverStream::new(out)
}

struct StreamHost {
    registry: &'static Registry,
    tx: std_mpsc::Sender<String>,
}

impl StreamHost {
    fn emit(&self, kind: &'static str, text: String) {
        if let Ok(json) = serde_json::to_string(&EventBody { kind, text }) {
            let _ = self.tx.send(json);
        }
    }
}

impl Host for StreamHost {
    fn checkpoint(&mut self, label: &str) -> Result<(), &'static str> {
        self.emit("step", format!("checkpoint · {label}"));
        Ok(())
    }

    fn undo(&mut self) -> Result<(), &'static str> {
        self.emit("step", "rollback".to_string());
        Ok(())
    }

    fn call(&mut self, tool: &str, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let handle = self.registry.find(tool).ok_or(ToolError::UnknownTool)?;
        if handle.spec().mutating && std::env::var("TINY_AGENT_APPLY").is_err() {
            self.emit("step", format!("{tool} · 需要 --apply，已跳过"));
            return Err(ToolError::NotPermitted("read-only"));
        }
        let kind = handle.call(args, out)?;
        self.emit("tool", format!("{tool}({args})"));
        for line in out.as_str().lines().take(40) {
            self.emit("output", line.to_string());
        }
        Ok(kind)
    }
}

fn summary(outcome: &Outcome) -> String {
    if outcome.calls == 0 && outcome.failures == 0 {
        return "模型没有回话：检查设置里的接口地址与 Key。".to_string();
    }
    format!(
        "完成 · {} 步 · {} 次失败",
        outcome.calls, outcome.failures
    )
}

fn model_label() -> &'static str {
    if std::env::var("TINY_AGENT_FAKE").is_ok() {
        return "fake";
    }
    "remote"
}

/// Pick a model for one run.
fn choose_llm(registry: &'static Registry, goal: &str) -> Box<dyn Llm> {
    if std::env::var("TINY_AGENT_FAKE").is_ok() {
        return Box::new(fake_plan(goal));
    }
    use tiny_lilyco::agent::llm_http::{Config, HttpLlm};
    if let Ok(base) = std::env::var("TINY_AGENT_BASE_URL") {
        let config = Config {
            base_url: base,
            api_key: std::env::var("TINY_AGENT_API_KEY").unwrap_or_default(),
            model: std::env::var("TINY_AGENT_MODEL").unwrap_or_else(|_| "openai".to_string()),
        };
        return Box::new(HttpLlm::new(config, registry));
    }
    let config = Config {
        base_url: "https://text.pollinations.ai/openai".to_string(),
        api_key: "none".to_string(),
        model: "openai".to_string(),
    };
    Box::new(HttpLlm::new(config, registry))
}

/// Deterministic plan used by `TINY_AGENT_FAKE=1` and by CI.
fn fake_plan(goal: &str) -> ScriptedLlm {
    let mut plan = ScriptedLlm::new();
    if let Some(pattern) = goal.split('`').nth(1) {
        let _ = plan.push("ast_grep", pattern);
    } else {
        let _ = plan.push("list_dir", "");
    }
    plan
}

fn build_registry(root: &std::path::Path) -> &'static Registry {
    let read: &'static dyn Tool = Box::leak(Box::new(ReadFile::new(root)));
    let list: &'static dyn Tool = Box::leak(Box::new(ListDir::new(root)));
    let edit: &'static dyn Tool = Box::leak(Box::new(EditFile::new(root)));
    let ast: &'static dyn Tool = Box::leak(Box::new(AstGrep::new(root, AstGrep::locate())));
    let reg: &'static mut Registry = Box::leak(Box::new(Registry::new()));
    let _ = reg.register(read);
    let _ = reg.register(list);
    let _ = reg.register(edit);
    let _ = reg.register(ast);
    reg
}
