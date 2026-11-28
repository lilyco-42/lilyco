use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use lilyco_core::schema::CommandSchema;
use lilyco_core::view::{Component, ViewSlot};

use crate::chat::{ChatAction, ChatState};
use crate::config::{KeyAction, Keymap};
use crate::renderer::{self, AppState, FormRenderer};
use crate::theme::Theme;
use crate::view::ViewState;
use crate::widgets::{FieldValue, FormField};

// ── TuiApp ────────────────────────────────────────────────

/// TUI 应用的完整状态
pub struct TuiApp {
    pub form: FormRenderer,
    /// 是否应退出
    pub should_quit: bool,
    /// 帮助面板是否显示
    pub show_help: bool,
    /// 多命令模式：可选命令 schema（`None` = 单命令模式）
    pub commands: Option<Vec<CommandSchema>>,
    /// 应用名（多命令选择页标题）
    pub app_name: String,
    /// 命令选择页当前高亮索引
    pub selected_command: usize,
    /// 当前激活的命令名（执行时 facade 按它取 handler）
    pub active_command: Option<String>,
    /// 路径补全会话：`(base 键, 候选列表, 当前索引)`；值变更即失效
    completion: Option<(String, Vec<String>, usize)>,
    /// 语义视图状态（静态槽位 + 运行时组件）
    pub view: ViewState,
    /// 聊天交互状态（焦点 / 选择 / 输入模式 / 草稿）
    pub chat: ChatState,
    /// 本地动作 intent 队列（只入队，不执行）
    pub pending_actions: Vec<ChatAction>,
    /// 聊天键位
    pub keymap: Keymap,
    /// 主题令牌
    pub theme: Theme,
    /// `gg` 前缀键是否按下
    g_pending: bool,
    /// 每个命令声明的静态视图槽位
    views_by_command: std::collections::HashMap<String, Vec<ViewSlot>>,
}

impl TuiApp {
    /// 单命令模式：从 CommandSchema 创建
    pub fn new(schema: &CommandSchema) -> Self {
        Self {
            form: FormRenderer::new(schema),
            should_quit: false,
            show_help: false,
            commands: None,
            app_name: schema.name.clone(),
            selected_command: 0,
            active_command: Some(schema.name.clone()),
            completion: None,
            view: ViewState::default(),
            chat: ChatState::default(),
            pending_actions: Vec::new(),
            keymap: Keymap::load_default(),
            theme: Theme::load_default(),
            g_pending: false,
            views_by_command: std::collections::HashMap::new(),
        }
    }

    /// 多命令模式：命令选择页起步，选中后进入对应表单
    ///
    /// `schemas` 应来自 Registry 的可见命令（隐藏命令由调用方过滤）。
    pub fn new_multi(app_name: &str, schemas: Vec<CommandSchema>) -> Self {
        let mut form = schemas
            .first()
            .map(FormRenderer::new)
            .expect("new_multi requires at least one command");
        // form 仅作占位（首个命令的表单），有效状态是命令选择页
        form.app_state = AppState::CommandSelect;
        Self {
            form,
            should_quit: false,
            show_help: false,
            commands: Some(schemas),
            app_name: app_name.to_string(),
            selected_command: 0,
            active_command: None,
            completion: None,
            view: ViewState::default(),
            chat: ChatState::default(),
            pending_actions: Vec::new(),
            keymap: Keymap::load_default(),
            theme: Theme::load_default(),
            g_pending: false,
            views_by_command: std::collections::HashMap::new(),
        }
    }

    /// 绑定静态视图槽位（单命令）
    pub fn with_views(mut self, slots: Vec<ViewSlot>) -> Self {
        let name = self.form.command_name.clone();
        self.views_by_command.insert(name, slots.clone());
        self.view = ViewState::new(slots);
        self
    }

    /// 注入自定义键位（测试 / 宿主）
    pub fn with_keymap(mut self, keymap: Keymap) -> Self {
        self.keymap = keymap;
        self
    }

