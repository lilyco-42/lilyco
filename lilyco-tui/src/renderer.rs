use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Clear, Gauge, Padding, Paragraph, Widget, Wrap};

use lilyco_core::prelude::*;
use lilyco_core::schema::CommandSchema;

use crate::chat::ChatAction;
use crate::theme::Theme;
use crate::view::ViewState;
use crate::widgets::{FieldValue, FormField};

// ── 表单渲染器 ─────────────────────────────────────────────

/// 表单渲染器：管理 CommandSchema 对应的表单状态
pub struct FormRenderer {
    /// 表单字段列表
    pub fields: Vec<FormField>,
    /// 当前焦点索引
    pub focus_index: usize,
    /// 当前滚动偏移
    pub scroll_offset: u16,
    /// 命令名
    pub command_name: String,
    /// 命令描述
    pub command_about: String,
    /// 进度事件日志（Running 状态）
    pub progress_log: Vec<String>,
    /// 进度条百分比 (0.0 ~ 1.0)
    pub progress_percent: Option<f32>,
    /// 启动时间（毫秒）
    pub elapsed_ms: u64,
    /// 当前状态
    pub app_state: AppState,
    /// 结果 JSON（Done 状态）
    pub result: Option<serde_json::Value>,
    /// 错误信息（Error 状态）
    pub error_message: Option<String>,
    /// 表单校验消息（Form 状态下提交被拦截时显示，值变更即清除）
    pub validation_message: Option<String>,
}

/// 应用状态机
#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    /// 多命令选择页（`TuiApp::new_multi` 起始状态）
    CommandSelect,
    Form,
    Confirm,
    Running,
    Done,
    Error,
}

impl FormRenderer {
    /// 从 CommandSchema 创建
    pub fn new(schema: &CommandSchema) -> Self {
        let fields: Vec<FormField> = schema.args.iter().map(FormField::from_schema).collect();
        Self {
            fields,
            focus_index: 0,
            scroll_offset: 0,
            command_name: schema.name.clone(),
            command_about: schema.about.clone(),
            progress_log: Vec::new(),
            progress_percent: None,
            elapsed_ms: 0,
            app_state: AppState::Form,
            result: None,
            error_message: None,
            validation_message: None,
        }
    }

    /// Tab 到下一个字段
    pub fn next_field(&mut self) {
        if !self.fields.is_empty() {
            self.focus_index = (self.focus_index + 1) % self.fields.len();
        }
    }

    /// Shift-Tab 到上一个字段
    pub fn prev_field(&mut self) {
        if !self.fields.is_empty() {
            self.focus_index = (self.focus_index + self.fields.len() - 1) % self.fields.len();
        }
    }

    /// 获取当前焦点字段（可变引用）
    pub fn focused_field_mut(&mut self) -> Option<&mut FormField> {
        self.fields.get_mut(self.focus_index)
    }

    /// 获取当前焦点字段（不可变引用）
    pub fn focused_field(&self) -> Option<&FormField> {
        self.fields.get(self.focus_index)
    }

    /// 生成 CLI 预览字符串
    pub fn cli_preview(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(self.command_name.clone());

        for field in &self.fields {
            if let Some(arg) = field.to_cli_arg() {
                parts.push(arg);
            }
        }

        parts.join(" ")
    }

    /// 检查所有 required 字段是否已填写
    pub fn all_required_filled(&self) -> bool {
        self.fields.iter().all(|f| {
            if !f.required {
                return true;
            }
            match &f.value {
                crate::widgets::FieldValue::Flag(_) => true, // flag 总有效
                crate::widgets::FieldValue::Text(v) => !v.is_empty(),
                crate::widgets::FieldValue::Number(_) => true,
                crate::widgets::FieldValue::Enum { .. } => true,
                crate::widgets::FieldValue::Path(v) => !v.is_empty(),
                crate::widgets::FieldValue::List { values, .. } => !values.is_empty(),
            }
        })
    }

    /// 全量校验当前表单，返回所有错误消息（空 = 可提交）
    ///
    /// 覆盖 required / Number 范围 / Path must_exist（与 CLI clap 校验语义一致）。
    pub fn validation_errors(&self) -> Vec<String> {
        self.fields.iter().filter_map(|f| f.validate()).collect()
    }
}

// ── 渲染 ──────────────────────────────────────────────────

