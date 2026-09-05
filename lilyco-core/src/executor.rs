//! 共享执行宿主（高内聚核心：三端共用的"参数 → 执行 → 进度事件"唯一实现）
//!
//! 此前 CLI / GUI / example 各自实现了一遍线程 + channel + 进度消费的宿主循环
//! （三份重复代码）。现在执行语义收敛到这里：
//! - [`spawn`]：后台线程执行 + 流式进度事件（CLI json-stream、GUI SSE 用）
//! - [`execute`]：同步执行到完成，收集全部事件（TUI、MCP 用）
//!
//! 各渲染端只负责把 [`Progress`] 事件渲染成自己的形态，不再关心执行细节。

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use crate::context::{Context, OutputFormat};
use crate::error::AppError;
use crate::progress::Progress;
use crate::registry::Handler;

/// 一个在后台线程运行的任务：取消句柄 + 进度接收端 + 线程句柄
pub struct Task {
    /// 置 true 可请求任务取消（命令侧通过 `ctx.is_cancelled()` 响应）
    pub cancel: Arc<AtomicBool>,
    /// 进度事件流（协议保证以 `Done` 或 `Error` 结尾）
    pub rx: Receiver<Progress>,
    /// 后台线程句柄（join 得到最终结果）
    pub handle: std::thread::JoinHandle<Result<serde_json::Value, AppError>>,
}

/// 派生后台线程执行命令，流式返回进度事件
///
/// 与 [`execute`] 共享同一套终态合成逻辑：无论 handler 是否通过 `ctx.done` /
/// `ctx.error` 上报终态，事件流都保证以 `Done` / `Error` 结尾（协议不变量）。
/// 消费者（CLI / TUI / MCP）不再需要自己兜底合成，也不会因 handler 忘报
/// 终态而永久阻塞在 channel 上。
pub fn spawn(handler: Handler, args: serde_json::Value) -> Task {
    spawn_with(handler, args, None)
}

/// [`spawn`] 的宿主桥变体：给 handler 的 Context 附加反向能力（采样 / roots）
///
/// 供 MCP 服务器执行 tools/call 时注入 [`crate::context::HostBridge`]；
/// CLI / TUI / GUI 传 `None`（等价 [`spawn`]）。
pub fn spawn_with(
    handler: Handler,
    args: serde_json::Value,
    host: Option<Arc<dyn crate::context::HostBridge>>,
) -> Task {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = channel();
    let mut ctx = Context::new(tx.clone(), cancel.clone(), OutputFormat::Human);
    if let Some(host) = host {
        ctx = ctx.with_host(host);
    }

    let handle = std::thread::spawn(move || {
        let result = (handler)(&ctx, &args);

        // 若 handler 未上报终态事件，按返回值合成，维持"恒以 Done/Error 结尾"不变量
        if !ctx.has_terminal() {
            match &result {
                Ok(v) => {
                    let _ = tx.send(Progress::Done {
                        result: v.clone(),
                        duration_ms: 0,
                    });
                }
                Err(e) => {
                    let _ = tx.send(Progress::Error {
                        code: 1,
                        message: e.to_string(),
                        kind: None,
                    });
                }
            }
        }

        result
    });

    Task { cancel, rx, handle }
}

/// 同步执行结果
pub struct RunOutcome {
    /// 全部进度事件（协议保证以 `Done` 或 `Error` 结尾）
    pub events: Vec<Progress>,
    /// 最终结果
    pub result: Result<serde_json::Value, AppError>,
}

impl RunOutcome {
    /// 取最终结果
    pub fn into_result(self) -> Result<serde_json::Value, AppError> {
        self.result
    }

    /// 最后一个进度事件（恒为 `Done` 或 `Error`）
    pub fn last_event(&self) -> Option<&Progress> {
        self.events.last()
    }
}

/// 同步执行到完成，收集全部进度事件
///
/// 协议保证：即使 handler 忘记通过 ctx 上报 Done/Error，
/// 返回值也会被合成终态事件，事件流永远以 `Done`/`Error` 结尾。
pub fn execute(handler: Handler, args: serde_json::Value) -> RunOutcome {
    execute_with(handler, args, None)
}

