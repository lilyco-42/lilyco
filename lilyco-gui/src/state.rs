//! 服务器共享状态：一次启动内的 schema / 注册表 / 会话队列 / 取消标志 / 令牌。
//!
//! 只有 `AppState` 的字段可见性需要跨模块，故整体 `pub(crate)`：路由器（lib）、
//! 渲染（render）、执行（run）、安全（security）读的是同一份，没有第二处真相。

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::Mutex;

use lilyco_core::registry::Registry;
use lilyco_core::schema::CommandSchema;

use crate::run::RunnerFn;

pub(crate) struct AppState {
    pub(crate) schema: Arc<CommandSchema>,
    /// 多命令模式（`serve_registry`）：整张注册表
    pub(crate) registry: Option<Arc<Registry>>,
    pub(crate) sessions: Mutex<HashMap<String, tokio::sync::mpsc::Receiver<serde_json::Value>>>,
    /// sid → 取消标志（registry 模式 /cancel 端点用；终态后移除）
    pub(crate) cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub(crate) runner: RunnerFn,
    pub(crate) token: String,
}

/// 测试夹具：各模块的测试共用同一批构造，断言本身留在测试里。
#[cfg(test)]
pub(crate) mod fixture {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum::response::Response;
    use tokio::sync::Mutex;

    use lilyco_core::registry::{Handler, RegisteredCommand, Registry};
    use lilyco_core::safety::SafetyTier;
    use lilyco_core::schema::{ArgKind, ArgSchema, CommandSchema};

    use super::AppState;
    use crate::run::RunnerFn;

    pub(crate) fn schema_of(name: &str, about: &str, args: Vec<ArgSchema>) -> CommandSchema {
        CommandSchema {
            name: name.into(),
            about: about.into(),
            args,
            subcommands: vec![],
            safety: SafetyTier::ReadOnly,
        }
    }

    pub(crate) fn arg(
        name: &str,
        about: &str,
        kind: ArgKind,
        required: bool,
        default: Option<serde_json::Value>,
    ) -> ArgSchema {
        ArgSchema {
            name: name.into(),
            about: about.into(),
            kind,
            required,
            default,
        }
    }

    /// 单命令模式的 state（令牌固定，测试不校验它）
    pub(crate) fn state_with(schema: CommandSchema) -> Arc<AppState> {
        let runner: RunnerFn = Arc::new(|_args, _tx| Box::pin(async {}));
        Arc::new(AppState {
            schema: Arc::new(schema),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner,
            token: "test-token".into(),
        })
    }

    /// 带一个必填 Text + 一个带区间的 Number 的 demo 命令
    pub(crate) fn test_state() -> Arc<AppState> {
        state_with(schema_of(
            "demo",
            "demo",
            vec![
                arg(
                    "quality",
                    "质量",
                    ArgKind::Number {
                        min: Some(0.0),
                        max: Some(51.0),
                    },
                    false,
                    Some(serde_json::json!(23)),
                ),
                arg("input", "输入", ArgKind::Text, true, None),
            ],
        ))
    }

    /// 多命令模式：ping 可见，secret 隐藏（`get` 命中但不可导航）
    pub(crate) fn two_command_registry() -> Registry {
        let mut reg = Registry::new();
        let ping_handler: Handler = Arc::new(|_ctx, _args| Ok(serde_json::json!({"ok": true})));
        reg.register(
            RegisteredCommand::new("ping", schema_of("ping", "问好", vec![]))
                .with_handler(ping_handler),
        )
        .unwrap();
        reg.register(
            RegisteredCommand::new("secret", schema_of("secret", "隐藏", vec![])).hidden(true),
        )
        .unwrap();
        reg
    }

    /// 多命令模式的 state：默认渲染的命令取注册表里第一个可见命令
    pub(crate) fn registry_state_with(reg: Registry) -> Arc<AppState> {
        let first = reg
            .visible()
            .next()
            .expect("fixture: registry has no visible commands")
            .schema
            .clone();
        let runner: RunnerFn = Arc::new(|_, _| Box::pin(async {}));
        Arc::new(AppState {
            schema: Arc::new(first),
            registry: Some(Arc::new(reg)),
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner,
            token: "test-token".into(),
        })
    }

    pub(crate) fn registry_state() -> Arc<AppState> {
        registry_state_with(two_command_registry())
    }

    pub(crate) async fn body_of(resp: Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }
}
