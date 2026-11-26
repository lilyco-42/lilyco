//! 安全门（Safety Gate）：Token 2 Anything 的物理世界守门员
//!
//! 当命令会作用于真实世界（无人机起飞、机械臂动作、工业设备启停）时，
//! "LLM 决定调用" 与 "动作真实发生" 之间必须有一道门。本模块定义：
//! - [`SafetyTier`]：命令的安全分级（借鉴 SINT Protocol 的 T0-T3 分层，
//!   见 lyco 调研 <https://github.com/pshkv/sint-protocol>，Apache-2.0）
//! - [`SafetyPolicy`]：分级 → 放行/拒绝 的决策策略（调用面各自注入）
//! - [`GateRequest`] / [`GateDecision`]：门的输入与输出
//!
//! 接入点在 [`crate::registry::Registry`]：注册带 handler 且分级高于
//! `ReadOnly` 的命令时，自动把 handler 包进策略检查，CLI / TUI / Web / MCP
//! 四个后端共用同一道门，无一处旁路。

use serde::{Deserialize, Serialize};

/// 命令的安全分级
///
/// - `T0 ReadOnly`：只读操作（查状态、列文件），任何调用面自动放行
/// - `T1 Confirm`：需要人工确认（起飞、删除文件）——本地交互面（人类在环）
///   可放行；MCP/自动化面默认拒绝
/// - `T2 Token`：需要能力令牌（受控的批量操作）——默认拒绝，由策略校验令牌
/// - `T3 NeverAuto`：禁止任何自动执行（抛投载荷、不可逆破坏）——策略层面
///   也不得放行给自动化调用面
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyTier {
    /// T0：只读，自动放行
    #[serde(alias = "t0")]
    ReadOnly,
    /// T1：需人工确认
    #[serde(alias = "t1")]
    Confirm,
    /// T2：需能力令牌
    #[serde(alias = "t2")]
    Token,
    /// T3：禁止自动化执行
    #[serde(alias = "t3")]
    NeverAuto,
}

impl Default for SafetyTier {
    fn default() -> Self {
        SafetyTier::ReadOnly
    }
}

impl SafetyTier {
    /// 机器可读短名（MCP tools/list 描述、日志用）
    pub fn tag(self) -> &'static str {
        match self {
            SafetyTier::ReadOnly => "T0",
            SafetyTier::Confirm => "T1 confirm",
            SafetyTier::Token => "T2 token",
            SafetyTier::NeverAuto => "T3 never-auto",
        }
    }

    /// 解析宏属性 / 配置里的分级写法（"t1" / "confirm" 等），非法值返回 None
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "t0" | "read_only" | "readonly" | "read" | "open" => Some(SafetyTier::ReadOnly),
            "t1" | "confirm" | "confirmation" => Some(SafetyTier::Confirm),
            "t2" | "token" | "capability" => Some(SafetyTier::Token),
            "t3" | "never_auto" | "neverauto" | "never" | "forbidden" => {
                Some(SafetyTier::NeverAuto)
            }
            _ => None,
        }
    }
}

/// 门的输入：哪个命令、什么分级、什么参数
pub struct GateRequest<'a> {
    /// 命令规范名（kebab-case）
    pub command: &'a str,
    /// 该命令声明的安全分级
    pub tier: SafetyTier,
    /// 本次调用的参数 JSON（策略可据此做细粒度判断，如限高、地理围栏）
    pub args: &'a serde_json::Value,
}

/// 门的输出：放行，或拒绝并给出原因
#[derive(Debug, Clone, PartialEq)]
pub enum GateDecision {
    /// 放行，执行真实 handler
    Allow,
    /// 拒绝，原因会以 [`crate::error::AppError::Safety`] 返回给调用面
    Deny(String),
}

