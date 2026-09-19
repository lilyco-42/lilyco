//! lplc — PLC 网关演示：一个二进制 = CLI + TUI + Web + MCP 四端 + 安全门 + 遥测
//!
//! ```bash
//! lplc plc-read --host 127.0.0.1:5020 --addr 0 --count 4   # CLI 读寄存器（T0 直通）
//! lplc --mcp                                                # MCP 服务器：Agent 可调
//! ```

use lilyco::prelude::*;
use lilyco_plc::{PlcRead, PlcWrite};

fn main() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<PlcRead>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<PlcWrite>())
        .unwrap();

    match lilyco::detect() {
        lilyco::Backend::Mcp => lilyco::serve_mcp(reg),
        _ => lilyco::run_cli_registry("lplc", reg),
    }
}
