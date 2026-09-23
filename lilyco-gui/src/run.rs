//! 执行侧：`POST /run` 分发、`GET /progress/{sid}` SSE 流、`POST /cancel/{sid}`。
//!
//! 能取消的执行路只有一条：`Registry` 里的 handler 经 `run_progress` 交给
//! `core::executor`（与 CLI / TUI / MCP 同一宿主），顺手把取消句柄登记进
//! `AppState::cancels`。自定义 `RunnerFn`（`serve`）自己 spawn 任务，GUI 手里
//! 没有句柄，页面因此不画「取消」按钮（`serve_app` 走的就是单命令注册表）。

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

/// 执行 handler 并把进度事件流式转发到 SSE 通道；同时登记取消句柄，终态后清理。
///
/// `cancels` 这一步是 /cancel 唯一能找到句柄的地方 —— 所以「可取消」这件事
/// 由它决定，页面据此决定画不画「取消」（`render::index` 读 `state.registry`）。
///
/// 注意 `for event in task.rx` 是**阻塞**收事件（crossbeam 收件箱，不是 async）：
/// 它占住一个 worker 线程，所以这条路只能在多线程 runtime 上跑（`axum::serve` 的
/// 运行时满足）。测试里要用 `flavor = "multi_thread"`，单线程 runtime 会直接僵住。
async fn run_progress(
    state: &AppState,
    sid: &str,
    handler: Handler,
    args: serde_json::Value,
    gui_tx: tokio::sync::mpsc::Sender<serde_json::Value>,
) {
    let task = executor::spawn(handler, args);
    state
        .cancels
        .lock()
        .await
        .insert(sid.to_string(), Arc::clone(&task.cancel));
    for event in task.rx {
        let json = serde_json::to_value(&event).unwrap();
        if gui_tx.send(json).await.is_err() {
            break;
        }
        if matches!(event, Progress::Done { .. } | Progress::Error { .. }) {
            break;
        }
    }
    state.cancels.lock().await.remove(sid);
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
            run_progress(&state2, &sid2, handler, args_value, tx).await;
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
    use crate::state::fixture::{
        body_of, registry_state, registry_state_with, schema_of, test_state,
    };

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

    /// 「取消」的整条链：/run 登记句柄 → /cancel 找得到 → 终态后清掉。
    /// 这条之所以要单独立着：`serve_app` 曾经走自定义 RunnerFn 那条路，句柄从来没进过
    /// `cancels`，于是页面上每颗「取消」都只回一句 404 —— 而所有其它测试全是绿的。
    ///
    /// `multi_thread` 不是提速：`run_progress` 收事件是阻塞的，单线程 runtime 下它会
    /// 饿死本测试的 `sleep`，第一次跑就把 `cargo test` 挂在那里（2026-09-23 实测 2 分钟不动）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn registry_run_registers_a_cancel_handle_and_clears_it() {
        let mut reg = Registry::new();
        let slow: Handler = Arc::new(|ctx, _args| {
            // 一直跑到被取消：命令侧的 ctx.is_cancelled() 是唯一的出口
            while !ctx.is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(serde_json::json!(null))
        });
        reg.register(
            RegisteredCommand::new("slow", schema_of("slow", "慢命令", vec![])).with_handler(slow),
        )
        .unwrap();
        let state = registry_state_with(reg);

        let resp = run_handler(
            State(state.clone()),
            Json(RunRequest {
                args: HashMap::new(),
                cmd: Some("slow".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let sid = serde_json::from_str::<serde_json::Value>(&body_of(resp).await).unwrap()
            ["session_id"]
            .as_str()
            .unwrap()
            .to_string();

        let mut found = false;
        for _ in 0..200 {
            if state.cancels.lock().await.contains_key(&sid) {
                found = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(found, "/run 返回了 {sid}，却没登记它的取消句柄");

        let resp = cancel_handler(State(state.clone()), Path(sid.clone())).await;
        assert_eq!(resp.status(), StatusCode::OK, "/cancel 找不到正在跑的会话");

        for _ in 0..200 {
            if !state.cancels.lock().await.contains_key(&sid) {
                return; // 终态后清理完成
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("会话已终态，取消句柄却还留在表里");
    }

    /// 反面：自定义 RunnerFn 那条路确实没有句柄 —— 页面据此不画「取消」（见 render 的同名测试）
    #[tokio::test]
    async fn runner_fn_sessions_cannot_be_cancelled() {
        let state = test_state();
        let mut args = HashMap::new();
        args.insert("input".to_string(), serde_json::json!("a.png"));
        args.insert("quality".to_string(), serde_json::json!(30));
        let resp = run_handler(State(state.clone()), Json(RunRequest { args, cmd: None })).await;
        let sid = serde_json::from_str::<serde_json::Value>(&body_of(resp).await).unwrap()
            ["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        let resp = cancel_handler(State(state), Path(sid)).await;
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "RunnerFn 模式不该假装能取消"
        );
    }
}
