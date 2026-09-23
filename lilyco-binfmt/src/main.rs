//! lbin — 二进制与容器文件结构域：识别 / 成员表 / 分区上色 / 节与符号。
//!
//! **「一域一二进制 × 四端」在文件分析这一域的落法**：同一份 `Registry`，
//! CLI 生成子命令、TUI 生成选择页、Web 用 `?cmd=` 切换、MCP 一次 `tools/list` 全返回。
//! 这四条命令全是 **T0 只读**：只把字节读成结构，既不执行、也不解压、更不写回；
//! 「改了这段字节有没有事」这类判断只以「文件结构里有没有谁指着它」的形式给出，
//! 并且把这条界说在明处（见 [`regions`] 模块注释）。
//!
//! ```bash
//! lbin identify --path ./app.dll --json
//! lbin regions --path ./a.out --json | jq '.totals'
//! lbin entries --path ./libstdc++.so.a --limit 20
//! lbin symbols --path ./main.o
//! lbin --gui        # Web 控制台（?cmd= 切换）
//! lbin --tui        # TUI 命令选择页
//! lbin --mcp        # MCP：tools/list 一次返回四条
//! lbin --schema     # 打印整张注册表清单
//! ```

mod entries;
mod identify;
mod read;
mod regions;
mod symbols;

use lilyco::prelude::*;
use std::sync::Arc;

/// 构建整个「二进制与容器结构」域的注册表（使用给定安全策略）
///
/// 策略必须在 `register` 之前就位 —— `Registry` 的门是在注册那一刻把 handler 包住的。
pub fn build_registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);
    let cmds: Vec<RegisteredCommand> = vec![
        RegisteredCommand::from_app::<identify::Identify>(),
        RegisteredCommand::from_app::<entries::Entries>(),
        RegisteredCommand::from_app::<regions::Regions>(),
        RegisteredCommand::from_app::<symbols::Symbols>(),
    ];
    for c in cmds {
        let name = c.name.clone();
        reg.register(c)
            .unwrap_or_else(|e| panic!("注册命令 `{name}` 失败: {e}"));
    }
    reg
}

/// 按后端选择安全策略（四条命令都是 T0，所以两面都放行；这里仍按约定显式区分）
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
    lilyco::run_registry_with("lbin", reg, backend);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::SafetyTier;

    fn build_registry() -> Registry {
        build_registry_with_policy(Arc::new(Interactive))
    }

    /// 四条命令、名字与顺序都对
    #[test]
    fn registry_has_expected_commands() {
        let reg = build_registry();
        let names: Vec<String> = reg.iter().map(|c| c.name.clone()).collect();
        for want in ["identify", "entries", "regions", "symbols"] {
            assert!(
                names.contains(&want.to_string()),
                "missing {want}: {names:?}"
            );
        }
        assert_eq!(reg.iter().count(), 4, "{names:?}");
        assert_eq!(reg.visible().count(), 4, "四条命令都要可见");
    }

    /// 这个域全部只读：出现任何高于 T0 的命令都是越界（它凭什么改文件？）
    #[test]
    fn every_command_is_read_only() {
        let reg = build_registry();
        for c in reg.iter() {
            assert_eq!(
                c.schema.safety,
                SafetyTier::ReadOnly,
                "{} 必须是 T0",
                c.name
            );
        }
    }

    #[test]
    fn every_command_has_handler() {
        let reg = build_registry();
        for c in reg.iter() {
            assert!(c.handler.is_some(), "{} 缺 handler", c.name);
        }
    }

    /// 四端同源的根基：每条命令都能导出 OpenAI/MCP 形状的工具定义
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
                !t["function"]["description"]
                    .as_str()
                    .unwrap_or("")
                    .is_empty(),
                "{} 缺 description（模型靠它选工具）",
                c.name
            );
            assert!(
                t["function"]["parameters"]["properties"]["path"].is_object(),
                "{} 必须以 path 为入参",
                c.name
            );
        }
    }

    /// 必填参数缺失要被校验拒掉（校验只在 core 一处实现）
    #[test]
    fn validate_args_rejects_missing_path() {
        let reg = build_registry();
        for c in reg.iter() {
            let error = c
                .schema
                .validate_args(&serde_json::json!({}))
                .expect_err("path 是必填的，空参数必须被拒");
            assert!(!error.to_string().is_empty(), "{} 的报错是空的", c.name);
        }
    }

    /// MCP 面用同一张表：T0 一律放行（本域没有需要人类在环的命令）
    #[test]
    fn mcp_policy_admits_this_domain() {
        let reg = build_registry_with_policy(Arc::new(DenyElevated));
        assert_eq!(reg.iter().count(), 4);
        for c in reg.iter() {
            assert_eq!(c.schema.safety, SafetyTier::ReadOnly);
        }
    }

    #[test]
    fn registry_json_manifest_is_serializable() {
        let reg = build_registry();
        let text = serde_json::to_string(&reg.to_json()).expect("清单可序列化");
        assert!(
            text.contains("identify") && text.contains("regions"),
            "{text}"
        );
    }

    /// 缺 path 必须被 validate_args 拦下（MCP / Web 直传 JSON 的唯一防线）
    #[test]
    fn missing_required_arg_is_rejected_by_schema() {
        let reg = build_registry();
        for name in ["identify", "entries", "regions", "symbols"] {
            let cmd = reg.get(name).expect("命令已注册");
            let err = cmd
                .schema
                .validate_args(&serde_json::json!({}))
                .unwrap_err()
                .to_string();
            assert!(err.contains("path"), "{name}: {err}");
        }
    }
}
