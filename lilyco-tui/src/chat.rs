//! 聊天 TUI 交互状态：焦点、消息选择与输入模式。
//!
//! 所有下标都会夹紧到合法范围，任何方法都不会 panic。

use lilyco_core::action::UiAction;

/// 本地动作意图；只入队，不执行网络或真实写操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatAction {
    Send(String),
    Reply(usize),
    Edit(usize),
    Delete(usize),
    Copy(usize),
    React(usize),
    Open(usize),
}

impl From<ChatAction> for UiAction {
    fn from(action: ChatAction) -> Self {
        match action {
            ChatAction::Send(text) => UiAction::Send(text),
            ChatAction::Reply(index) => UiAction::Reply(index),
            ChatAction::Edit(index) => UiAction::Edit(index),
            ChatAction::Delete(index) => UiAction::Delete(index),
            ChatAction::Copy(index) => UiAction::Copy(index),
            ChatAction::React(index) => UiAction::React(index),
            ChatAction::Open(index) => UiAction::Open(index),
        }
    }
}

impl ChatAction {
    /// 写类动作需要确认；复制 / 打开 / 发送直接入队
    pub fn requires_confirm(&self) -> bool {
        matches!(
            self,
            ChatAction::Delete(_)
                | ChatAction::Edit(_)
                | ChatAction::Reply(_)
                | ChatAction::React(_)
        )
    }

    /// 确认层展示的动作描述
    pub fn confirm_text(&self) -> String {
        match self {
            ChatAction::Delete(index) => format!("删除第 {} 条消息？", index.saturating_add(1)),
            ChatAction::Edit(index) => format!("编辑第 {} 条消息？", index.saturating_add(1)),
            ChatAction::Reply(index) => format!("回复第 {} 条消息？", index.saturating_add(1)),
            ChatAction::React(index) => format!("回应第 {} 条消息？", index.saturating_add(1)),
            ChatAction::Copy(index) => format!("复制第 {} 条消息？", index.saturating_add(1)),
            ChatAction::Open(index) => format!("打开第 {} 条消息？", index.saturating_add(1)),
            ChatAction::Send(_) => "发送消息？".into(),
        }
    }
}

/// 聊天视图的本地交互状态
#[derive(Debug, Clone)]
pub struct ChatState {
    focus: usize,
    selected: usize,
    insert: bool,
    draft: String,
    focus_count: usize,
    confirm: Option<ChatAction>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            focus: 0,
            selected: 0,
            insert: false,
            draft: String::new(),
            focus_count: 4,
            confirm: None,
        }
    }
}

impl ChatState {
    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn pending_confirm(&self) -> Option<&ChatAction> {
        self.confirm.as_ref()
    }

    /// 请求确认；已有时覆盖为最新动作
    pub fn request_confirm(&mut self, action: ChatAction) {
        self.confirm = Some(action);
    }

