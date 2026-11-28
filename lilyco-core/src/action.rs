//! 从 UI 回传给 App 的动作意图。
//!
//! TUI 只产生 intent；是否执行、是否通过权限门由 App 决定。

/// 聊天 UI 动作
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    Send(String),
    Reply(usize),
    Edit(usize),
    Delete(usize),
    Copy(usize),
    React(usize),
    Open(usize),
}