    /// 注入自定义主题（测试 / 宿主）
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// 取走本地动作 intent 队列
    pub fn drain_actions(&mut self) -> Vec<ChatAction> {
        std::mem::take(&mut self.pending_actions)
    }

    /// 多命令模式 + 每命令视图槽位
    pub fn new_multi_with_views(
        app_name: &str,
        entries: Vec<(CommandSchema, Vec<ViewSlot>)>,
    ) -> Self {
        let schemas: Vec<CommandSchema> =
            entries.iter().map(|(schema, _)| schema.clone()).collect();
        let mut app = Self::new_multi(app_name, schemas);
        app.views_by_command = entries
            .into_iter()
            .map(|(schema, slots)| (schema.name.clone(), slots))
            .collect();
        app
    }

    /// 写入运行时视图组件（同 id 覆盖）
    pub fn apply_view(&mut self, id: impl Into<String>, component: Component) {
        self.view.apply(id, component);
        self.sync_chat_limit();
        self.sync_view_chat_state();
    }

    /// 把本地聊天状态写回语义视图，驱动选中行与输入区渲染
    fn sync_view_chat_state(&mut self) {
        self.view.apply_chat_state(
            self.chat.selected(),
            self.chat.draft(),
            self.chat.is_insert(),
        );
    }

    /// 当前视图中第一个 MessageList 的消息条数
    fn message_list_limit(&self) -> Option<usize> {
        self.view.iter_components().find_map(message_list_len)
    }

    /// 把聊天选中下标夹紧到消息列表长度
    fn sync_chat_limit(&mut self) {
        let Some(limit) = self.message_list_limit() else {
            return;
        };
        if limit == 0 {
            self.chat.select_top();
        } else if self.chat.selected() >= limit {
            self.chat.select_bottom(limit);
        }
    }

    /// 获取当前状态
    pub fn state(&self) -> &AppState {
        &self.form.app_state
    }

    /// 获取可变字段引用（测试用）
    pub fn fields_mut(&mut self) -> &mut Vec<FormField> {
        &mut self.form.fields
    }

    pub fn fields(&self) -> &[FormField] {
        &self.form.fields
    }

    // ── 事件分发 ───────────────────────────────────────

    /// 处理一个按键事件，返回 `false` 表示应退出事件循环
    pub fn handle_event(&mut self, key: KeyEvent) -> bool {
        match self.form.app_state {
            AppState::CommandSelect => self.handle_select_event(key),
            AppState::Form => self.handle_form_event(key),
            AppState::Confirm => self.handle_confirm_event(key),
            AppState::Running => self.handle_running_event(key),
            AppState::Done | AppState::Error => self.handle_terminal_event(key),
        }
    }

