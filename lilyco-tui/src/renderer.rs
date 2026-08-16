use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Gauge, Widget};

use lilyco_core::prelude::*;
use lilyco_core::schema::CommandSchema;

use crate::widgets::FormField;

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
}

/// 应用状态机
#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
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
    let header_h = 3u16;
    let footer_h = 3u16;
    let form_h = area.height.saturating_sub(header_h + footer_h);

    // ── 顶部标题栏 ──
    let header = Rect {
        y: area.y,
        height: header_h,
        ..area
    };
    let title = format!(" {} — {} ", fr.command_name, fr.command_about);
    let title_style = Style::default().fg(Color::Black).bg(Color::Cyan);
    buf.set_string(header.x, header.y, &title, title_style);

    // CLI 预览（标题下方）
    let preview = format!(" $ {} ", fr.cli_preview());
    let preview_area = Rect {
        y: header.y + 1,
        height: 1,
        ..area
    };
    buf.set_string(
        preview_area.x,
        preview_area.y,
        &preview,
        Style::default().fg(Color::Green),
    );

    // 分隔线
    let sep_y = header.y + 2;
    let sep = "─".repeat(area.width as usize);
    buf.set_string(area.x, sep_y, &sep, Style::default().fg(Color::DarkGray));

    // ── 表单区域 ──
    let form_area = Rect {
        y: sep_y + 1,
        height: form_h,
        ..area
    };

    let visible_count = form_area.height as usize;
    let start = fr.scroll_offset as usize;

    for (idx, field) in fr.fields.iter().enumerate().skip(start).take(visible_count) {
        let row_y = form_area.y + (idx - start) as u16;
        if row_y >= form_area.y + form_area.height {
            break;
        }
        let field_area = Rect {
            y: row_y,
            height: 1,
            x: form_area.x,
            width: form_area.width,
        };
        let focused = idx == fr.focus_index;
        field.render(field_area, buf, focused);
    }

    // ── 底部快捷键 ──
    let footer_y = area.y + area.height - footer_h;
    let shortcuts = match fr.app_state {
        AppState::Form => "[Tab] 切换  [Enter] 确认  [Esc] 退出  [F1] 帮助",
        AppState::Confirm => "[Enter] 确认执行  [Esc] 返回",
        AppState::Running => "[Ctrl-C] 取消",
        AppState::Done => "[Enter] 退出",
        AppState::Error => "[任意键] 返回",
    };
    buf.set_string(
        area.x,
        footer_y + 1,
        shortcuts,
        Style::default().fg(Color::DarkGray),
    );
}

/// 渲染确认对话框 overlay
pub fn render_confirm(fr: &FormRenderer, area: Rect, buf: &mut Buffer) {
    let preview = fr.cli_preview();
    let msg = format!(" 确认执行？\n\n 命令: {preview}\n\n [Enter] 确认  [Esc] 取消 ");

    // 居中弹窗
    let w = 60u16.min(area.width);
    let h = 6u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let popup = Rect {
        x,
        y,
        width: w,
        height: h,
    };

    // 背景
    for dy in 0..h {
        let line = " ".repeat(w as usize);
        buf.set_string(
            popup.x,
            popup.y + dy,
            &line,
            Style::default().bg(Color::DarkGray),
        );
    }

    for (i, line) in msg.lines().enumerate() {
        if i < h as usize {
            buf.set_string(
                popup.x + 2,
                popup.y + i as u16,
                line,
                Style::default().fg(Color::White).bg(Color::DarkGray),
            );
        }
    }
}

/// 渲染 Running 状态
pub fn render_running(fr: &FormRenderer, area: Rect, buf: &mut Buffer) {
    // 进度条
    let gauge_y = area.y;
    let pct = fr.progress_percent.unwrap_or(0.0);
    let gauge = Gauge::default()
        .ratio(f64::from(pct).clamp(0.0, 1.0))
        .label(format!("{:.0}%", pct * 100.0));
    gauge.render(
        Rect {
            y: gauge_y,
            height: 3,
            ..area
        },
        buf,
    );

    // 滚动日志
    let log_start = gauge_y + 4;
    let log_area = Rect {
        y: log_start,
        height: area.height.saturating_sub(log_start + 3),
        ..area
    };
    let visible_logs = log_area.height as usize;
    let logs = &fr.progress_log;
    let skip = if logs.len() > visible_logs {
        logs.len() - visible_logs
    } else {
        0
    };
    for (i, msg) in logs.iter().skip(skip).enumerate() {
        buf.set_string(
            log_area.x,
            log_area.y + i as u16,
            msg,
            Style::default().fg(Color::White),
        );
    }

    // 底部：已用时间
    let footer_y = area.y + area.height - 2;
    buf.set_string(
        area.x,
        footer_y,
        &format!(" 已用时间: {}s  [Ctrl-C] 取消 ", fr.elapsed_ms / 1000),
        Style::default().fg(Color::DarkGray),
    );
}

/// 渲染 Done 状态
pub fn render_done(fr: &FormRenderer, area: Rect, buf: &mut Buffer) {
    let text = format!(
        " ✓ 任务完成！\n\n 耗时: {}ms\n 结果: {}",
        fr.elapsed_ms,
        serde_json::to_string_pretty(fr.result.as_ref().unwrap_or(&serde_json::Value::Null))
            .unwrap_or_default()
    );
    for (i, line) in text.lines().enumerate() {
        buf.set_string(
            area.x + 2,
            area.y + 2 + i as u16,
            line,
            Style::default().fg(Color::Green),
        );
    }
}

/// 渲染 Error 状态
pub fn render_error(fr: &FormRenderer, area: Rect, buf: &mut Buffer) {
    let text = format!(
        " ✗ 错误\n\n {}",
        fr.error_message.as_deref().unwrap_or("未知错误")
    );
    for (i, line) in text.lines().enumerate() {
        buf.set_string(
            area.x + 2,
            area.y + 2 + i as u16,
            line,
            Style::default().fg(Color::Red),
        );
    }
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
