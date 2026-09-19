//! lfiles — 文件整理域：查找 / 批量重命名 / 去重 / 目录统计。
//!
//! **「一域一二进制 × 四端」的样板**：同一个 `Registry`，四个后端各取一份渲染。
//!
//! ```bash
//! lfiles find --root D:/photos --pattern '*.jpg' --json      # CLI
//! lfiles --gui                                                # Web 控制台（?cmd= 下拉切换）
//! lfiles --tui                                                # TUI 命令选择页
//! lfiles --mcp                                                # MCP：tools/list 一次返回全部命令
//! lfiles --schema                                             # 打印整张注册表清单
//! ```
//!
//! 安全分级：
//! - `find` / `dedup` / `stats` = **T0 只读**，自动化面（MCP）可放行
//! - `rename` = **T1 需确认**，默认 dry-run；自动化面默认拒绝，必须人类在环
//!
//! 域内共享的遍历/匹配/体积工具在 [`util`]。

mod dedup;
mod find;
mod rename;
mod stats;
mod util;

use lilyco::prelude::*;
use lilyco_core::safety::{DenyElevated, Interactive, SafetyPolicy};
use std::sync::Arc;

/// 构建整个「文件整理」域的注册表（使用给定安全策略）
///
/// **策略必须在 `register` 之前就位** —— `Registry` 的门是在注册那一刻
/// 把 handler 包住的，事后换策略只能叠门（旧门先拒），所以这里把它做成参数。
///
/// 四端共用这一份：CLI 生成子命令、TUI 生成选择页、Web 用 `?cmd=` 切换、
/// MCP 一次 `tools/list` 全返回 —— 加命令只在这里加一行。
pub fn build_registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);
    let cmds: Vec<RegisteredCommand> = vec![
        RegisteredCommand::from_app::<find::Find>(),
        RegisteredCommand::from_app::<rename::Rename>(),
        RegisteredCommand::from_app::<dedup::Dedup>(),
        RegisteredCommand::from_app::<stats::Stats>(),
    ];
    for c in cmds {
        let name = c.name.clone();
        reg.register(c).unwrap_or_else(|e| panic!("注册命令 `{name}` 失败: {e}"));
    }
    reg
}

/// 按后端选择安全策略（与 facade 的默认约定一致，这里显式化便于单测）
pub fn policy_for(backend: lilyco::Backend) -> Arc<dyn SafetyPolicy> {
    match backend {
        lilyco::Backend::Mcp => Arc::new(DenyElevated),
        #[allow(unreachable_patterns)]
        _ => Arc::new(Interactive),
    }
}