    /// 渲染当前状态到 Buffer
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        match self.form.app_state {
            AppState::CommandSelect => {
                if let Some(schemas) = &self.commands {
                    renderer::render_command_select(
                        &self.app_name,
                        schemas,
                        self.selected_command,
                        area,
                        buf,
                    );
                }
            }
            AppState::Form => {
                renderer::render_form(&self.form, area, buf);
                if self.show_help {
                    render_help_overlay(area, buf);
                }
            }
            AppState::Running => {
                if self.view.is_empty() {
                    renderer::render_running(&self.form, area, buf);
                } else {
                    renderer::render_running_with_view(
                        &self.form,
                        &self.view,
                        self.pending_actions.len(),
                        self.chat.pending_confirm(),
                        &self.theme,
                        area,
                        buf,
                    );
                }
            }
            AppState::Confirm => {
                renderer::render_form(&self.form, area, buf);
                renderer::render_confirm(&self.form, area, buf);
            }
            AppState::Done => {
                renderer::render_done(&self.form, area, buf);
            }
            AppState::Error => {
                renderer::render_error(&self.form, area, buf);
            }
        }
    }

    // ── 各状态事件处理 ─────────────────────────────────

    /// 命令选择页：↑↓/jk 移动，Enter 进入表单，Esc/q 退出
    fn handle_select_event(&mut self, key: KeyEvent) -> bool {
        use KeyCode::*;
        let count = self.commands.as_ref().map(Vec::len).unwrap_or(0);
        match key.code {
            Esc | Char('q') => {
                self.should_quit = true;
                return false;
            }
            Up | Char('k') => {
                self.selected_command = self.selected_command.saturating_sub(1);
            }
            Down | Char('j') => {
                if count > 0 {
                    self.selected_command = (self.selected_command + 1).min(count - 1);
                }
            }
            Enter => {
                if let Some(schemas) = &self.commands {
                    let idx = self.selected_command.min(schemas.len() - 1);
                    let schema = schemas[idx].clone();
                    self.form = FormRenderer::new(&schema);
                    self.active_command = Some(schema.name.clone());
                    let slots = self
                        .views_by_command
                        .get(&schema.name)
                        .cloned()
                        .unwrap_or_default();
                    self.view = ViewState::new(slots);
                }
            }
            _ => {}
        }
        true
    }

    fn handle_form_event(&mut self, key: KeyEvent) -> bool {
        use KeyCode::*;

        if self.show_help {
            if matches!(key.code, Esc | Enter | Tab | F(1)) {
                self.show_help = false;
            }
            return true;
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                Down => self.form.next_field(),
                Up => self.form.prev_field(),
                _ => return true,
            }
            self.completion = None;
            return true;
        }

        // 全局快捷键优先
        match key.code {
            Esc => {
                self.should_quit = true;
                return false;
            }
            Tab => {
                // 路径输入中 Tab 保持补全，Alt+↓ 始终可前往下一字段。
                let is_path_with_input = self
                    .form
                    .focused_field()
                    .is_some_and(|f| matches!(&f.value, FieldValue::Path(v) if !v.is_empty()));
                if is_path_with_input {
                    self.path_complete();
                } else {
                    self.completion = None;
                    self.form.next_field();
                }
                return true;
            }
            BackTab => {
                self.completion = None;
                self.form.prev_field();
                return true;
            }
            Enter => {
                // 全量校验（required / Number 范围 / Path must_exist）不通过则
                // 留在表单并显示红色消息；通过则清消息进入确认
                let errors = self.form.validation_errors();
                if errors.is_empty() {
                    self.form.validation_message = None;
                    self.form.app_state = AppState::Confirm;
                } else {
                    self.form.validation_message = Some(errors.join("；"));
                }
                return true;
            }
            F(1) => {
                self.show_help = !self.show_help;
                return true;
            }
            _ => {}
        }

        // 传递给焦点字段
        if let Some(field) = self.form.focused_field_mut() {
            let changed = field.handle_key(key);
            if changed {
                // 值变更后旧校验消息与补全会话失效
                self.form.validation_message = None;
                self.completion = None;
                // 更新滚动位置
                self.update_scroll();
            }
        }

        true
    }

    /// Path 字段 Tab 补全（readline 风格）
    ///
    /// - 首次 Tab：按当前输入读目录、过滤前缀，补全到第一个候选
    /// - 再次 Tab（同 base）：循环候选（目录条目带 `/` 后缀）
    /// - 无候选：不改动值，会话置空
    fn path_complete(&mut self) {
        use std::path::Path as StdPath;

        let Some(field) = self.form.focused_field() else {
            return;
        };
        let FieldValue::Path(text) = &field.value else {
            return;
        };
        let (dir, prefix) = split_dir_prefix(text);
        let base = format!("{dir}\0{prefix}");

        // 同一会话：循环候选。续期条件 = base 未变（尚未补全）
        // 或 当前文本正是上次应用的候选（补全后的循环；编辑过则开新会话）
        if let Some((b, cands, idx)) = &mut self.completion {
            let applied = cands.get(*idx).map(String::as_str) == Some(text.as_str());
            if !cands.is_empty() && (*b == base || applied) {
                *idx = (*idx + 1) % cands.len();
                let value = cands[*idx].clone();
                if let FieldValue::Path(v) = &mut self.form.focused_field_mut().unwrap().value {
                    *v = value;
                }
                return;
            }
        }

        // 新会话：读目录
        let read_dir = if dir.is_empty() {
            StdPath::new(".")
        } else {
            StdPath::new(&dir)
        };
        let mut cands: Vec<String> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(read_dir) {
            for entry in rd.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with(&prefix) {
                    continue;
                }
                let suffix = if entry.path().is_dir() { "/" } else { "" };
                cands.push(format!("{dir}{name}{suffix}"));
            }
        }
        cands.sort();
        if cands.is_empty() {
            self.completion = None;
            return;
        }
        self.completion = Some((base, cands.clone(), 0));
        if let FieldValue::Path(v) = &mut self.form.focused_field_mut().unwrap().value {
            *v = cands[0].clone();
        }
    }

    fn handle_confirm_event(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => {
                self.form.app_state = AppState::Running;
                true
            }
            KeyCode::Esc => {
                self.form.app_state = AppState::Form;
                true
            }
            _ => true,
        }
    }

    fn handle_running_event(&mut self, key: KeyEvent) -> bool {
        self.sync_chat_limit();
        let was_g = self.g_pending;
        self.g_pending = false;

        if self.chat.pending_confirm().is_some() {
            let cont = self.handle_chat_confirm_event(key);
            self.sync_view_chat_state();
            return cont;
        }

        let cont = if self.chat.is_insert() {
            self.handle_running_insert_event(key)
        } else {
            self.handle_running_normal_event(key, was_g)
        };
        self.sync_view_chat_state();
        cont
    }

    /// 确认层按键：Enter 入队，Esc 取消，其它键忽略
    fn handle_chat_confirm_event(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => {
                if let Some(action) = self.chat.pending_confirm().cloned() {
                    self.chat.clear_confirm();
                    self.pending_actions.push(action);
                }
            }
            KeyCode::Esc => self.chat.clear_confirm(),
            _ => {}
        }
        true
    }

    fn handle_running_normal_event(&mut self, key: KeyEvent, was_g: bool) -> bool {
        let limit = self.message_list_limit();

        if self.keymap.is(KeyAction::SelectNext, key) {
            if let Some(limit) = limit {
                self.chat.select_next(limit);
            } else {
                self.view.scroll_by(1);
            }
            return true;
        }
        if self.keymap.is(KeyAction::SelectPrev, key) {
            if limit.is_some() {
                self.chat.select_prev();
            } else {
                self.view.scroll_by(-1);
            }
            return true;
        }
        if self.keymap.is(KeyAction::SelectTop, key) {
            if limit.is_some() {
                self.chat.select_top();
            } else {
                self.view.reset_scroll();
            }
            return true;
        }
        if self.keymap.is(KeyAction::SelectBottom, key) {
            if let Some(limit) = limit {
                self.chat.select_bottom(limit);
            }
            return true;
        }
        if self.keymap.is(KeyAction::HalfPageDown, key) {
            if let Some(limit) = limit {
                self.chat.half_page_down(limit);
            } else {
                self.view.scroll_by(10);
            }
            return true;
        }
        if self.keymap.is(KeyAction::HalfPageUp, key) {
            if limit.is_some() {
                self.chat.half_page_up();
            } else {
                self.view.scroll_by(-10);
            }
            return true;
        }
        if key.code == KeyCode::PageDown {
            self.view.scroll_by(10);
            return true;
        }
        if key.code == KeyCode::PageUp {
            self.view.scroll_by(-10);
            return true;
        }
        if self.keymap.is(KeyAction::Insert, key) {
            self.chat.enter_insert();
            return true;
        }
        if self.keymap.is(KeyAction::Send, key) {
            // Normal 模式不发送；发送必须经宿主 / 权限门
            return true;
        }
        if let Some(action) = self.normal_chat_action(key) {
            if action.requires_confirm() {
                self.chat.request_confirm(action);
            } else {
                self.pending_actions.push(action);
            }
            return true;
        }
        if self.keymap.is(KeyAction::Cancel, key) {
            self.cancel_running();
            return true;
        }

        // 兼容旧 `gg`：默认 keymap 的顶键是 Home，这里保留双 g 别名
        if key.code == KeyCode::Char('g') {
            if was_g {
                if limit.is_some() {
                    self.chat.select_top();
                } else {
                    self.view.reset_scroll();
                }
            } else {
                self.g_pending = true;
            }
            return true;
        }
        true
    }

    /// Normal 模式消息动作；仅入队 intent，不执行真实操作
    fn normal_chat_action(&self, key: KeyEvent) -> Option<ChatAction> {
        let selected = self.chat.selected();
        if self.keymap.matches(&["y".to_string()], key) {
            Some(ChatAction::Copy(selected))
        } else if self.keymap.matches(&["r".to_string()], key) {
            Some(ChatAction::Reply(selected))
        } else if self.keymap.matches(&["R".to_string()], key) {
            Some(ChatAction::React(selected))
        } else if self.keymap.matches(&["e".to_string()], key) {
            Some(ChatAction::Edit(selected))
        } else if self.keymap.matches(&["d".to_string()], key) {
            Some(ChatAction::Delete(selected))
        } else if self.keymap.matches(&["o".to_string()], key) {
            Some(ChatAction::Open(selected))
        } else {
            None
        }
    }

    /// 插入模式：普通字符入草稿，Esc 只退出插入不取消任务
    fn handle_running_insert_event(&mut self, key: KeyEvent) -> bool {
        if self.keymap.is(KeyAction::ExitInsert, key) {
            self.chat.exit_insert();
            return true;
        }
        if self.keymap.is(KeyAction::Send, key) {
            let draft = self.chat.take_draft();
            self.pending_actions.push(ChatAction::Send(draft));
            self.chat.exit_insert();
            return true;
        }
        if self.keymap.is(KeyAction::Cancel, key) && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.cancel_running();
            return true;
        }
        match key.code {
            KeyCode::Backspace => self.chat.backspace(),
            KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if ch == 'j' {
                    self.chat.newline();
                }
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::ALT) => {
                self.chat.push_char(ch);
            }
            _ => {}
        }
        true
    }

    fn cancel_running(&mut self) {
        self.form.app_state = AppState::Error;
        self.form.error_message = Some("用户取消".into());
    }

    fn handle_terminal_event(&mut self, _key: KeyEvent) -> bool {
        // Done / Error 状态下任意键：
        // 多命令模式 → 返回命令选择页（继续导航下一个命令）
        // 单命令模式 → 退出
        if self.commands.is_some() {
            if let Some(schemas) = &self.commands {
                let idx = self.selected_command.min(schemas.len().saturating_sub(1));
                self.form = FormRenderer::new(&schemas[idx]);
            }
            self.form.app_state = AppState::CommandSelect;
            self.active_command = None;
            self.view = ViewState::default();
            true
        } else {
            self.should_quit = true;
            false
        }
    }

    fn update_scroll(&mut self) {
        // 确保焦点字段在可见范围内
        let fi = self.form.focus_index as u16;
        if fi < self.form.scroll_offset {
            self.form.scroll_offset = fi;
        }
    }

    // ── 进度 API（Run trait 调用）─────────────────────

    /// 开始任务
    pub fn start_progress(&mut self, _total: Option<u64>, message: Option<String>) {
        self.form.progress_percent = None;
        self.form.elapsed_ms = 0;
        if let Some(msg) = message {
            self.form.progress_log.push(msg);
        }
    }

    /// 推进进度
    pub fn tick_progress(&mut self, current: u64, total: Option<u64>, message: Option<String>) {
        if let Some(t) = total {
            self.form.progress_percent = Some(current as f32 / t as f32);
        }
        if let Some(msg) = message {
            self.form.progress_log.push(msg);
        }
    }

    /// 日志
    pub fn log_progress(&mut self, level: &str, message: String) {
        self.form.progress_log.push(format!("[{level}] {message}"));
    }

    /// 标记完成
    pub fn finish_progress(&mut self, result: serde_json::Value, duration_ms: u64) {
        self.form.elapsed_ms = duration_ms;
        self.form.result = Some(result);
        self.form.app_state = AppState::Done;
    }

    /// 标记错误
    pub fn error_progress(&mut self, code: i32, message: String) {
        self.form.error_message = Some(format!("[E{code}] {message}"));
        self.form.app_state = AppState::Error;
    }

    /// 设置取消
    pub fn cancel(&mut self) {
        self.form.error_message = Some("已取消".into());
        self.form.app_state = AppState::Error;
    }
}

