//! Token 2 Anything 硬件桥端到端（P2）
//!
//! 链路：Agent token → 安全门（T0 直通 / T2 验令牌）→ Modbus TCP → mock PLC 寄存器，
//! 全程零真硬件、云端可跑。

use lilyco::prelude::*;
use lilyco_core::safety::{GateDecision, GateRequest};
use lilyco_plc::{MockPlc, ModbusTcp, PlcRead, PlcWrite};
use std::sync::Arc;

fn handler_of(reg: &Registry, name: &str) -> Handler {
    reg.get(name).unwrap().handler.clone().unwrap()
}

// ── Modbus 客户端 vs mock PLC ─────────────────────────────

#[test]
fn client_read_write_coil_against_mock() {
    let plc = MockPlc::spawn(vec![10, 20, 30]).unwrap();
    let mut c = ModbusTcp::connect(plc.addr, 1).unwrap();

    // fc 0x03 读
    assert_eq!(c.read_holding_registers(0, 3).unwrap(), vec![10, 20, 30]);
    // fc 0x06 写单寄存器 → 回读验证落盘
    c.write_single_register(1, 99).unwrap();
    assert_eq!(c.read_holding_registers(0, 3).unwrap(), vec![10, 99, 30]);
    // fc 0x05 写线圈（0xFF00=1）→ 回读
    c.write_single_coil(2, true).unwrap();
    assert_eq!(c.read_holding_registers(2, 1).unwrap(), vec![1]);
}

#[test]
fn out_of_range_read_surfaces_modbus_exception() {
    let plc = MockPlc::spawn(vec![1]).unwrap();
    let mut c = ModbusTcp::connect(plc.addr, 1).unwrap();
    // 越界读 → mock 回异常码 0x02（非法地址）
    let err = c.read_holding_registers(5, 2).unwrap_err();
    assert!(err.to_string().contains("异常响应"), "{err}");
    assert!(err.to_string().contains("code=0x02"), "{err}");
}

#[test]
fn count_out_of_bounds_is_rejected_locally() {
    let plc = MockPlc::spawn(vec![1]).unwrap();
    let mut c = ModbusTcp::connect(plc.addr, 1).unwrap();
    assert!(c.read_holding_registers(0, 0).is_err());
    assert!(c.read_holding_registers(0, 126).is_err());
}

// ── 安全门 + 遥测端到端 ───────────────────────────────────

#[test]
fn plc_read_passes_gate_and_emits_telemetry() {
    let plc = MockPlc::spawn(vec![7, 8]).unwrap();
    let mut reg = Registry::new(); // 默认 DenyElevated：T0 直通
    reg.register(RegisteredCommand::from_app::<PlcRead>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "plc-read"),
        serde_json::json!({ "host": plc.addr.to_string(), "addr": 0, "count": 2 }),
    );
    let result = outcome.result.expect("T0 只读命令必须放行");
    assert_eq!(result["values"], serde_json::json!([7, 8]));

    // P1 遥测：每个寄存器一个数据点
    assert!(outcome.events.iter().any(|e| matches!(
        e,
        Progress::Telemetry { key, value }
            if key == "reg0" && value.as_i64() == Some(7)
    )));
    assert!(outcome.events.iter().any(|e| matches!(
        e,
        Progress::Telemetry { key, .. } if key == "reg1"
    )));
}

#[test]
fn plc_write_blocked_by_default_policy() {
    let plc = MockPlc::spawn(vec![0]).unwrap();
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<PlcWrite>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "plc-write"),
        serde_json::json!({ "host": plc.addr.to_string(), "addr": 0, "value": 42 }),
    );
    // T2：自动化面默认拒绝，且拒绝发生在连接 PLC 之前（门在 handler 外层）
    let err = outcome.result.unwrap_err();
    assert!(matches!(err, AppError::Safety(_)), "{err}");
    assert!(err.to_string().contains("plc-write"), "{err}");
}

/// 能力令牌策略：T2 命令须携带正确 token 才放行（P0 SafetyPolicy 扩展点实战）
struct TokenPolicy {
    token: &'static str,
}

impl SafetyPolicy for TokenPolicy {
    fn check(&self, req: GateRequest<'_>) -> GateDecision {
        match req.tier {
            SafetyTier::ReadOnly => GateDecision::Allow,
            SafetyTier::Token => {
                let ok = req.args.get("token").and_then(|v| v.as_str()) == Some(self.token);
                if ok {
                    GateDecision::Allow
                } else {
                    GateDecision::Deny("能力令牌缺失或不匹配".into())
                }
            }
            other => GateDecision::Deny(format!("分级 {} 在此面不允许", other.tag())),
        }
    }
}

#[test]
fn plc_write_requires_token_then_lands_in_plc() {
    let plc = MockPlc::spawn(vec![0, 0]).unwrap();
    let mut reg = Registry::new().with_policy(Arc::new(TokenPolicy { token: "SECRET-42" }));
    reg.register(RegisteredCommand::from_app::<PlcWrite>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<PlcRead>())
        .unwrap();

    // 无 token → 门拒
    let h = handler_of(&reg, "plc-write");
    let denied = execute(
        h.clone(),
        serde_json::json!({ "host": plc.addr.to_string(), "addr": 1, "value": 42 }),
    );
    assert!(
        matches!(denied.result, Err(AppError::Safety(_))),
        "{denied:?}"
    );

    // 错 token → 门拒
    let bad = execute(
        h.clone(),
        serde_json::json!({ "host": plc.addr.to_string(), "addr": 1, "value": 42, "token": "WRONG" }),
    );
    assert!(matches!(bad.result, Err(AppError::Safety(_))));

    // 对 token → 放行，真实写进 PLC
    let ok = execute(
        h,
        serde_json::json!({ "host": plc.addr.to_string(), "addr": 1, "value": 42, "token": "SECRET-42" }),
    );
    assert!(ok.result.is_ok(), "{:?}", ok.result);

    // T0 读回验证落盘
    let read = execute(
        handler_of(&reg, "plc-read"),
        serde_json::json!({ "host": plc.addr.to_string(), "addr": 1, "count": 1 }),
    );
    assert_eq!(read.result.unwrap()["values"], serde_json::json!([42]));
}