    pub fn clear_confirm(&mut self) {
        self.confirm = None;
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn is_insert(&self) -> bool {
        self.insert
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }

    fn count(&self) -> usize {
        self.focus_count.max(1)
    }

    fn last_focus(&self) -> usize {
        self.count().saturating_sub(1)
    }

    /// 焦点循环到下一个 pane
    pub fn focus_next(&mut self) {
        self.focus = self.focus.min(self.last_focus());
        self.focus = (self.focus.saturating_add(1)) % self.count();
    }

    /// 焦点循环到上一个 pane
    pub fn focus_prev(&mut self) {
        self.focus = self.focus.min(self.last_focus());
        self.focus = (self.focus + self.last_focus()) % self.count();
    }

    /// 焦点左移（边界夹紧，不循环）
    pub fn focus_left(&mut self) {
        self.focus = self.focus.min(self.last_focus());
        self.focus = self.focus.saturating_sub(1);
    }

    /// 焦点右移（边界夹紧，不循环）
    pub fn focus_right(&mut self) {
        self.focus = self.focus.min(self.last_focus());
        self.focus = self.focus.saturating_add(1).min(self.last_focus());
    }

    /// 选中下一条消息；`limit` 为消息总数
    pub fn select_next(&mut self, limit: usize) {
        if limit == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self.selected.saturating_add(1).min(limit - 1);
    }

    /// 选中上一条消息
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// 选中第一条消息
    pub fn select_top(&mut self) {
        self.selected = 0;
    }

    /// 选中最后一条消息；`limit` 为消息总数
    pub fn select_bottom(&mut self, limit: usize) {
        self.selected = limit.saturating_sub(1);
    }

    /// 向下半页（固定 10 行）
    pub fn half_page_down(&mut self, limit: usize) {
        if limit == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self.selected.saturating_add(10).min(limit - 1);
    }

    /// 向上半页（固定 10 行）
    pub fn half_page_up(&mut self) {
        self.selected = self.selected.saturating_sub(10);
    }

    pub fn enter_insert(&mut self) {
        self.insert = true;
    }

    pub fn exit_insert(&mut self) {
        self.insert = false;
    }

    /// 追加一个字符到草稿
    pub fn push_char(&mut self, ch: char) {
        self.draft.push(ch);
    }

    /// 删除草稿末尾字符；空串时为无操作
    pub fn backspace(&mut self) {
        self.draft.pop();
    }

    /// 取出草稿并清空
    pub fn take_draft(&mut self) -> String {
        std::mem::take(&mut self.draft)
    }

    /// 草稿换行
    pub fn newline(&mut self) {
        self.draft.push('\n');
    }

    /// 设置焦点 pane 数量并夹紧当前焦点
    pub fn set_focus_count(&mut self, count: usize) {
        self.focus_count = count.max(1);
        self.focus = self.focus.min(self.last_focus());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_cycles_and_clamps_at_boundaries() {
        let mut chat = ChatState::default();
        assert_eq!(chat.focus(), 0);

        chat.focus_prev();
        assert_eq!(chat.focus(), 3, "从首项向上应循环到末项");
        chat.focus_next();
        assert_eq!(chat.focus(), 0, "从末项向下应循环到首项");

        chat.set_focus_count(3);
        chat.focus_left();
        assert_eq!(chat.focus(), 0, "左侧边界不越界");
        chat.focus_right();
        chat.focus_right();
        chat.focus_right();
        assert_eq!(chat.focus(), 2, "右侧边界不越界");

        chat.focus_next();
        assert_eq!(chat.focus(), 0, "循环回首项");
        chat.set_focus_count(0);
        assert_eq!(chat.focus(), 0, "零 pane 也应保持合法焦点");
        chat.focus_next();
        assert_eq!(chat.focus(), 0);
    }

    #[test]
    fn selection_is_clamped_to_limit() {
        let mut chat = ChatState::default();
        chat.select_next(3);
        chat.select_next(3);
        chat.select_next(3);
        assert_eq!(chat.selected(), 2, "选择不应超过 limit - 1");

        chat.select_prev();
        assert_eq!(chat.selected(), 1, "向上移动一条");
        chat.select_bottom(3);
        assert_eq!(chat.selected(), 2);
        chat.select_top();
        assert_eq!(chat.selected(), 0);

        chat.half_page_down(30);
        assert_eq!(chat.selected(), 10, "半页固定 10 行");
        chat.half_page_down(12);
        assert_eq!(chat.selected(), 11, "半页后夹紧到底部");
        chat.half_page_up();
        assert_eq!(chat.selected(), 1);

        chat.select_next(0);
        chat.select_bottom(0);
        assert_eq!(chat.selected(), 0, "空列表保持 0");
    }

    #[test]
    fn insert_mode_toggles_and_edits_draft() {
        let mut chat = ChatState::default();
        assert!(!chat.is_insert());
        assert_eq!(chat.draft(), "");

        chat.enter_insert();
        assert!(chat.is_insert());
        chat.push_char('你');
        chat.push_char('好');
        chat.newline();
        chat.push_char('啊');
        assert_eq!(chat.draft(), "你好\n啊");

        chat.backspace();
        assert_eq!(chat.draft(), "你好\n");
        chat.backspace();
        assert_eq!(chat.draft(), "你好");
        chat.exit_insert();
        assert!(!chat.is_insert());
        assert_eq!(chat.draft(), "你好", "退出插入不清空草稿");
    }

    #[test]
    fn backspace_on_empty_draft_does_not_panic() {
        let mut chat = ChatState::default();
        chat.backspace();
        chat.backspace();
        assert_eq!(chat.draft(), "");
    }
}