// ── 聊天视图辅助 ──────────────────────────────────────────

/// 递归查找第一个 MessageList 的消息条数
fn message_list_len(component: &Component) -> Option<usize> {
    match component {
        Component::MessageList { entries, .. } => Some(entries.len()),
        Component::Card { children, .. }
        | Component::Section { children, .. }
        | Component::ScrollArea { children, .. }
        | Component::AspectRatio { children, .. }
        | Component::Collapsible { children, .. }
        | Component::Tooltip { children, .. }
        | Component::Form { children, .. }
        | Component::Dialog { children, .. }
        | Component::Sheet { children, .. }
        | Component::Drawer { children, .. }
        | Component::Popover { children, .. }
        | Component::Pane { children, .. } => children.iter().find_map(message_list_len),
        Component::Resizable { start, end, .. } => {
            start.iter().chain(end.iter()).find_map(message_list_len)
        }
        Component::Tabs { items, .. } => items
            .iter()
            .find_map(|item| item.children.iter().find_map(message_list_len)),
        Component::Carousel { items, .. } => items
            .iter()
            .find_map(|item| item.children.iter().find_map(message_list_len)),
        _ => None,
    }
}

// ── 路径补全辅助 ──────────────────────────────────────────

/// 拆分路径输入为（目录前缀, 文件名前缀），兼容 `/` 与 `\` 分隔符
///
/// - `src/`   → ("src/", "")
/// - `src/ma` → ("src/", "ma")
/// - `ma`     → ("", "ma")
pub(crate) fn split_dir_prefix(text: &str) -> (String, String) {
    match text.rfind('/').max(text.rfind('\\')) {
        Some(pos) => (text[..=pos].to_string(), text[pos + 1..].to_string()),
        None => (String::new(), text.to_string()),
    }
}

