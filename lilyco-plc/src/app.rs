//! PLC 读写两个派生应用——同一 struct 派生 CLI/TUI/Web/MCP，天生受安全门控
//!
//! - [`PlcRead`]：T0 只读，自动化面（MCP/Agent）直接放行，读取期间逐寄存器上报遥测
//! - [`PlcWrite`]：T2 能力令牌——自动化面默认拒绝（P0 安全门），
//!   部署侧注入校验 token 的 `SafetyPolicy` 后放行

use crate::ModbusTcp;
use lilyco::prelude::*;

/// 读 PLC 保持寄存器（T0 只读）
#[derive(App)]
#[app(
    name = "plc-read",
    about = "读 PLC 保持寄存器（Modbus TCP，只读），读取期间逐寄存器上报遥测",
    run = "run_read"
)]
pub struct PlcRead {
    /// PLC 地址 host:port，如 127.0.0.1:5020
    host: String,
    /// Modbus unit id（默认 1）
    unit: Option<u8>,
    /// 起始寄存器地址（0 起）
    addr: u16,
    /// 读取数量（默认 1，最多 125）
    count: Option<u16>,
}

pub fn run_read(app: &PlcRead, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let mut client = ModbusTcp::connect(app.host.as_str(), app.unit.unwrap_or(1))
        .map_err(|e| AppError::Runtime(format!("PLC 连接失败: {e}")))?;
    let count = app.count.unwrap_or(1);
    let regs = client
        .read_holding_registers(app.addr, count)
        .map_err(|e| AppError::Runtime(format!("PLC 读取失败: {e}")))?;
    // P1 遥测：每个寄存器一个数据点，Agent 实时看到 PLC 状态
    for (i, v) in regs.iter().enumerate() {
        ctx.telemetry(
            format!("reg{}", app.addr as usize + i),
            serde_json::json!(v),
        );
    }
    let r = serde_json::json!({ "addr": app.addr, "values": regs });
    ctx.done(r.clone(), 0);
    Ok(r)
}

/// 写 PLC 单个保持寄存器（T2 需能力令牌——自动化面默认拒绝）
#[derive(App)]
#[app(
    name = "plc-write",
    about = "写 PLC 单个保持寄存器（Modbus TCP，需能力令牌，自动化面默认拒绝）",
    run = "run_write",
    safety = "t2"
)]
pub struct PlcWrite {
    /// PLC 地址 host:port
    host: String,
    /// Modbus unit id（默认 1）
    unit: Option<u8>,
    /// 目标寄存器地址
    addr: u16,
    /// 写入值
    value: u16,
}

pub fn run_write(app: &PlcWrite, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let mut client = ModbusTcp::connect(app.host.as_str(), app.unit.unwrap_or(1))
        .map_err(|e| AppError::Runtime(format!("PLC 连接失败: {e}")))?;
    client
        .write_single_register(app.addr, app.value)
        .map_err(|e| AppError::Runtime(format!("PLC 写入失败: {e}")))?;
    ctx.telemetry(format!("reg{}", app.addr), serde_json::json!(app.value));
    let r = serde_json::json!({ "addr": app.addr, "written": app.value });
    ctx.done(r.clone(), 0);
    Ok(r)
}
