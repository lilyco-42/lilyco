//! @BIN@ — <这个域回答什么问题>。
//!
//! **「一域一二进制 × 四端」的骨架**：同一份 `Registry`，CLI 生成子命令、TUI 生成选择页、
//! Web 用 `?cmd=` 切换、MCP 一次 `tools/list` 全返回。命令侧只写业务，不认识任何端。
//!
//! ```bash
//! @BIN@ show --path ./Cargo.toml --json
//! @BIN@ --gui        # Web 控制台（?cmd= 切换命令）
//! @BIN@ --tui        # TUI 命令选择页
//! @BIN@ --mcp        # MCP 服务器（agent 直接调）
//! @BIN@ --schema     # 打印整张注册表清单
//! ```
//!
//! 落地后要改的地方见 docs/INTEGRATION.md §2（12 步清单，第 11 步的 CI 枚举最容易漏）。

mod show;

use lilyco::prelude::*;
use std::sync::Arc;

/// 构建整个域的注册表（使用给定安全策略）
///
/// 策略必须在 `register` 之前就位 —— `Registry` 的门是在注册那一刻把 handler 包住的，
/// 事后 `into_registry_with_policy` 只能替换注册表项，解不开已经包上的旧门。
pub fn build_registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);
    let cmds: Vec<RegisteredCommand> = vec![
        RegisteredCommand::from_app::<show::Show>(),
        // 每条新命令在这里加一行；命令本体是 src/<命令>.rs 里的 struct
    ];
    for c in cmds {
        let name = c.name.clone();
        reg.register(c)
            .unwrap_or_else(|e| panic!("注册命令 `{name}` 失败: {e}"));
    }
    reg
}

/// 按调用面选择安全策略：agent 那一面不接受需要确认的命令（本域命令若是 T0，两边其实都放行）
pub fn policy_for(backend: lilyco::Backend) -> Arc<dyn SafetyPolicy> {
    match backend {
        lilyco::Backend::Mcp => Arc::new(DenyElevated),
        #[allow(unreachable_patterns)]
        _ => Arc::new(Interactive),
    }
}

fn main() {
    let backend = lilyco::detect_registry_backend();
    let reg = build_registry_with_policy(policy_for(backend));
    lilyco::run_registry_with("@BIN@", reg, backend);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::SafetyTier;

    fn names() -> Vec<String> {
        build_registry_with_policy(Arc::new(Interactive))
            .visible()
            .map(|c| c.schema.name.clone())
            .collect()
    }

    /// 命令表是四端共同的契约面：改名 = 破坏 agent 侧的调用，所以要断言
    #[test]
    fn registry_lists_the_commands_this_domain_answers() {
        assert_eq!(names(), vec!["show".to_string()]);
    }

    /// 每条命令的分级都要显式对上：模板给的是 T0 只读，加写盘/删文件的命令时必须改它，
    /// 并且同步 about 文案与该域文档（改分级 = 改 agent 侧能不能自动调）
    #[test]
    fn every_command_carries_the_declared_safety_tier() {
        let reg = build_registry_with_policy(Arc::new(Interactive));
        for c in reg.visible() {
            assert_eq!(
                c.schema.safety,
                SafetyTier::ReadOnly,
                "`{}` 的分级变了：about 与 docs 里的只读声明也要改",
                c.schema.name
            );
        }
    }
}