/// [`execute`] 的宿主桥变体（同 [`spawn_with`] 与 [`execute`] 的关系）
pub fn execute_with(
    handler: Handler,
    args: serde_json::Value,
    host: Option<Arc<dyn crate::context::HostBridge>>,
) -> RunOutcome {
    let task = spawn_with(handler, args, host);
    let mut events = Vec::new();
    for ev in task.rx {
        let done = matches!(ev, Progress::Done { .. } | Progress::Error { .. });
        events.push(ev);
        if done {
            break;
        }
    }
    let result = match task.handle.join() {
        Ok(r) => r,
        Err(panic) => Err(AppError::Runtime(format!("handler panicked: {panic:?}"))),
    };

    // 合成终态事件，维持协议不变量
    let has_terminal = events
        .last()
        .map(|e| matches!(e, Progress::Done { .. } | Progress::Error { .. }))
        .unwrap_or(false);
    if !has_terminal {
        match &result {
            Ok(v) => events.push(Progress::Done {
                result: v.clone(),
                duration_ms: 0,
            }),
            Err(e) => events.push(Progress::Error {
                code: 1,
                message: e.to_string(),
                kind: None,
            }),
        }
    }

    RunOutcome { events, result }
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::{LogLevel, Progress};
    use std::sync::Arc;

    fn ok_handler() -> Handler {
        Arc::new(|ctx, args| {
            ctx.tick(1, Some(2), "step a");
            ctx.log(LogLevel::Info, "mid");
            ctx.tick(2, Some(2), "step b");
            let r = serde_json::json!({ "r": args["n"] });
            ctx.done(r.clone(), 5);
            Ok(r)
        })
    }

    fn err_handler() -> Handler {
        Arc::new(|_ctx, _args| Err(AppError::Runtime("boom".into())))
    }

    #[test]
    fn execute_collects_events_and_result() {
        let outcome = execute(ok_handler(), serde_json::json!({ "n": 42 }));
        assert_eq!(outcome.events.len(), 4, "tick + log + tick + done");
        assert!(matches!(outcome.events[3], Progress::Done { .. }));
        assert_eq!(outcome.result.unwrap()["r"], 42);
    }

    #[test]
    fn execute_synthesizes_terminal_event_on_error() {
        let outcome = execute(err_handler(), serde_json::json!({}));
        assert!(outcome.result.is_err());
        // handler 没上报任何事件，execute 应合成 Error 终态
        assert!(matches!(outcome.last_event(), Some(Progress::Error { .. })));
    }

    #[test]
    fn execute_synthesizes_done_when_handler_forgets() {
        let handler: Handler = Arc::new(|_ctx, _args| Ok(serde_json::json!({ "ok": true })));
        let outcome = execute(handler, serde_json::json!({}));
        assert!(matches!(outcome.last_event(), Some(Progress::Done { .. })));
        assert!(outcome.result.is_ok());
    }

    #[test]
    fn execute_handles_panic() {
        let handler: Handler = Arc::new(|_ctx, _args| panic!("deliberate"));
        let outcome = execute(handler, serde_json::json!({}));
        assert!(outcome.result.is_err());
        assert!(outcome.result.unwrap_err().to_string().contains("panicked"));
    }

    #[test]
    fn spawn_streams_events_lazily() {
        let task = spawn(ok_handler(), serde_json::json!({ "n": 1 }));
        let mut seen = 0;
        for ev in task.rx {
            seen += 1;
            assert!(matches!(ev, Progress::Done { .. }) == (seen == 4));
            if seen == 4 {
                break;
            }
        }
        assert_eq!(seen, 4);
        task.handle.join().unwrap().unwrap();
    }

    #[test]
    fn cancel_flag_is_accessible() {
        let task = spawn(ok_handler(), serde_json::json!({ "n": 1 }));
        task.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // 只是验证取消句柄可写（命令侧读取）
        assert!(task.cancel.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn spawn_synthesizes_terminal_when_handler_forgets() {
        // handler 忘了 ctx.done()/ctx.error()，spawn 也应合成终态（协议不变量）
        let handler: Handler = Arc::new(|_ctx, _args| Ok(serde_json::json!({ "ok": true })));
        let task = spawn(handler, serde_json::json!({}));
        let mut events = Vec::new();
        for ev in task.rx {
            let done = matches!(ev, Progress::Done { .. } | Progress::Error { .. });
            events.push(ev);
            if done {
                break;
            }
        }
        assert!(
            matches!(events.last(), Some(Progress::Done { .. })),
            "spawn should synthesize Done, got: {events:?}"
        );
    }

    #[test]
    fn spawn_synthesizes_error_when_handler_returns_err() {
        // handler 返回 Err 但没发 Error 事件 → spawn 合成 Error 终态
        let handler: Handler = Arc::new(|_ctx, _args| Err(AppError::Runtime("boom".into())));
        let task = spawn(handler, serde_json::json!({}));
        let mut events = Vec::new();
        for ev in task.rx {
            let done = matches!(ev, Progress::Done { .. } | Progress::Error { .. });
            events.push(ev);
            if done {
                break;
            }
        }
        assert!(
            matches!(events.last(), Some(Progress::Error { .. })),
            "spawn should synthesize Error, got: {events:?}"
        );
    }

    #[test]
    fn spawn_single_terminal_even_when_handler_emits() {
        // handler 主动 ctx.done → spawn 不应再合成，只有一次终态
        let task = spawn(ok_handler(), serde_json::json!({ "n": 1 }));
        let events: Vec<Progress> = task.rx.iter().collect();
        let terminals = events
            .iter()
            .filter(|e| matches!(e, Progress::Done { .. } | Progress::Error { .. }))
            .count();
        assert_eq!(terminals, 1, "exactly one terminal event, got {events:?}");
    }
}
