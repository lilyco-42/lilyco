//! lpxls — PixelLiner（Alice in Cradle）角色资源域：
//! 姿势提取 / 帧图层结构数据提取 / 跨表姿势搜索。
//!
//! **「一域一二进制 × 四端」**：同一份 `Registry`，CLI 生成子命令、TUI 生成选择页、
//! Web 用 `?cmd=` 切换、MCP 一次 `tools/list` 全返回。
//!
//! ```bash
//! lpxls poses D:/gal/.../StreamingAssets --pose gun --full   # CLI：全结构导出
//! lpxls frame .../Enemies/honeycomb.pxls.dat --pose gun      # 抄小兵 gun 的图层变换
//! lpxls find-pose .../StreamingAssets --name gun             # 谁的表里有 gun？
//! lpxls --gui     # Web 控制台
//! lpxls --tui     # TUI 命令选择页
//! lpxls --mcp     # MCP：agent 直接调
//! lpxls --schema  # 注册表清单
//! ```
//!
//! 全部命令 T0 只读；解析器规格逆向自 PixelLiner 反编译源码（见 pxls.rs 头注释）。

mod findpose;
mod frame;
mod poses;
mod pxls;
mod unityfs;
mod util;

use lilyco::__core::safety::{DenyElevated, Interactive, SafetyPolicy};
use lilyco::prelude::*;
use std::sync::Arc;

/// 构建整个 pxls 域的注册表（策略必须在 register 之前就位）
pub fn build_registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);
    let cmds: Vec<RegisteredCommand> = vec![
        RegisteredCommand::from_app::<poses::Poses>(),
        RegisteredCommand::from_app::<frame::Frame>(),
        RegisteredCommand::from_app::<findpose::FindPose>(),
    ];
    for c in cmds {
        let name = c.name.clone();
        reg.register(c)
            .unwrap_or_else(|e| panic!("注册命令 `{name}` 失败: {e}"));
    }
    reg
}

/// 按调用面选择安全策略（MCP 自动化面 = DenyElevated）
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
    lilyco::run_registry_with("lpxls", reg, backend);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::__core::safety::SafetyTier;

    fn build_registry() -> Registry {
        build_registry_with_policy(Arc::new(Interactive))
    }

    fn names() -> Vec<String> {
        build_registry()
            .visible()
            .map(|c| c.schema.name.clone())
            .collect()
    }

    /// 命令表是四端共同契约面：改名 = 破坏 agent 调用
    #[test]
    fn registry_lists_the_commands_this_domain_answers() {
        let mut got = names();
        got.sort();
        let mut want = vec!["poses", "frame", "find-pose"];
        want.sort_unstable();
        assert_eq!(got, want.into_iter().map(str::to_string).collect::<Vec<_>>());
    }

    /// 全域只读：每条命令都必须 T0
    #[test]
    fn every_command_is_read_only() {
        let reg = build_registry();
        for c in reg.visible() {
            assert_eq!(
                c.schema.safety,
                SafetyTier::ReadOnly,
                "`{}` 分级变了：本域只做提取，写回功能加进来时要改这里",
                c.schema.name
            );
        }
    }

    /// 每条命令都带 handler 且能导出 OpenAI 工具定义（MCP tools/list 形状）
    #[test]
    fn every_command_exports_openai_tool() {
        let reg = build_registry();
        for c in reg.visible() {
            assert!(c.handler.is_some(), "{} 缺 handler", c.name);
            let t = c.schema.to_openai_tool();
            assert_eq!(t["function"]["name"], c.name.as_str());
            assert!(!t["function"]["parameters"]["properties"].is_null());
        }
    }

    /// 缺参必须被 validate_args 拦下（MCP/Web 直传 JSON 的唯一防线）
    #[test]
    fn missing_required_args_are_rejected() {
        let reg = build_registry();
        for name in ["poses", "frame", "find-pose"] {
            let err = reg
                .get(name)
                .expect(name)
                .schema
                .validate_args(&serde_json::json!({}))
                .unwrap_err()
                .to_string();
            assert!(err.contains("root"), "{name}: {err}");
        }
    }

    /// MCP 面放行 T0、CLI 面同样放行（策略分层回归）
    #[test]
    fn readonly_allowed_on_both_surfaces() {
        use lilyco::__core::safety::{GateDecision, GateRequest};
        let args = serde_json::json!({ "root": ".", "pose": "gun" });
        for (policy, surface) in [
            (policy_for(lilyco::Backend::Mcp), "mcp"),
            (policy_for(lilyco::Backend::Cli), "cli"),
        ] {
            let reg = Registry::new().with_policy(policy);
            let reg = {
                let mut r = reg;
                for c in [
                    RegisteredCommand::from_app::<frame::Frame>(),
                    RegisteredCommand::from_app::<findpose::FindPose>(),
                ] {
                    r.register(c).unwrap();
                }
                r
            };
            for name in ["frame", "find-pose"] {
                let c = reg.get(name).unwrap();
                assert_eq!(
                    reg.policy().check(GateRequest {
                        command: name,
                        tier: c.schema.safety,
                        args: &args
                    }),
                    GateDecision::Allow,
                    "{surface}/{name} 是 T0 必须放行"
                );
            }
        }
    }
}
