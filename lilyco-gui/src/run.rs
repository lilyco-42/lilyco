//! 执行侧：`POST /run` 分发、`GET /progress/{sid}` SSE 流、`POST /cancel/{sid}`。
//!
//! 单命令模式走调用方给的 `RunnerFn`；多命令模式（registry）按 `cmd` 显式取 handler，
//! 并把取消句柄登记进 `AppState::cancels`。两条路都经 `core::executor`，
//! 与 CLI / TUI / MCP 共用同一执行宿主。

use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{
    sse::{Event, KeepAlive, Sse},
    IntoResponse, Response,
};
use axum::Json;

use lilyco_core::executor;
use lilyco_core::registry::Handler;
use lilyco_core::Progress;

use crate::state::AppState;
use crate::util::generate_id;

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

/// 执行 handler 并把进度事件流式转发到 SSE 通道（单命令 / 多命令共用）
pub(crate) async fn run_progress(
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

// ── Handlers ───────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub(crate) struct RunRequest {
    args: HashMap<String, serde_json::Value>,
    /// 多命令模式：要执行的命令名（单命令模式忽略）
    #[serde(default)]
    cmd: Option<String>,
}

pub(crate) async fn run_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RunRequest>,
) -> Response {
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

/// 请求取消：置取消标志；命令侧通过 `ctx.is_cancelled()` 响应
pub(crate) async fn cancel_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let flag = state.cancels.lock().await.get(&id).cloned();
    match flag {
        Some(f) => {
            f.store(true, Ordering::Relaxed);
            Json(serde_json::json!({ "ok": true })).into_response()
        }
        None => (StatusCode::NOT_FOUND, "no such running session").into_response(),
    }
}

pub(crate) async fn progress_handler(
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::Json;
    use tokio::sync::Mutex;

    use lilyco_core::registry::{RegisteredCommand, Registry};
    use lilyco_core::schema::CommandSchema;

    use super::*;
    use crate::state::fixture::{registry_state, test_state};

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
}