/// 对整个表单做一次完整渲染到 Buffer
///
/// 布局：
/// ┌────────────── 顶部标题栏 ──────────────────────────┐
/// │ 命令行预览                                         │
/// ├────────────── 参数表单（可滚动）────────────────────┤
/// │  [*] name:  widget                                 │
/// │  [ ] name:  widget                                 │
/// │  ...                                               │
/// ├────────────── 底部固定 ────────────────────────────┤
/// │ $ cli-preview-string                               │
/// │ [Tab]切换 [Enter]确认 [Esc]退出 [F1]帮助            │
/// └────────────────────────────────────────────────────┘
pub fn render_form(fr: &FormRenderer, area: Rect, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    let header_rows = if area.height >= 8 { 3 } else { 1 };
    let footer_rows = u16::from(area.height > 2) + u16::from(area.height > 3);
    let visible = area.height.saturating_sub(header_rows + footer_rows);
    render_title(buf, area, &fr.command_name);
    if header_rows > 1 {
        write_row(buf, area, 1, &format!(" {}", fr.command_about), muted());
        write_row(buf, area, 2, &format!(" $ {}", fr.cli_preview()), muted());
    }

    let start = visible_start(
        fr.fields.len(),
        fr.focus_index,
        fr.scroll_offset.into(),
        visible,
    );
    for (row, field) in fr
        .fields
        .iter()
        .skip(start)
        .take(usize::from(visible))
        .enumerate()
    {
        field.render(
            Rect::new(area.x, area.y + header_rows + row as u16, area.width, 1),
            buf,
            start + row == fr.focus_index,
        );
    }
    let hint = match fr.focused_field().map(|field| &field.value) {
        Some(FieldValue::Path(path)) if !path.is_empty() => "Tab 补全 · Alt+↓ 下一项",
        Some(FieldValue::Number(_)) => "↑↓ 调数值",
        Some(FieldValue::Enum { .. }) => "←→ 切选项",
        Some(FieldValue::Flag(_)) => "空格 切换",
        _ => "",
    };
    if footer_rows == 2 {
        let (message, style) = if let Some(message) = &fr.validation_message {
            (format!(" ⚠ {message}"), Style::default().fg(Color::Red))
        } else if fr.fields.is_empty() {
            (" 无参数".into(), muted())
        } else {
            (
                format!(" {}/{}  {hint}", fr.focus_index + 1, fr.fields.len()),
                muted(),
            )
        };
        write_row(buf, area, area.height - 2, &message, style);
    }
    if footer_rows > 0 {
        let shortcuts = if area.width >= 57 {
            " Enter 确认 · Tab 切换 · F1 帮助 · Esc 退出"
        } else {
            " Enter · Tab · F1 · Esc"
        };
        write_row(buf, area, area.height - 1, shortcuts, muted());
    }
}

/// 渲染多命令选择页，选中条目始终留在可见区域。
pub fn render_command_select(
    app_name: &str,
    schemas: &[CommandSchema],
    selected: usize,
    area: Rect,
    buf: &mut Buffer,
) {
    if area.is_empty() {
        return;
    }
    render_title(buf, area, &format!("{app_name}  /  选择命令"));
    let visible = area.height.saturating_sub(2);
    let start = visible_start(schemas.len(), selected, 0, visible);
    for (row, schema) in schemas
        .iter()
        .skip(start)
        .take(usize::from(visible))
        .enumerate()
    {
        let focused = start + row == selected;
        let marker = if focused { "›" } else { " " };
        let style = if focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let line = format!("{marker} {}  {}", schema.name, schema.about);
        write_row(buf, area, row as u16 + 1, &line, style);
    }
    if area.height > 1 {
        write_row(
            buf,
            area,
            area.height - 1,
            " ↑↓ 选择 · Enter · Esc",
            muted(),
        );
    }
}

fn visible_start(len: usize, focus: usize, offset: usize, visible: u16) -> usize {
    let visible = usize::from(visible);
    offset
        .min(focus)
        .max(focus.saturating_add(1).saturating_sub(visible))
        .min(len.saturating_sub(visible))
}

fn muted() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn write_row(buf: &mut Buffer, area: Rect, row: u16, text: &str, style: Style) {
    if row < area.height {
        buf.set_stringn(area.x, area.y + row, text, usize::from(area.width), style);
    }
}

fn render_title(buf: &mut Buffer, area: Rect, title: &str) {
    let style = Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED);
    if !area.is_empty() {
        buf.set_style(Rect { height: 1, ..area }, style);
        write_row(buf, area, 0, &format!(" {title}"), style);
    }
}

pub(crate) fn render_popup(area: Rect, buf: &mut Buffer, title: &str, content: &str) {
    if area.is_empty() {
        return;
    }
    let width = 64.min(area.width);
    let height = (content.lines().count() as u16 + 4).min(area.height);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let block = Block::bordered()
        .title(title)
        .padding(Padding::horizontal(1))
        .border_style(Style::default().fg(Color::Cyan));
    Clear.render(popup, buf);
    Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false })
        .render(popup, buf);
}

/// 渲染确认对话框 overlay
pub fn render_confirm(fr: &FormRenderer, area: Rect, buf: &mut Buffer) {
    let content = format!("Enter 执行 · Esc 返回\n\n$ {}", fr.cli_preview());
    render_popup(area, buf, " 确认执行 ", &content);
}

