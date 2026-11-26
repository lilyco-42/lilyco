use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use lilyco_core::schema::CommandSchema;

use crate::renderer::{self, AppState, FormRenderer};
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
            AppState::Form | AppState::Running => {
                renderer::render_form(&self.form, area, buf);
                if self.show_help {
                    render_help_overlay(area, buf);
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
                }
            }
            _ => {}
        }
        true
    }

    fn handle_form_event(&mut self, key: KeyEvent) -> bool {
        use KeyCode::*;

        // 全局快捷键优先
        match key.code {
            Esc => {
                self.should_quit = true;
                return false;
            }
            Tab => {
                if self.show_help {
                    self.show_help = false;
                } else {
                    // Path 字段聚焦且已有输入 → Tab 用于目录补全（readline 风格，
                    // 重复 Tab 循环候选）；空值时照常切换字段，保证前进导航可达
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
                }
                return true;
            }
            BackTab => {
                self.form.prev_field();
                return true;
            }
            Enter => {
                if self.show_help {
                    self.show_help = false;
                    return true;
                }
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
        // 取消：Ctrl-C、c / C、q / Q、Esc
        // 注意：取消只改变 UI 状态为 Error；真正的中断由 facade 通过
        // executor::Task.cancel 请求，handler 读取 ctx.is_cancelled() 优雅退出。
        let is_ctrl_c =
            key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
        let is_cancel = is_ctrl_c
            || matches!(
                key.code,
                KeyCode::Char('c')
                    | KeyCode::Char('C')
                    | KeyCode::Char('q')
                    | KeyCode::Char('Q')
                    | KeyCode::Esc
            );
        if is_cancel {
            self.form.app_state = AppState::Error;
            self.form.error_message = Some("用户取消".into());
        }
        true
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
    use ratatui::style::{Color, Style};

    let text = "\
        快捷键帮助

        Tab / Shift+Tab      切换字段焦点
        Tab(Path 输入中)     路径补全 / 循环候选
        ↑↓                   数字加减
        ←→                   Enum 选项切换
        空格                  Flag 切换
        Enter                 确认执行
        Esc                   退出
        F1                    关闭帮助
    ";
    let w = 36u16;
    let h = 13u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let dark = Style::default().bg(Color::DarkGray);

    for dy in 0..h {
        buf.set_string(x, y + dy, &" ".repeat(w as usize), dark);
    }
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            buf.set_string(
                x + 2,
                y + 1 + i as u16,
                trimmed,
                Style::default().fg(Color::White).bg(Color::DarkGray),
            );
        }
    }
}