fn main() {
    // 先探后端，再按该调用面选策略注册 —— 顺序不能反（见 build_registry_with_policy 注释）
    let backend = lilyco::detect_registry_backend();
    let reg = build_registry_with_policy(policy_for(backend));
    lilyco::run_registry_with("lfiles", reg, backend);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用：宽松策略下的注册表（等价于 CLI/TUI/Web 面）
    fn build_registry() -> Registry {
        build_registry_with_policy(Arc::new(Interactive))
    }

    /// 注册表装配正确：四条命令、名字与安全分级都对
    #[test]
    fn registry_has_expected_commands() {
        let reg = build_registry();
        let names: Vec<String> = reg.iter().map(|c| c.name.clone()).collect();
        for want in ["find", "rename", "dedup", "stats"] {
            assert!(names.contains(&want.to_string()), "missing {want}: {names:?}");
        }
        assert_eq!(reg.iter().count(), 4, "{names:?}");
        assert!(reg.visible().count() == 4, "all MVP commands are visible");
    }

    /// 安全分级：只有 rename 高于 T0，其余都必须是 T0
    #[test]
    fn only_rename_is_elevated() {
        use lilyco_core::safety::SafetyTier;
        let reg = build_registry();
        for c in reg.iter() {
            let tier = c.schema.safety;
            if c.name == "rename" {
                assert_eq!(tier, SafetyTier::Confirm, "rename 必须 T1");
            } else {
                assert_eq!(tier, SafetyTier::ReadOnly, "{} 必须 T0", c.name);
            }
        }
    }

    /// 每条命令都带 handler（不是只声明 schema 的空壳）
    #[test]
    fn every_command_has_handler() {
        let reg = build_registry();
        for c in reg.iter() {
            assert!(c.handler.is_some(), "{} 缺 handler", c.name);
        }
    }

    /// 四端同源的根基：每条命令都能导出 OpenAI 工具定义（= MCP tools/list 形状）
    #[test]
    fn every_command_exports_openai_tool() {
        let reg = build_registry();
        for c in reg.iter() {
            let t = c.schema.to_openai_tool();
            assert_eq!(t["type"], "function");
            assert_eq!(t["function"]["name"], c.name.as_str());
            assert!(
                t["function"]["parameters"]["properties"].is_object(),
                "{} 的参数 schema 不合法",
                c.name
            );
            assert!(
                !t["function"]["description"].as_str().unwrap().is_empty(),
                "{} 缺 description（模型靠它选工具）",
                c.name
            );
        }
    }

    /// 整张注册表 JSON 清单可序列化（`--schema` 的输出）
    #[test]
    fn registry_json_manifest_is_serializable() {
        let reg = build_registry();
        let json = reg.to_json();
        let s = serde_json::to_string(&json).unwrap();
        assert!(s.contains("\"find\"") && s.contains("\"rename\""), "{s}");
    }

    /// 缺参必须被 validate_args 拦下（MCP/Web 直传 JSON 的唯一防线）
    #[test]
    fn missing_required_arg_is_rejected_by_schema() {
        let reg = build_registry();
        let find = reg.get("find").expect("find registered");
        let err = find
            .schema
            .validate_args(&serde_json::json!({}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("root"), "{err}");
    }

    /// T0 的 find 在交互策略下必须放行
    #[test]
    fn find_is_allowed_under_default_policy() {
        use lilyco_core::safety::{GateDecision, GateRequest, SafetyPolicy};
        let reg = build_registry();
        let find = reg.get("find").expect("find registered");
        let decision = reg.policy().check(GateRequest {
            command: "find",
            tier: find.schema.safety,
            args: &serde_json::json!({ "root": "." }),
        });
        assert_eq!(decision, GateDecision::Allow);
    }

    /// **策略分层**：交互面（CLI/TUI/Web）必须放行 T1 的 rename
    ///
    /// 这是刚修过的缺陷的回归测试：早期 façade 从没注入过策略，
    /// 所有人都吃 `DenyElevated` 默认值 → 人在 CLI 上手敲 `rename` 也被拒。
    #[test]
    fn interactive_surface_allows_rename() {
        use lilyco_core::safety::{GateDecision, GateRequest, SafetyPolicy};
        let reg = build_registry_with_policy(Arc::new(Interactive));
        let rename = reg.get("rename").expect("rename registered");
        assert_eq!(
            reg.policy().check(GateRequest {
                command: "rename",
                tier: rename.schema.safety,
                args: &serde_json::json!({ "root": "." }),
            }),
            GateDecision::Allow,
            "人类在环的 CLI 面必须能执行 T1 rename"
        );
    }

    /// **策略分层**：MCP 自动化面必须拒绝 T1 的 rename，但仍放行 T0
    #[test]
    fn mcp_surface_denies_rename_but_allows_readonly() {
        use lilyco_core::safety::{GateDecision, GateRequest, SafetyPolicy};
        let reg = build_registry_with_policy(policy_for(lilyco::Backend::Mcp));
        let rename = reg.get("rename").unwrap();
        let denied = reg.policy().check(GateRequest {
            command: "rename",
            tier: rename.schema.safety,
            args: &serde_json::json!({ "root": "." }),
        });
        assert!(matches!(denied, GateDecision::Deny(_)), "{denied:?}");

        for name in ["find", "dedup", "stats"] {
            let c = reg.get(name).unwrap();
            assert_eq!(
                reg.policy().check(GateRequest {
                    command: name,
                    tier: c.schema.safety,
                    args: &serde_json::json!({ "root": "." }),
                }),
                GateDecision::Allow,
                "{name} 是 T0，自动化面必须放行"
            );
        }
    }

    /// `policy_for` 的分层映射：只有 MCP 用严格策略
    #[test]
    fn policy_for_maps_backends_correctly() {
        use lilyco::Backend;
        // 用 Debug 名做粗判（trait object 无法直接比较）
        let name = |b: Backend| match b {
            Backend::Mcp => "DenyElevated",
            _ => "Interactive",
        };
        assert_eq!(name(Backend::Mcp), "DenyElevated");
        assert_eq!(name(Backend::Cli), "Interactive");
        // 真实验证：MCP 策拒绝 T1、交互策放行 T1
        use lilyco_core::safety::{GateDecision, GateRequest, SafetyTier};
        let args = serde_json::json!({});
        let mcp = policy_for(Backend::Mcp);
        assert!(matches!(
            mcp.check(GateRequest {
                command: "rename",
                tier: SafetyTier::Confirm,
                args: &args
            }),
            GateDecision::Deny(_)
        ));
        let cli = policy_for(Backend::Cli);
        assert_eq!(
            cli.check(GateRequest {
                command: "rename",
                tier: SafetyTier::Confirm,
                args: &args
            }),
            GateDecision::Allow
        );
    }
}