// ── 帮助 overlay ──────────────────────────────────────────

fn render_help_overlay(area: Rect, buf: &mut Buffer) {
    let text = "\
Esc / F1         关闭帮助\n\
Tab / Shift+Tab  切换字段\n\
Alt+↑ / Alt+↓    上一项 / 下一项\n\
Tab              补全已输入的路径\n\
↑↓               调整数字\n\
←→               切换枚举选项\n\
空格             切换开关\n\
Enter            确认执行";
    renderer::render_popup(area, buf, " 快捷键 ", text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco_core::safety::SafetyTier;
    use lilyco_core::view::{ComposerMode, MessageEntry, Tone};

    fn schema() -> CommandSchema {
        CommandSchema {
            name: "chat".into(),
            about: "聊天".into(),
            args: vec![],
            subcommands: vec![],
            safety: SafetyTier::ReadOnly,
        }
    }

    fn running_app() -> TuiApp {
        let mut app = TuiApp::new(&schema());
        app.form.app_state = AppState::Running;
        app
    }

    fn entries(count: usize) -> Vec<MessageEntry> {
        (0..count)
            .map(|index| MessageEntry {
                author: "作者".into(),
                time: "12:00".into(),
                body: format!("第{index}条"),
                edited: false,
                reply_to: None,
                tone: Tone::Neutral,
                mine: false,
            })
            .collect()
    }

    fn composer(value: &str, mode: ComposerMode) -> Component {
        Component::Composer {
            value: value.into(),
            placeholder: "写点什么".into(),
            mode,
            attachments: vec![],
        }
    }

    fn message_list(count: usize, selected: Option<usize>) -> Component {
        Component::MessageList {
            entries: entries(count),
            selected,
            unread: None,
        }
    }

    #[test]
    fn chat_state_sync_writes_selection_and_draft() {
        let mut app = running_app();
        app.apply_view("messages", message_list(3, Some(0)));
        app.apply_view("composer", composer("", ComposerMode::Normal));

        app.chat.select_next(3);
        app.sync_view_chat_state();

        match app.view.get("messages") {
            Some(Component::MessageList { selected, .. }) => {
                assert_eq!(*selected, Some(1), "选中下标应写回 MessageList");
            }
            other => panic!("期望 MessageList，实际 {other:?}"),
        }
        match app.view.get("composer") {
            Some(Component::Composer { value, .. }) => {
                assert_eq!(value, app.chat.draft(), "Composer value 应等于草稿");
            }
            other => panic!("期望 Composer，实际 {other:?}"),
        }
    }

    #[test]
    fn insert_mode_draft_is_visible_in_composer() {
        let mut app = running_app();
        app.apply_view("composer", composer("", ComposerMode::Normal));

        app.chat.enter_insert();
        app.chat.push_char('你');
        app.chat.push_char('好');
        app.sync_view_chat_state();

        match app.view.get("composer") {
            Some(Component::Composer { value, mode, .. }) => {
                assert_eq!(*mode, ComposerMode::Insert);
                assert_eq!(value, "你好");
            }
            other => panic!("期望 Composer，实际 {other:?}"),
        }
    }

    #[test]
    fn chat_state_sync_reaches_nested_pane_children() {
        let mut app = running_app();
        app.apply_view(
            "chat",
            Component::Pane {
                id: "main".into(),
                title: "聊天".into(),
                active: true,
                children: vec![message_list(4, None), composer("", ComposerMode::Normal)],
            },
        );

        app.chat.select_next(4);
        app.chat.enter_insert();
        app.chat.push_char('草');
        app.sync_view_chat_state();

        match app.view.get("chat") {
            Some(Component::Pane { children, .. }) => {
                assert!(
                    matches!(
                        children.first(),
                        Some(Component::MessageList {
                            selected: Some(1),
                            ..
                        })
                    ),
                    "嵌套 MessageList 应同步选中"
                );
                assert!(
                    matches!(
                        children.get(1),
                        Some(Component::Composer {
                            value,
                            mode: ComposerMode::Insert,
                            ..
                        }) if value == "草"
                    ),
                    "嵌套 Composer 应同步草稿与模式"
                );
            }
            other => panic!("期望 Pane，实际 {other:?}"),
        }
    }

    #[test]
    fn running_j_selects_nested_message_list() {
        let mut app = running_app();
        app.apply_view(
            "chat",
            Component::Pane {
                id: "main".into(),
                title: "聊天".into(),
                active: true,
                children: vec![message_list(4, None)],
            },
        );

        app.handle_event(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

        assert_eq!(app.chat.selected(), 1, "嵌套消息列表也应响应 j");
        match app.view.get("chat") {
            Some(Component::Pane { children, .. }) => {
                assert!(
                    matches!(
                        children.first(),
                        Some(Component::MessageList {
                            selected: Some(1),
                            ..
                        })
                    ),
                    "嵌套 MessageList 的 selected 应更新"
                );
            }
            other => panic!("期望 Pane，实际 {other:?}"),
        }
    }

    #[test]
    fn enter_in_insert_mode_queues_send_and_clears_draft() {
        let mut app = running_app();
        app.apply_view("composer", composer("", ComposerMode::Normal));

        app.handle_event(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.handle_event(KeyEvent::new(KeyCode::Char('你'), KeyModifiers::NONE));
        app.handle_event(KeyEvent::new(KeyCode::Char('好'), KeyModifiers::NONE));
        app.handle_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            app.pending_actions,
            vec![ChatAction::Send("你好".into())],
            "Enter 应只入队 Send intent"
        );
        assert_eq!(app.chat.draft(), "", "发送后草稿应清空");
        assert!(!app.chat.is_insert(), "发送后退出插入模式");
        assert_eq!(*app.state(), AppState::Running, "发送不改变运行状态");
        match app.view.get("composer") {
            Some(Component::Composer { value, mode, .. }) => {
                assert_eq!(value, "");
                assert_eq!(*mode, ComposerMode::Normal);
            }
            other => panic!("期望 Composer，实际 {other:?}"),
        }
    }

    #[test]
    fn normal_mode_actions_queue_selected_index() {
        let mut app = running_app();
        app.apply_view("messages", message_list(3, None));
        app.chat.select_next(3);
        app.chat.select_next(3);
        assert_eq!(app.chat.selected(), 2);

        for key in [
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE),
        ] {
            app.handle_event(key);
        }

        assert_eq!(
            app.pending_actions,
            vec![ChatAction::Copy(2), ChatAction::Open(2)],
            "复制 / 打开直接入队"
        );
        assert!(app.chat.pending_confirm().is_none());
        assert_eq!(app.chat.selected(), 2, "动作不移动选择");
    }

    #[test]
    fn destructive_action_requires_confirmation() {
        let mut app = running_app();
        app.apply_view("messages", message_list(3, None));
        app.chat.select_next(3);
        app.chat.select_next(3);

        app.handle_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert_eq!(
            app.chat.pending_confirm().cloned(),
            Some(ChatAction::Delete(2))
        );
        assert!(app.drain_actions().is_empty(), "确认前不应入队");
    }

    #[test]
    fn confirm_enter_queues_and_clears() {
        let mut app = running_app();
        app.apply_view("messages", message_list(3, None));
        app.chat.select_next(3);
        app.chat.select_next(3);
        app.handle_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        app.handle_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(app.chat.pending_confirm().is_none());
        assert_eq!(app.drain_actions(), vec![ChatAction::Delete(2)]);
        assert_eq!(*app.state(), AppState::Running);
    }

    #[test]
    fn confirm_esc_cancels_without_intent() {
        let mut app = running_app();
        app.apply_view("messages", message_list(3, None));
        app.chat.select_next(3);
        app.handle_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        app.handle_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.chat.pending_confirm().is_none());
        assert!(app.drain_actions().is_empty(), "Esc 不应产生 intent");
        assert_eq!(*app.state(), AppState::Running, "Esc 只取消确认");
    }

    #[test]
    fn copy_and_open_skip_confirmation() {
        let mut app = running_app();
        app.apply_view("messages", message_list(3, None));
        app.chat.select_next(3);
        app.handle_event(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        app.handle_event(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));

        assert!(app.chat.pending_confirm().is_none());
        assert_eq!(
            app.drain_actions(),
            vec![ChatAction::Copy(1), ChatAction::Open(1)]
        );
    }

    #[test]
    fn drain_actions_takes_and_clears() {
        let mut app = running_app();
        app.pending_actions.push(ChatAction::Send("hi".into()));
        app.pending_actions.push(ChatAction::Delete(1));

        let taken = app.drain_actions();
        assert_eq!(
            taken,
            vec![ChatAction::Send("hi".into()), ChatAction::Delete(1)]
        );
        assert!(app.pending_actions.is_empty());
        assert!(app.drain_actions().is_empty());
    }
}
