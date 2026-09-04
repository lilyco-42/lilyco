use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use lilyco_core::schema::CommandSchema;

use crate::renderer::{self, AppState, FormRenderer};
use crate::widgets::FormField;

// ── TuiApp ────────────────────────────────────────────────

/// TUI 应用的完整状态
pub struct TuiApp {
    pub form: FormRenderer,
    /// 是否应退出
    pub should_quit: bool,
    /// 帮助面板是否显示
    pub show_help: bool,
}

impl TuiApp {
    /// 从 CommandSchema 创建
    pub fn new(schema: &CommandSchema) -> Self {
        Self {
            form: FormRenderer::new(schema),
            should_quit: false,
            show_help: false,
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
            AppState::Form => self.handle_form_event(key),
            AppState::Confirm => self.handle_confirm_event(key),
            AppState::Running => self.handle_running_event(key),
            AppState::Done | AppState::Error => self.handle_terminal_event(key),
        }
    }

    /// 渲染当前状态到 Buffer
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        match self.form.app_state {
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
                    self.form.next_field();
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
                // 值变更后旧校验消息失效，清除
                self.form.validation_message = None;
                // 更新滚动位置
                self.update_scroll();
            }
        }

        true
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
        // Done / Error 状态下任意键退出
        self.should_quit = true;
        false
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

// ── 帮助 overlay ──────────────────────────────────────────

fn render_help_overlay(area: Rect, buf: &mut Buffer) {
    use ratatui::style::{Color, Style};

    let text = "\
        快捷键帮助

        Tab / Shift+Tab      切换字段焦点
        ↑↓                   数字加减
        ←→                   Enum 选项切换
        空格                  Flag 切换
        Enter                 确认执行
        Esc                   退出
        F1                    关闭帮助
    ";
    let w = 36u16;
    let h = 12u16;
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