/// 渲染 Running 状态
pub fn render_running(fr: &FormRenderer, area: Rect, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    render_title(buf, area, &format!("运行中  /  {}", fr.command_name));
    if area.height > 2 {
        let progress = Rect::new(area.x, area.y + 1, area.width, 1);
        if let Some(percent) = fr.progress_percent.filter(|value| value.is_finite()) {
            Gauge::default()
                .ratio(f64::from(percent).clamp(0.0, 1.0))
                .gauge_style(Style::default().fg(Color::Cyan))
                .render(progress, buf);
        } else {
            Paragraph::new(" 正在处理…")
                .style(muted())
                .render(progress, buf);
        }
        let logs = Rect::new(area.x, area.y + 2, area.width, area.height - 3);
        let start = fr
            .progress_log
            .len()
            .saturating_sub(usize::from(logs.height));
        for (row, message) in fr.progress_log.iter().skip(start).enumerate() {
            write_row(buf, logs, row as u16, message, Style::default());
        }
    }
    if area.height > 1 {
        write_row(buf, area, area.height - 1, " Ctrl+C / Esc 取消", muted());
    }
}

/// 渲染 Running 状态与语义视图：有视图时上半区交给 `ViewState`，底部保留进度与取消提示
pub fn render_running_with_view(
    fr: &FormRenderer,
    view: &ViewState,
    pending: usize,
    confirm: Option<&ChatAction>,
    theme: &Theme,
    area: Rect,
    buf: &mut Buffer,
) {
    if area.is_empty() {
        return;
    }
    render_title(buf, area, &format!("运行中  /  {}", fr.command_name));
    if area.height <= 1 {
        return;
    }
    let content = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(2),
    );
    view.render(content, buf, theme);
    let mut footer = String::new();
    if let Some(percent) = fr.progress_percent.filter(|value| value.is_finite()) {
        footer.push_str(&format!(
            " {:>3}%",
            (f64::from(percent) * 100.0).round() as u16
        ));
    }
    if view.needs_scrollbar(content.width, content.height) {
        footer.push_str(" · j/k 滚动");
    }
    if pending > 0 {
        footer.push_str(&format!(" · 待发送 {pending}"));
    }
    footer.push_str(" · Ctrl+C / Esc 取消");
    write_row(buf, area, area.height - 1, &footer, muted());
    render_confirm_overlay(confirm, theme, area, buf);
}

/// 在视口中央绘制写操作确认层
fn render_confirm_overlay(
    confirm: Option<&ChatAction>,
    theme: &Theme,
    area: Rect,
    buf: &mut Buffer,
) {
    let Some(action) = confirm else {
        return;
    };
    let width = area.width.saturating_sub(4).min(48);
    if width == 0 || area.height == 0 {
        return;
    }
    let height = 5.min(area.height);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    Clear.render(popup, buf);
    let block = Block::bordered()
        .title("确认操作")
        .border_style(theme.warn_style())
        .title_style(theme.warn_style());
    let inner = block.inner(popup);
    block.render(popup, buf);
    if inner.height > 0 {
        write_row(buf, inner, 0, &action.confirm_text(), theme.text_style());
    }
    if inner.height > 1 {
        write_row(
            buf,
            inner,
            1,
            "[Enter] 确认  [Esc] 取消",
            theme.muted_style(),
        );
    }
}

pub fn render_done(fr: &FormRenderer, area: Rect, buf: &mut Buffer) {
    let data = fr.result.as_ref().unwrap_or(&serde_json::Value::Null);
    let result = serde_json::to_string_pretty(data).unwrap_or_default();
    render_outcome(area, buf, &format!("完成 · {} ms", fr.elapsed_ms), &result);
}

pub fn render_error(fr: &FormRenderer, area: Rect, buf: &mut Buffer) {
    render_outcome(
        area,
        buf,
        "运行错误",
        fr.error_message.as_deref().unwrap_or("未知错误"),
    );
}

fn render_outcome(area: Rect, buf: &mut Buffer, title: &str, details: &str) {
    render_title(buf, area, title);
    if area.is_empty() || area.height < 2 {
        return;
    }
    let content = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    Paragraph::new(details)
        .wrap(Wrap { trim: false })
        .render(content, buf);
    write_row(buf, area, area.height - 1, " Enter 继续", muted());
}

// ── TuiRenderer ────────────────────────────────────────────

/// TUI 渲染器
///
/// 实现 `Renderer` trait，`Output = FormRenderer` 即创建好表单的完整状态。
#[derive(Debug, Clone, Default)]
pub struct TuiRenderer;

impl Renderer for TuiRenderer {
    type Output = FormRenderer;

    fn render(&self, schema: &CommandSchema) -> Self::Output {
        FormRenderer::new(schema)
    }
}