/// 安全策略：把 (命令, 分级, 参数) 映射为放行/拒绝
///
/// 调用面各自注入：MCP 服务器用严格策略、本地交互 CLI 用宽松策略、
/// 测试用 [`Unrestricted`]。核心不内置"对的方向"，只保证门永远存在
/// 且默认 fail-closed。
pub trait SafetyPolicy: Send + Sync {
    fn check(&self, req: GateRequest<'_>) -> GateDecision;
}

/// 默认策略（fail-closed）：只放行 T0 只读命令
///
/// MCP 服务器 / 无人值守自动化面的正确缺省。T1+ 全部拒绝，
/// 拒绝信息里写明分级与放行途径。
#[derive(Debug, Clone, Copy, Default)]
pub struct DenyElevated;

impl SafetyPolicy for DenyElevated {
    fn check(&self, req: GateRequest<'_>) -> GateDecision {
        match req.tier {
            SafetyTier::ReadOnly => GateDecision::Allow,
            other => GateDecision::Deny(format!(
                "命令 `{}` 分级为 {}，自动化调用面默认拒绝；请在本地交互面执行，或注册时换用允许该分级的 SafetyPolicy",
                req.command,
                other.tag()
            )),
        }
    }
}

/// 本地交互策略：人类在环（T0/T1 放行），高危（T2/T3）仍拒绝
///
/// 适用于人亲手敲命令的 CLI / TUI 面——按键本身就是确认动作。
#[derive(Debug, Clone, Copy, Default)]
pub struct Interactive;

impl SafetyPolicy for Interactive {
    fn check(&self, req: GateRequest<'_>) -> GateDecision {
        match req.tier {
            SafetyTier::ReadOnly | SafetyTier::Confirm => GateDecision::Allow,
            other => GateDecision::Deny(format!(
                "命令 `{}` 分级为 {}，交互面也不放行：T2 需要能力令牌策略，T3 禁止自动化执行",
                req.command,
                other.tag()
            )),
        }
    }
}

/// 全放行策略（仅限测试 / 明确自担风险的场景）
///
/// 名字刻意刺眼：grep 到它出现在生产代码里就是坏味道。
#[derive(Debug, Clone, Copy, Default)]
pub struct Unrestricted;

impl SafetyPolicy for Unrestricted {
    fn check(&self, _req: GateRequest<'_>) -> GateDecision {
        GateDecision::Allow
    }
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req<'a>(tier: SafetyTier, args: &'a serde_json::Value) -> GateRequest<'a> {
        GateRequest {
            command: "drone-takeoff",
            tier,
            args,
        }
    }

    #[test]
    fn tier_parse_accepts_both_short_and_long_forms() {
        assert_eq!(SafetyTier::parse("t0"), Some(SafetyTier::ReadOnly));
        assert_eq!(SafetyTier::parse("t1"), Some(SafetyTier::Confirm));
        assert_eq!(SafetyTier::parse("t2"), Some(SafetyTier::Token));
        assert_eq!(SafetyTier::parse("t3"), Some(SafetyTier::NeverAuto));
        assert_eq!(SafetyTier::parse("Confirm"), Some(SafetyTier::Confirm));
        assert_eq!(SafetyTier::parse("never_auto"), Some(SafetyTier::NeverAuto));
        assert_eq!(SafetyTier::parse("bogus"), None);
    }

    #[test]
    fn tier_serde_roundtrip_with_aliases() {
        for (json_str, tier) in [
            (r#""read_only""#, SafetyTier::ReadOnly),
            (r#""t1""#, SafetyTier::Confirm),
            (r#""t3""#, SafetyTier::NeverAuto),
        ] {
            let t: SafetyTier = serde_json::from_str(json_str).unwrap();
            assert_eq!(t, tier, "alias {json_str} should deserialize");
        }
        assert_eq!(
            serde_json::to_string(&SafetyTier::NeverAuto).unwrap(),
            r#""never_auto""#
        );
    }

    #[test]
    fn default_tier_is_read_only() {
        assert_eq!(SafetyTier::default(), SafetyTier::ReadOnly);
    }

    #[test]
    fn deny_elevated_allows_read_only_only() {
        let args = json!({});
        assert_eq!(
            DenyElevated.check(req(SafetyTier::ReadOnly, &args)),
            GateDecision::Allow
        );
        for tier in [
            SafetyTier::Confirm,
            SafetyTier::Token,
            SafetyTier::NeverAuto,
        ] {
            match DenyElevated.check(req(tier, &args)) {
                GateDecision::Deny(msg) => {
                    assert!(msg.contains("drone-takeoff"), "{msg}");
                    assert!(msg.contains(tier.tag()), "{msg}");
                }
                GateDecision::Allow => panic!("DenyElevated must not allow {tier:?}"),
            }
        }
    }

    #[test]
    fn interactive_allows_confirm_but_not_token_or_never_auto() {
        let args = json!({});
        assert_eq!(
            Interactive.check(req(SafetyTier::ReadOnly, &args)),
            GateDecision::Allow
        );
        assert_eq!(
            Interactive.check(req(SafetyTier::Confirm, &args)),
            GateDecision::Allow
        );
        assert!(matches!(
            Interactive.check(req(SafetyTier::NeverAuto, &args)),
            GateDecision::Deny(_)
        ));
    }

    #[test]
    fn unrestricted_allows_everything() {
        let args = json!({});
        for tier in [
            SafetyTier::ReadOnly,
            SafetyTier::Confirm,
            SafetyTier::Token,
            SafetyTier::NeverAuto,
        ] {
            assert_eq!(Unrestricted.check(req(tier, &args)), GateDecision::Allow);
        }
    }

    #[test]
    fn tier_tags_are_stable() {
        assert_eq!(SafetyTier::ReadOnly.tag(), "T0");
        assert_eq!(SafetyTier::Confirm.tag(), "T1 confirm");
        assert_eq!(SafetyTier::Token.tag(), "T2 token");
        assert_eq!(SafetyTier::NeverAuto.tag(), "T3 never-auto");
    }
}
