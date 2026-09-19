//! lbitnet — 本地 BitNet 采样桥演示：一个二进制 = CLI + TUI + Web + MCP 四端
//!
//! ```bash
//! lbitnet bitnet-ask --prompt "你好" --max-tokens 128          # CLI 本地推理（T0 直通）
//! lbitnet bitnet-chat-demo --prompt "介绍一下你自己"           # host 桥注入演示
//! lbitnet --mcp                                                # MCP：Agent 可调（离线采样）
//! ```

use lilyco::prelude::*;
use lilyco_bitnet::{BitNetAsk, BitNetChatDemo};

fn main() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<BitNetAsk>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<BitNetChatDemo>())
        .unwrap();

    match lilyco::detect() {
        lilyco::Backend::Mcp => lilyco::serve_mcp(reg),
        _ => lilyco::run_cli_registry("lbitnet", reg),
    }
}
