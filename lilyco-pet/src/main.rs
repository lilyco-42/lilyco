//! lpet — 丛雨桌宠网关：一个二进制 = CLI + MCP 双形态（TUI/Web 随特性门控）
//!
//! ```bash
//! lpet pet-say --text "哥哥，早上好"                # 合成语音 mp3（T0 直通）
//! lpet pet-act --action face_smile                  # 动作指令 JSON（T0 直通）
//! lpet --mcp                                        # 本地 MCP：pet 前端 / Agent 直连
//! ```

use lilyco::prelude::*;
use lilyco_pet::{PetAct, PetSay};

fn main() {
    let mut reg = Registry::new(); // 默认 DenyElevated：pet-say / pet-act 均为 T0，自动放行
    reg.register(RegisteredCommand::from_app::<PetSay>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<PetAct>())
        .unwrap();

    match lilyco::detect() {
        lilyco::Backend::Mcp => lilyco::serve_mcp(reg),
        _ => lilyco::run_cli_registry("lpet", reg),
    }
}
