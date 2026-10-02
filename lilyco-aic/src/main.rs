//! laic — Alice in Cradle 全格式域：
//! 姿势提取 / 帧图层结构数据提取 / 跨表姿势搜索 / 贴图导出 / 立绘合成 / 把改过的表写回去。
//!
//! **「一域一二进制 × 四端」**：同一份 `Registry`，CLI 生成子命令、TUI 生成选择页、
//! Web 用 `?cmd=` 切换、MCP 一次 `tools/list` 全返回。
//!
//! ```bash
//! laic poses D:/gal/.../StreamingAssets --pose gun --full   # CLI：全结构导出
//! laic frame .../Enemies/honeycomb.pxls.dat --pose gun      # 抄小兵 gun 的图层变换
//! laic find-pose .../StreamingAssets --name gun             # 谁的表里有 gun？
//! laic tex .../PxlNoel --out tmp/tex                        # Texture2D 全解码（BC7/ASTC 也吃）
//! laic sprites .../PxlNoel/noel.pxls.dat --out tmp/sp       # 按 UV 裁 sprite（按层名命名）
//! laic render .../PxlNoel/noel.pxls.dat --pose 'big*' --out tmp/r --anim --sheet
//! laic repack .../PxlNoel/noel.pxls.dat --out tmp/out --rename old=new --apply
//! laic --gui     # Web 控制台
//! laic --tui     # TUI 命令选择页
//! laic --mcp     # MCP：agent 直接调
//! laic --schema  # 注册表清单
//! ```
//!
//! 安全分级：
//! - `poses` / `frame` / `find-pose` / `tex` / `mpcc` / `sprites` / `render` = **T0 只读**
//! - `repack` = **T1 需确认**，默认 dry-run；自动化面（MCP）默认拒绝，必须人类在环
//!
//! 解析器规格逆向自 PixelLiner 反编译源码（见 pxlslib.rs 头注释）。

mod atlas;
mod findpose;
mod frame;
mod mpcc;
mod poses;
mod pxlslib;
mod render;
mod repack;
mod serialized;
mod sprites;
mod tex;
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
        RegisteredCommand::from_app::<tex::Tex>(),
        RegisteredCommand::from_app::<mpcc::Mpcc>(),
        RegisteredCommand::from_app::<sprites::Sprites>(),
        RegisteredCommand::from_app::<render::Render>(),
        RegisteredCommand::from_app::<repack::Repack>(),
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
    lilyco::run_registry_with("laic", reg, backend);
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
        let mut want = vec![
            "poses",
            "frame",
            "find-pose",
            "tex",
            "mpcc",
            "sprites",
            "render",
            "repack",
        ];
        want.sort_unstable();
        assert_eq!(got, want.into_iter().map(str::to_string).collect::<Vec<_>>());
    }

    /// 安全分级：只有 repack 高于 T0（写回），其余全部只读
    #[test]
    fn only_repack_is_elevated() {
        let reg = build_registry();
        for c in reg.visible() {
            if c.schema.name == "repack" {
                assert_eq!(
                    c.schema.safety,
                    SafetyTier::Confirm,
                    "repack 是写回，必须 T1（默认 dry-run + 自动化面拒绝）"
                );
            } else {
                assert_eq!(
                    c.schema.safety,
                    SafetyTier::ReadOnly,
                    "`{}` 分级变了：本域只读命令必须是 T0",
                    c.schema.name
                );
            }
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

    /// 写回命令必须被自动化面拦下（MCP = DenyElevated），而 CLI 面允许走到确认环节
    #[test]
    fn repack_is_denied_on_mcp_but_reachable_on_cli() {
        use lilyco::__core::safety::{GateDecision, GateRequest};
        let args = serde_json::json!({ "root": ".", "out": "o", "apply": true });
        let mut tier = None;
        for (policy, surface) in [
            (policy_for(lilyco::Backend::Mcp), "mcp"),
            (policy_for(lilyco::Backend::Cli), "cli"),
        ] {
            let mut reg = Registry::new().with_policy(policy);
            reg.register(RegisteredCommand::from_app::<repack::Repack>()).unwrap();
            let c = reg.get("repack").unwrap();
            let t = c.schema.safety;
            assert_eq!(t, SafetyTier::Confirm, "repack 必须 T1");
            tier = Some(t);
            let decision = reg.policy().check(GateRequest {
                command: "repack",
                tier: t,
                args: &args,
            });
            if surface == "mcp" {
                assert_ne!(decision, GateDecision::Allow, "MCP 自动化面不能放行写回");
            } else {
                assert_eq!(decision, GateDecision::Allow, "CLI 面应放行到确认环节（dry-run 兜底）");
            }
        }
        assert!(tier.is_some());
    }
}
