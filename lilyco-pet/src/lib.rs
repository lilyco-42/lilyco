//! # Lilyco Pet — 丛雨桌宠桥：动作与语音成为 lilyco 工具
//!
//! 桌宠项目（lilyco-42/cute-pet，本地 `cute_box/pet`）是 macroquad 跨平台桌宠「丛雨」，
//! 已有立绘分层合成 / 眨眼口型动画 / LLM 聊天 / GPT-SoVITS 克隆音色。本 crate 把它的
//! 两个能力面抽成 lilyco 工具（`#[derive(App)]`，天生 CLI/TUI/Web/MCP 四端 + 安全门 + 遥测）：
//!
//! - [`PetSay`]（`pet-say`，T0）：EdgeTTS 文本转语音，mp3 落盘并回报路径。
//!   实现方式对齐 lly（github.com/lilyco-42/lly）的成熟方案——见 [`edge_tts`] 模块说明
//! - [`PetAct`]（`pet-act`，T0）：输出结构化动作指令 JSON（表情 / 服装 / 差分 / 快速说话），
//!   供 pet 前端进程经 IPC（本地 MCP / stdio 行协议）消费——见 [`action`] 模块说明
//!
//! 两者都标 T0 只读级：语音合成只写用户指定的（或系统临时目录）文件，
//! 动作只产出指令 JSON 而不直接驱动任何真实世界行为——默认 `DenyElevated`
//! 策略自动放行，自动化面（Agent / MCP）无需额外授权。
//!
//! 完整融合蓝图（IPC 接线 / 语音双链路 / mpkg 人设记忆 / 情感状态门控）
//! 见 `docs/ECOSYSTEM_PET.md`。
//!
//! # 用法
//!
//! ```bash
//! lpet pet-say --text "哥哥，早上好"                 # → mp3 路径 + 字节数 + 估算时长
//! lpet pet-act --action face_smile --intensity 1.5   # → 动作指令 JSON
//! lpet --mcp                                         # 本地 MCP：pet 前端 / Agent 直连
//! ```

pub mod action;
pub mod app;
pub mod edge_tts;

pub use action::PetAction;
pub use app::{PetAct, PetSay};
