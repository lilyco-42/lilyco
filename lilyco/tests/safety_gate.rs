//! Token 2 Anything 安全门端到端验收（P0）
//!
//! 场景：LLM 通过 MCP 调用无人机命令。派生宏声明的安全分级在注册表
//! 生成执行路径时被自动包上门——未批准的 `drone-takeoff` 必须被拦下，
//! `drone-payload-drop`（T3）在任何自动化面都不可执行，`drone-status`（T0）直通。
//!
//! 借鉴 SINT Protocol 的 T0-T3 分层（lyco 调研，2026-09）。

use lilyco::prelude::*;
use std::sync::Arc;

#[derive(App)]
#[app(
    name = "drone-status",
    about = "读取无人机状态（只读）",
    run = "run_status"
)]
struct DroneStatus {}

fn run_status(_app: &DroneStatus, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let r = serde_json::json!({ "battery": 87, "gps": 12 });
    ctx.done(r.clone(), 1);
    Ok(r)
}

#[derive(App)]
#[app(
    name = "drone-takeoff",
    about = "无人机起飞（需人工确认）",
    run = "run_takeoff",
    safety = "t1"
)]
struct DroneTakeoff {
    /// 目标高度（米）
    altitude: f64,
}

fn run_takeoff(app: &DroneTakeoff, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let r = serde_json::json!({ "took_off": true, "altitude": app.altitude });
    ctx.done(r.clone(), 2);
    Ok(r)
}

#[derive(App)]
#[app(
    name = "drone-payload-drop",
    about = "抛投载荷（不可逆，禁止自动化）",
    run = "run_drop",
    safety = "t3"
)]
struct DronePayloadDrop {
    /// 投放目标
    target: String,
}

fn run_drop(app: &DronePayloadDrop, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let r = serde_json::json!({ "dropped_at": app.target });
    ctx.done(r.clone(), 3);
    Ok(r)
}

fn drone_registry() -> Registry {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<DroneStatus>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<DroneTakeoff>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<DronePayloadDrop>())
        .unwrap();
    reg
}

fn handler_of(reg: &Registry, name: &str) -> Handler {
    reg.get(name).unwrap().handler.clone().unwrap()
}

#[test]
fn derive_declares_safety_tier_in_schema() {
    assert_eq!(DroneStatus::schema().safety, SafetyTier::ReadOnly);
    assert_eq!(DroneTakeoff::schema().safety, SafetyTier::Confirm);
    assert_eq!(DronePayloadDrop::schema().safety, SafetyTier::NeverAuto);
}

#[test]
fn t0_status_executes_on_automated_surface() {
    let reg = drone_registry();
    let outcome = execute(handler_of(&reg, "drone-status"), serde_json::json!({}));
    let result = outcome.result.expect("T0 只读命令必须放行");
    assert_eq!(result["battery"], 87);
}

#[test]
fn t1_takeoff_is_denied_on_automated_surface() {
    let reg = drone_registry();
    let outcome = execute(
        handler_of(&reg, "drone-takeoff"),
        serde_json::json!({ "altitude": 50 }),
    );
    let err = outcome.result.expect_err("MCP 自动化面必须拒绝 T1");
    assert!(matches!(err, AppError::Safety(_)), "{err}");
    assert!(err.to_string().contains("drone-takeoff"), "{err}");
    // 协议不变量：事件流仍以 Error 终态收尾
    assert!(matches!(outcome.last_event(), Some(Progress::Error { .. })));
}

#[test]
fn t1_takeoff_executes_when_human_in_the_loop() {
    // 本地交互面：人类亲手敲的命令即确认动作
    let mut reg = Registry::new().with_policy(Arc::new(Interactive));
    reg.register(RegisteredCommand::from_app::<DroneTakeoff>())
        .unwrap();
    let outcome = execute(
        handler_of(&reg, "drone-takeoff"),
        serde_json::json!({ "altitude": 120 }),
    );
    let result = outcome.result.expect("交互面 T1 应放行");
    assert_eq!(result["took_off"], true);
    assert_eq!(result["altitude"], 120.0);
}

#[test]
fn t3_payload_drop_is_denied_everywhere() {
    // 自动化面（默认 fail-closed）
    let reg = drone_registry();
    let outcome = execute(
        handler_of(&reg, "drone-payload-drop"),
        serde_json::json!({ "target": "sim-target" }),
    );
    assert!(matches!(outcome.result, Err(AppError::Safety(_))));

    // 交互面也不放行
    let mut reg2 = Registry::new().with_policy(Arc::new(Interactive));
    reg2.register(RegisteredCommand::from_app::<DronePayloadDrop>())
        .unwrap();
    let outcome2 = execute(
        handler_of(&reg2, "drone-payload-drop"),
        serde_json::json!({ "target": "sim-target" }),
    );
    assert!(matches!(outcome2.result, Err(AppError::Safety(_))));
}

/// 地理围栏策略：策略可以看参数做细粒度决策（T2 能力令牌的雏形）
struct Geofence {
    max_altitude: f64,
}

impl SafetyPolicy for Geofence {
    fn check(&self, req: GateRequest<'_>) -> GateDecision {
        let alt = req
            .args
            .get("altitude")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if alt <= self.max_altitude {
            GateDecision::Allow
        } else {
            GateDecision::Deny(format!(
                "地理围栏：限高 {}m，请求 {alt}m",
                self.max_altitude
            ))
        }
    }
}

#[test]
fn policy_can_inspect_args_for_geofence() {
    let mut reg = Registry::new().with_policy(Arc::new(Geofence {
        max_altitude: 120.0,
    }));
    reg.register(RegisteredCommand::from_app::<DroneTakeoff>())
        .unwrap();
    let h = handler_of(&reg, "drone-takeoff");

    let ok = execute(h.clone(), serde_json::json!({ "altitude": 100 }));
    assert_eq!(ok.result.expect("围栏内应放行")["altitude"], 100.0);

    let denied = execute(h, serde_json::json!({ "altitude": 500 }));
    assert!(
        matches!(&denied.result, Err(AppError::Safety(m)) if m.contains("500")),
        "{:?}",
        denied.result
    );
}

#[test]
fn tools_list_description_exposes_tier_to_agents() {
    // MCP 侧把分级写进 description：Agent 在 tools/list 就能看到门槛
    let server = lilyco_mcp::McpServer::new(drone_registry());
    let raw = server
        .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .expect("tools/list should respond");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    let by_name = |n: &str| {
        tools
            .iter()
            .find(|t| t["name"] == n)
            .unwrap_or_else(|| panic!("tool {n} missing"))
    };
    let status = by_name("drone-status");
    assert!(!status["description"].as_str().unwrap().contains("safety:"));
    let takeoff = by_name("drone-takeoff");
    assert!(
        takeoff["description"]
            .as_str()
            .unwrap()
            .contains("T1 confirm"),
        "{:?}",
        takeoff["description"]
    );
    let drop = by_name("drone-payload-drop");
    assert!(drop["description"]
        .as_str()
        .unwrap()
        .contains("T3 never-auto"));
}
