//! `lilyco` — 元 CLI：lilyco 生态的「cargo 时刻」。
//!
//! cargo 之于 Rust 包，`lilyco-dev` 之于 lilyco 域应用 —— 开发者的整个生命周期
//! 收进一个二进制：
//!
//! | 命令 | 作用 | 物化了哪份文档流程 |
//! |---|---|---|
//! | `lilyco new <name>` | 从内嵌的 `scripts/domain-template/` 脚手架出 `lilyco-<name>` crate | DOMAIN-GUIDE「30 行最小应用」 |
//! | `lilyco build` | 薄包装 `cargo build`（含 Android headless profile） | INTEGRATION §2 构建步骤 |
//! | `lilyco run` | `cargo run` + 表面选择（`--tui` / `--gui` / `--mcp` 透传给 app） | 四端一行启动 |
//! | `lilyco doc` | 跑 app 的 `--schema`，产出能力表 `CAPABILITIES.md` + `capabilities.json` | 能力表 = agent 侧的契约面 |
//!
//! 元 CLI 自己就是框架消费者（dogfooding）：四个命令全是 `#[derive(App)]`，
//! 走 facade 的 `Registry` + 按调用面注入安全策略 —— 所以 `lilyco --schema`
//! 能打印自身能力表，`lilyco --mcp` 就是可被 agent 发现的 MCP 服务器。
//!
//! **为什么四个命令全是 T1**：new 写文件、build/run 外呼 cargo、doc 外呼并写
//! 文件 —— 按生态铁律「删除/覆盖/外呼 ≥ T1」如实声明。MCP 面上（DenyElevated）
//! 它们全部被拦：agent 可以 *看见并提案*（`tools/list` 可见 schema），
//! 执行由人在 CLI 面确认。这正是安全模型的意图，不是缺陷。
//!
//! ```text
//! lilyco new todo                 # → ./lilyco-todo/（crate lilyco-todo，bin ltodo）
//! cd lilyco-todo && lilyco build  # → cargo build
//! lilyco run --surface tui        # → cargo run -- --tui
//! lilyco doc                      # → CAPABILITIES.md + capabilities.json
//! ```

mod cargo_ops;
mod docgen;
mod scaffold;

use lilyco::prelude::*;
use std::sync::Arc;

/// 构建元 CLI 的注册表（策略必须在 register 前就位 —— 门在注册那一刻包住 handler）
pub fn build_registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);
    let cmds: Vec<RegisteredCommand> = vec![
        RegisteredCommand::from_app::<scaffold::New>(),
        RegisteredCommand::from_app::<cargo_ops::Build>(),
        RegisteredCommand::from_app::<cargo_ops::Run>(),
        RegisteredCommand::from_app::<docgen::Doc>(),
    ];
    for c in cmds {
        let name = c.name.clone();
        reg.register(c)
            .unwrap_or_else(|e| panic!("注册命令 `{name}` 失败: {e}"));
    }
    reg
}

/// 按调用面选择安全策略（与域模板 main.rs 同款）：agent 面拒 T1+，人面放行 T0+T1
fn policy_for(backend: lilyco::Backend) -> Arc<dyn SafetyPolicy> {
    match backend {
        lilyco::Backend::Mcp => Arc::new(DenyElevated),
        #[allow(unreachable_patterns)]
        _ => Arc::new(Interactive),
    }
}

fn main() {
    let backend = lilyco::detect_registry_backend();
    let reg = build_registry_with_policy(policy_for(backend));
    lilyco::run_registry_with("lilyco", reg, backend);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::SafetyTier;

    /// 命令表是四端共同的契约面：改名 = 破坏 agent 侧调用，所以要断言。
    /// `Registry.commands` 是 HashMap，`visible()` 顺序不定 → 排序后比集合。
    #[test]
    fn registry_lists_exactly_the_four_meta_commands() {
        let mut names: Vec<String> = build_registry_with_policy(Arc::new(Interactive))
            .visible()
            .map(|c| c.name.clone())
            .collect();
        names.sort();
        assert_eq!(names, vec!["build", "doc", "new", "run"]);
    }

    /// 全部 T1 如实声明：new 写文件、build/run 外呼、doc 外呼+写文件。
    /// 若有人把它改成 T0，MCP 面就会无人确认地写盘/外呼 —— 必须改回来。
    #[test]
    fn every_meta_command_declares_confirm() {
        for c in build_registry_with_policy(Arc::new(Interactive)).visible() {
            assert_eq!(
                c.schema.safety,
                SafetyTier::Confirm,
                "`{}` 的分级变了：它有副作用（写盘/外呼），T1 是底线",
                c.name
            );
        }
    }

    /// about 是 agent 侧唯一的说明文本：每条命令必须写清副作用与返回形状
    #[test]
    fn every_meta_command_documents_side_effects_and_result() {
        for c in build_registry_with_policy(Arc::new(Interactive)).visible() {
            assert!(
                c.schema.about.len() > 40,
                "`{}` 的 about 太短（{} chars）：要写清副作用与返回形状",
                c.name,
                c.schema.about.len()
            );
        }
    }
}
