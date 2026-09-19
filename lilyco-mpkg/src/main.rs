//! lmpkg — 记忆包工具箱演示：一个二进制 = CLI + TUI + Web + MCP 四端 + 安全门 + 遥测
//!
//! ```bash
//! lmpkg mpkg-put --dir ./mpkg-store --file snake.mpkg.json    # T2：需能力令牌策略
//! lmpkg mpkg-get --dir ./mpkg-store --name make-snake-game    # T0：按名取出
//! lmpkg mpkg-verify --dir ./mpkg-store --hash sha256:032bfc…  # T0：完整性校验
//! lmpkg --mcp                                                 # MCP 服务器：Agent 可调
//! ```

use lilyco::prelude::*;
use lilyco_mpkg::{MpkgGet, MpkgPut, MpkgVerify};

fn main() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<MpkgPut>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<MpkgGet>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<MpkgVerify>())
        .unwrap();

    match lilyco::detect() {
        lilyco::Backend::Mcp => lilyco::serve_mcp(reg),
        _ => lilyco::run_cli_registry("lmpkg", reg),
    }
}
