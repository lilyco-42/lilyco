//! Component → 终端渲染（Lilyco TUI 适配器）
//!
//! 遵循 `lilyco-core::theme` 的适配器契约：只消费语义组件，未原生支持的
//! kind 走 `FallbackStrategy::DegradeToText`，降级为一行可读文本，永不 panic。

use lilyco_core::progress::LogLevel;
use lilyco_core::theme::{AdapterCapabilities, FallbackStrategy, UI_PROTOCOL_VERSION};
use lilyco_core::view::{ButtonVariant, Component, ComposerMode, LogLine, MessageEntry, Tone};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, Paragraph, Sparkline, Tabs, Widget, Wrap};

use crate::theme::{color, Theme};

/// 适配器标识
pub const ADAPTER_ID: &str = "lilyco-tui";

/// 本适配器的能力声明（框架协议要求的显式契约）
pub fn adapter_capabilities() -> AdapterCapabilities {
    AdapterCapabilities {
        adapter_id: ADAPTER_ID.into(),
        framework_name: "ratatui".into(),
        protocol_version: UI_PROTOCOL_VERSION.into(),
        supports_dark_mode: true,
        supports_density: false,
        fallback: FallbackStrategy::DegradeToText,
        supported_kinds: supported_kinds(),
    }
}

/// 已原生渲染的组件 kind 列表
pub fn supported_kinds() -> Vec<String> {
    [
        "card",
        "section",
        "separator",
        "scroll_area",
        "aspect_ratio",
        "collapsible",
        "accordion",
        "tabs",
        "breadcrumb",
        "pagination",
        "sidebar",
        "resizable",
        "carousel",
        "progress",
        "badge",
        "alert",
        "toast",
        "skeleton",
        "avatar",
        "key_value",
        "table",
        "chart",
        "log",
        "result",
        "button",
        "toggle",
        "toggle_group",
        "tooltip",
        "hover_card",
        "input",
        "textarea",
        "select",
        "checkbox",
        "radio_group",
        "switch",
        "slider",
        "combobox",
        "input_otp",
        "form",
        "label",
        "dialog",
        "sheet",
        "drawer",
        "alert_dialog",
        "popover",
        "dropdown_menu",
        "context_menu",
        "menubar",
        "command",
        "navigation_menu",
        "message_list",
        "composer",
        "pane",
    ]
    .iter()
    .map(|kind| (*kind).to_string())
    .collect()
}

// ── 尺寸计算 ─────────────────────────────────────────────

/// 计算组件在给定宽度下需要的行数
pub fn measure(component: &Component, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    match component {
        Component::Card {
            description,
            children,
            ..
        } => {
            let inner = width.saturating_sub(2);
            2u16.saturating_add(
                description
                    .as_deref()
                    .map_or(0, |text| text_height(text, inner)),
            )
            .saturating_add(children_height(children, inner))
        }
        Component::Section { title, children } => {
            u16::from(title.is_some()).saturating_add(children_height(children, width))
        }
        Component::Separator { .. } => 1,
        Component::ScrollArea { children, .. } => children_height(children, width),
        Component::AspectRatio { children, .. } => children_height(children, width),
        Component::Collapsible { open, children, .. } => {
            let body = if *open {
                children_height(children, width)
            } else {
                0
            };
            1u16.saturating_add(body)
        }
        Component::Accordion { items } => items.iter().fold(0u16, |acc, item| {
            let body = if item.open {
                text_height(&item.body, width)
            } else {
                0
            };
            acc.saturating_add(1).saturating_add(body)
        }),
        Component::Tabs { items, active } => {
            let index = tab_index(items, active);
            let body = items
                .get(index)
                .map_or(0, |item| children_height(&item.children, width));
            1u16.saturating_add(body)
        }
        Component::Breadcrumb { .. }
        | Component::Pagination { .. }
        | Component::Badge { .. }
        | Component::Toast { .. }
        | Component::Avatar { .. }
        | Component::Button { .. }
        | Component::Toggle { .. }
        | Component::ToggleGroup { .. }
        | Component::HoverCard { .. }
        | Component::Input { .. }
        | Component::Select { .. }
        | Component::Checkbox { .. }
        | Component::RadioGroup { .. }
        | Component::Switch { .. }
        | Component::Slider { .. }
        | Component::Combobox { .. }
        | Component::InputOtp { .. }
        | Component::Label { .. }
        | Component::NavigationMenu { .. }
        | Component::DatePicker { .. } => 1,
        Component::Sidebar { items, .. } => 2u16.saturating_add(items.len() as u16),
        Component::Resizable { ratio, start, end } => {
            let left = ratio_percent(*ratio);
            let left_width = percent_width(width, left);
            let right_width = width.saturating_sub(left_width);
            children_height(start, left_width).max(children_height(end, right_width))
        }
        Component::Carousel { index, items } => {
            let current = items.get(*index as usize);
            let body = current.map_or(0, |item| children_height(&item.children, width));
            u16::from(current.is_some()).saturating_add(body)
        }
        Component::Progress { label, .. } => {
            if label.is_some() {
                2
            } else {
                1
            }
        }
        Component::Alert { description, .. } => 2u16.saturating_add(
            description
                .as_deref()
                .map_or(0, |text| text_height(text, width.saturating_sub(2))),
        ),
        Component::Skeleton { lines } => u16::try_from((*lines).max(1)).unwrap_or(u16::MAX),
        Component::KeyValue { items } => items.len().max(1) as u16,
        Component::Table { caption, rows, .. } => u16::from(caption.is_some())
            .saturating_add(1)
            .saturating_add(rows.len() as u16),
        Component::Chart { .. } => 1,
        Component::Log { lines } => lines.len().max(1) as u16,
        Component::Result { value, .. } => 1u16.saturating_add(
            value
                .as_ref()
                .map_or(0, |data| text_height(&pretty_json(data), width)),
        ),
        Component::Tooltip { children, text } => {
            children_height(children, width)
                .max(1)
                .min(if children.is_empty() {
                    text_height(text, width)
                } else {
                    u16::MAX
                })
        }
        Component::Textarea { rows, .. } => {
            1u16.saturating_add(u16::try_from(*rows).unwrap_or(u16::MAX))
        }
        Component::Calendar { .. } => 1,
        Component::Form { children, submit } => {
            children_height(children, width).saturating_add(u16::from(submit.is_some()))
        }
        Component::Dialog { children, .. }
        | Component::Sheet { children, .. }
        | Component::Drawer { children, .. } => {
            2u16.saturating_add(children_height(children, width))
        }
        Component::AlertDialog { description, .. } => 3u16.saturating_add(
            description
                .as_deref()
                .map_or(0, |text| text_height(text, width.saturating_sub(2))),
        ),
        Component::Popover { children, .. } => {
            1u16.saturating_add(children_height(children, width))
        }
        Component::DropdownMenu { items, .. } => 1u16.saturating_add(items.len() as u16),
        Component::ContextMenu { items } => items.len().max(1) as u16,
        Component::Menubar { menus } => menus.len().max(1) as u16,
        Component::Command { query, items } => {
            u16::from(query.is_some()).saturating_add(items.len().max(1) as u16)
        }
        Component::MessageList { entries, .. } => entries.iter().fold(0u16, |acc, entry| {
            let body =
                u16::try_from(wrap_body(&entry.body, width).len().max(1)).unwrap_or(u16::MAX);
            acc.saturating_add(1).saturating_add(body).saturating_add(1)
        }),
        Component::Composer { attachments, .. } => {
            1u16.saturating_add(u16::try_from(attachments.len()).unwrap_or(u16::MAX))
        }
        Component::Pane { children, .. } => {
            2u16.saturating_add(children_height(children, width.saturating_sub(2)))
        }
    }
}

fn children_height(children: &[Component], width: u16) -> u16 {
    children
        .iter()
        .fold(0u16, |acc, child| acc.saturating_add(measure(child, width)))
}

fn text_height(text: &str, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    let width = usize::from(width);
    let mut rows = 0u16;
    for raw in text.split('\n') {
        let line_width = Line::from(raw).width();
        let lines = if line_width == 0 {
            1
        } else {
            line_width.div_ceil(width)
        };
        rows = rows.saturating_add(u16::try_from(lines).unwrap_or(u16::MAX));
    }
    rows.max(1)
}

fn wrap_body(text: &str, width: u16) -> Vec<String> {
    let max = usize::from(width);
    if max == 0 {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        if raw.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut used = 0usize;
        for ch in raw.chars() {
            let ch_width = Line::from(ch.to_string()).width();
            if used > 0 && used.saturating_add(ch_width) > max {
                lines.push(std::mem::take(&mut current));
                used = 0;
            }
            if used == 0 && ch_width > max {
                lines.push(ch.to_string());
                continue;
            }
            current.push(ch);
            used = used.saturating_add(ch_width);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn ratio_percent(ratio: f64) -> u16 {
    let percent = (ratio.clamp(0.0, 1.0) * 100.0).round();
    if percent.is_finite() {
        percent as u16
    } else {
        50
    }
}

fn percent_width(width: u16, percent: u16) -> u16 {
    width.saturating_mul(percent) / 100
}

fn tab_index(items: &[lilyco_core::view::TabItem], active: &Option<String>) -> usize {
    active
        .as_deref()
        .and_then(|id| items.iter().position(|item| item.id == id))
        .unwrap_or(0)
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

// ── 渲染入口 ─────────────────────────────────────────────

/// 把组件渲染到给定区域（区域已被调用方裁剪）
pub fn render(component: &Component, area: Rect, buf: &mut Buffer) {
    render_with_theme(component, area, buf, &Theme::default());
}

/// 使用指定主题渲染组件
pub fn render_with_theme(component: &Component, area: Rect, buf: &mut Buffer, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    match component {
        Component::Card {
            title,
            description,
            children,
        } => {
            let block = Block::bordered()
                .title(title.clone().unwrap_or_default())
                .border_style(theme.border_style());
            let inner = block.inner(area);
            block.render(area, buf);
            let mut offset = 0u16;
            if let Some(text) = description {
                let height = text_height(text, inner.width).min(inner.height);
                Paragraph::new(text.as_str())
                    .style(dim(theme))
                    .wrap(Wrap { trim: false })
                    .render(Rect::new(inner.x, inner.y, inner.width, height), buf);
                offset = height;
            }
            render_children(
                children,
                Rect::new(
                    inner.x,
                    inner.y.saturating_add(offset),
                    inner.width,
                    inner.height.saturating_sub(offset),
                ),
                buf,
                theme,
            );
        }
        Component::Section { title, children } => {
            let offset = u16::from(title.is_some());
            if let Some(text) = title {
                write_row(buf, area, 0, text, bold(theme));
            }
            render_children(
                children,
                Rect::new(
                    area.x,
                    area.y.saturating_add(offset),
                    area.width,
                    area.height.saturating_sub(offset),
                ),
                buf,
                theme,
            );
        }
        Component::Separator { label } => {
            let text = match label {
                Some(text) => format!("── {text} ──"),
                None => "─".repeat(usize::from(area.width)),
            };
            write_row(buf, area, 0, &text, dim(theme));
        }
        Component::ScrollArea { children, .. } => render_children(children, area, buf, theme),
        Component::AspectRatio { children, .. } => render_children(children, area, buf, theme),
        Component::Collapsible {
            title,
            open,
            children,
            ..
        } => {
            let marker = if *open { "[-]" } else { "[+]" };
            write_row(buf, area, 0, &format!("{marker} {title}"), bold(theme));
            if *open {
                render_children(
                    children,
                    Rect::new(
                        area.x,
                        area.y.saturating_add(1),
                        area.width,
                        area.height.saturating_sub(1),
                    ),
                    buf,
                    theme,
                );
            }
        }
        Component::Accordion { items } => {
            let mut offset = 0u16;
            for item in items {
                if offset >= area.height {
                    break;
                }
                let marker = if item.open { "[-]" } else { "[+]" };
                write_row(
                    buf,
                    area,
                    offset,
                    &format!("{marker} {}", item.title),
                    bold(theme),
                );
                offset = offset.saturating_add(1);
                if item.open && offset < area.height {
                    let height = text_height(&item.body, area.width).min(area.height - offset);
                    Paragraph::new(item.body.as_str())
                        .style(theme.text_style())
                        .wrap(Wrap { trim: false })
                        .render(Rect::new(area.x, area.y + offset, area.width, height), buf);
                    offset = offset.saturating_add(height);
                }
            }
        }
        Component::Tabs { items, active } => {
            let index = tab_index(items, active);
            let titles: Vec<Line> = items
                .iter()
                .map(|item| Line::from(item.label.clone()))
                .collect();
            if !titles.is_empty() {
                Tabs::new(titles)
                    .select(index)
                    .highlight_style(cyan(theme).add_modifier(Modifier::BOLD))
                    .render(Rect::new(area.x, area.y, area.width, 1), buf);
            }
            if let Some(item) = items.get(index) {
                render_children(
                    &item.children,
                    Rect::new(
                        area.x,
                        area.y.saturating_add(1),
                        area.width,
                        area.height.saturating_sub(1),
                    ),
                    buf,
                    theme,
                );
            }
        }
        Component::Breadcrumb { items } => {
            write_row(buf, area, 0, &items.join("  ›  "), dim(theme));
        }
        Component::Pagination { page, pages } => {
            write_row(buf, area, 0, &format!("第 {page} / {pages} 页"), dim(theme));
        }
        Component::Sidebar { items, active } => {
            let block = Block::bordered().border_style(theme.border_style());
            let inner = block.inner(area);
            block.render(area, buf);
            for (index, item) in items.iter().enumerate() {
                if index as u16 >= inner.height {
                    break;
                }
                let selected = active.as_deref() == Some(item.id.as_str());
                let marker = if selected { "›" } else { " " };
                let style = if selected {
                    cyan(theme).add_modifier(Modifier::BOLD)
                } else {
                    theme.text_style()
                };
                write_row(
                    buf,
                    inner,
                    index as u16,
                    &format!("{marker} {}", item.label),
                    style,
                );
            }
        }
        Component::Resizable { ratio, start, end } => {
            let left = ratio_percent(*ratio).clamp(1, 99);
            let chunks = Layout::horizontal([
                Constraint::Percentage(left),
                Constraint::Percentage(100 - left),
            ])
            .split(area);
            render_children(start, chunks[0], buf, theme);
            render_children(end, chunks[1], buf, theme);
        }
        Component::Carousel { index, items } => {
            if let Some(item) = items.get(*index as usize) {
                if let Some(title) = &item.title {
                    write_row(buf, area, 0, title, bold(theme));
                }
                render_children(
                    &item.children,
                    Rect::new(
                        area.x,
                        area.y.saturating_add(1),
                        area.width,
                        area.height.saturating_sub(1),
                    ),
                    buf,
                    theme,
                );
            }
        }
        Component::Progress {
            label,
            current,
            total,
        } => {
            let mut offset = 0u16;
            if let Some(text) = label {
                write_row(buf, area, 0, text, theme.text_style());
                offset = 1;
            }
            if offset >= area.height {
                return;
            }
            let bar = Rect::new(area.x, area.y + offset, area.width, 1);
            match (current, total) {
                (Some(done), Some(all)) if *all > 0 => {
                    let ratio = (*done as f64 / *all as f64).clamp(0.0, 1.0);
                    Gauge::default()
                        .ratio(ratio)
                        .gauge_style(cyan(theme))
                        .render(bar, buf);
                }
                _ => write_row(buf, area, offset, "正在处理…", dim(theme)),
            }
        }
        Component::Badge { text, tone } => {
            let style = theme
                .text_style()
                .fg(Color::Black)
                .bg(tone_color(*tone, theme));
            write_row(buf, area, 0, &format!(" {text} "), style);
        }
        Component::Alert {
            title,
            description,
            tone,
        } => {
            let color = tone_color(*tone, theme);
            let block = Block::bordered()
                .title(title.clone())
                .border_style(Style::default().fg(color))
                .title_style(Style::default().fg(color));
            let inner = block.inner(area);
            block.render(area, buf);
            if let Some(text) = description {
                Paragraph::new(text.as_str())
                    .style(theme.text_style())
                    .wrap(Wrap { trim: false })
                    .render(inner, buf);
            }
        }
        Component::Toast { text, tone } => {
            let style = theme
                .text_style()
                .fg(Color::Black)
                .bg(tone_color(*tone, theme));
            write_row(buf, area, 0, &format!(" {text} "), style);
        }
        Component::Skeleton { lines } => {
            let width = usize::from(area.width).min(80);
            let row_text = "░".repeat(width);
            for index in 0..usize::try_from((*lines).max(1)).unwrap_or(0) {
                if index as u16 >= area.height {
                    break;
                }
                write_row(buf, area, index as u16, &row_text, dim(theme));
            }
        }
        Component::Avatar { name, src } => {
            let text = if src.as_deref().is_some_and(|value| !value.is_empty()) {
                format!("({name}) ▸ 图片")
            } else {
                format!("({name})")
            };
            write_row(buf, area, 0, &text, cyan(theme));
        }
        Component::KeyValue { items } => {
            for (index, item) in items.iter().enumerate() {
                if index as u16 >= area.height {
                    break;
                }
                let line = vec![
                    Span::styled(format!("{}: ", item.key), bold(theme)),
                    Span::styled(item.value.clone(), theme.text_style()),
                ];
                write_spans(buf, area, index as u16, line);
            }
        }
        Component::Table {
            caption,
            columns,
            rows,
            sortable: _,
        } => {
            let column_width = (usize::from(area.width) / columns.len().max(1)).max(1);
            let mut offset = 0u16;
            if let Some(text) = caption {
                write_row(buf, area, offset, text, dim(theme));
                offset = offset.saturating_add(1);
            }
            if offset < area.height {
                let header: String = columns
                    .iter()
                    .map(|column| pad(column, column_width))
                    .collect();
                write_row(buf, area, offset, &header, bold(theme));
                offset = offset.saturating_add(1);
            }
            for row in rows {
                if offset >= area.height {
                    break;
                }
                let text: String = row.iter().map(|cell| pad(cell, column_width)).collect();
                write_row(buf, area, offset, &text, theme.text_style());
                offset = offset.saturating_add(1);
            }
        }
        Component::Chart {
            chart_kind: _,
            labels: _,
            series,
        } => {
            if series.is_empty() {
                write_row(buf, area, 0, "—", dim(theme));
            } else {
                let data: Vec<u64> = series
                    .iter()
                    .map(|value| {
                        if value.is_finite() && *value > 0.0 {
                            *value as u64
                        } else {
                            0
                        }
                    })
                    .collect();
                Sparkline::default()
                    .data(&data)
                    .style(cyan(theme))
                    .render(Rect::new(area.x, area.y, area.width, 1), buf);
            }
        }
        Component::Log { lines } => {
            for (index, item) in lines.iter().enumerate() {
                if index as u16 >= area.height {
                    break;
                }
                write_row(
                    buf,
                    area,
                    index as u16,
                    &format_log(item),
                    log_style(&item.level, theme),
                );
            }
        }
        Component::Result { ok, title, value } => {
            let (marker, style) = if *ok {
                ("✔", theme.success_style())
            } else {
                ("✘", theme.danger_style())
            };
            write_row(buf, area, 0, &format!("{marker} {title}"), style);
            if let Some(data) = value {
                if area.height > 1 {
                    Paragraph::new(pretty_json(data))
                        .style(theme.text_style())
                        .wrap(Wrap { trim: false })
                        .render(
                            Rect::new(area.x, area.y + 1, area.width, area.height - 1),
                            buf,
                        );
                }
            }
        }
        Component::Button {
            label,
            variant,
            disabled,
            ..
        } => {
            let style = match variant {
                ButtonVariant::Primary => {
                    Style::default().fg(Color::Black).bg(color(&theme.accent))
                }
                ButtonVariant::Danger => Style::default().fg(Color::Black).bg(color(&theme.danger)),
                ButtonVariant::Outline => theme.accent_style(),
                ButtonVariant::Ghost => dim(theme),
            };
            let style = if *disabled {
                style.add_modifier(Modifier::DIM)
            } else {
                style
            };
            write_row(buf, area, 0, &format!("[ {label} ]"), style);
        }
        Component::Toggle { label, pressed, .. } => {
            let marker = if *pressed { "[x]" } else { "[ ]" };
            write_row(
                buf,
                area,
                0,
                &format!("{marker} {label}"),
                theme.text_style(),
            );
        }
        Component::ToggleGroup {
            options, selected, ..
        } => {
            let text = options
                .iter()
                .map(|option| {
                    let marker = if selected.iter().any(|value| value == option) {
                        "●"
                    } else {
                        "○"
                    };
                    format!("{marker} {option}")
                })
                .collect::<Vec<_>>()
                .join("   ");
            write_row(buf, area, 0, &text, theme.text_style());
        }
        Component::Tooltip { text, children } => {
            if children.is_empty() {
                write_row(buf, area, 0, text, dim(theme));
            } else {
                render_children(children, area, buf, theme);
            }
        }
        Component::HoverCard {
            trigger,
            title,
            body,
        } => write_row(
            buf,
            area,
            0,
            &format!("{trigger} ▸ {title}: {body}"),
            dim(theme),
        ),
        Component::Input {
            name,
            label,
            value,
            placeholder,
        } => {
            let caption = label.as_deref().unwrap_or(name);
            let shown = if value.is_empty() {
                placeholder.as_deref().unwrap_or("_")
            } else {
                value
            };
            write_row(
                buf,
                area,
                0,
                &format!("{caption}: {shown}"),
                theme.text_style(),
            );
        }
        Component::Textarea {
            name,
            label,
            value,
            rows,
        } => {
            let caption = label.as_deref().unwrap_or(name);
            write_row(buf, area, 0, &format!("{caption}:"), bold(theme));
            let rows = usize::try_from(*rows).unwrap_or(usize::MAX);
            for (index, line) in value.lines().enumerate() {
                if index >= rows || index as u16 + 1 >= area.height {
                    break;
                }
                write_row(buf, area, index as u16 + 1, line, theme.text_style());
            }
        }
        Component::Select {
            name, label, value, ..
        }
        | Component::Combobox {
            name, label, value, ..
        } => {
            let caption = label.as_deref().unwrap_or(name);
            write_row(
                buf,
                area,
                0,
                &format!("{caption}: {value} ▾"),
                theme.text_style(),
            );
        }
        Component::Checkbox { label, checked, .. } => {
            let marker = if *checked { "[x]" } else { "[ ]" };
            write_row(
                buf,
                area,
                0,
                &format!("{marker} {label}"),
                theme.text_style(),
            );
        }
        Component::RadioGroup {
            name,
            label,
            options,
            selected,
        } => {
            let caption = label.as_deref().unwrap_or(name);
            let text = options
                .iter()
                .map(|option| {
                    let marker = if selected.as_deref() == Some(option.as_str()) {
                        "●"
                    } else {
                        "○"
                    };
                    format!("{marker} {option}")
                })
                .collect::<Vec<_>>()
                .join("   ");
            write_row(
                buf,
                area,
                0,
                &format!("{caption}: {text}"),
                theme.text_style(),
            );
        }
        Component::Switch { label, checked, .. } => {
            let state = if *checked { "开" } else { "关" };
            write_row(
                buf,
                area,
                0,
                &format!("{label}: [{state}]"),
                theme.text_style(),
            );
        }
        Component::Slider {
            name,
            label,
            min,
            max,
            value,
        } => {
            let caption = label.as_deref().unwrap_or(name);
            let span = (*max - *min).abs();
            let ratio = if span > 0.0 {
                ((*value - *min) / span).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let filled = (ratio * 10.0).round() as usize;
            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled));
            write_row(
                buf,
                area,
                0,
                &format!("{caption}: {bar} {value}"),
                theme.text_style(),
            );
        }
        Component::InputOtp {
            name,
            label,
            value,
            length,
        } => {
            let caption = label.as_deref().unwrap_or(name);
            let filled = value.chars().count();
            let cells = (0..*length)
                .map(|index| {
                    if (index as usize) < filled {
                        "●"
                    } else {
                        "○"
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            write_row(
                buf,
                area,
                0,
                &format!("{caption}: {cells}"),
                theme.text_style(),
            );
        }
        Component::Calendar {
            month,
            selected,
            marked,
        } => {
            let text = format!(
                "📅 {month}  选中: {}  标记: {}",
                selected.as_deref().unwrap_or("-"),
                marked.join(", ")
            );
            write_row(buf, area, 0, &text, dim(theme));
        }
        Component::DatePicker { name, label, value } => {
            let caption = label.as_deref().unwrap_or(name);
            write_row(
                buf,
                area,
                0,
                &format!("{caption}: {} ▾", value.as_deref().unwrap_or("-")),
                theme.text_style(),
            );
        }
        Component::Form { children, submit } => {
            let offset = children_height(children, area.width).min(area.height);
            render_children(
                children,
                Rect::new(area.x, area.y, area.width, offset),
                buf,
                theme,
            );
            if let Some(action) = submit {
                if offset < area.height {
                    write_row(buf, area, offset, &format!("[ {action} ]"), cyan(theme));
                }
            }
        }
        Component::Label { text, .. } => write_row(buf, area, 0, text, bold(theme)),
        Component::Dialog {
            title,
            description,
            children,
            ..
        } => {
            let block = Block::bordered()
                .title(title.clone())
                .border_style(theme.border_style());
            let inner = block.inner(area);
            block.render(area, buf);
            let mut offset = 0u16;
            if let Some(text) = description {
                let height = text_height(text, inner.width).min(inner.height);
                Paragraph::new(text.as_str())
                    .style(dim(theme))
                    .wrap(Wrap { trim: false })
                    .render(Rect::new(inner.x, inner.y, inner.width, height), buf);
                offset = height;
            }
            render_children(
                children,
                Rect::new(
                    inner.x,
                    inner.y.saturating_add(offset),
                    inner.width,
                    inner.height.saturating_sub(offset),
                ),
                buf,
                theme,
            );
        }
        Component::Sheet {
            title,
            side,
            children,
            ..
        } => {
            let block = Block::bordered()
                .title(format!("{title}  ({side:?})"))
                .border_style(theme.border_style());
            let inner = block.inner(area);
            block.render(area, buf);
            render_children(children, inner, buf, theme);
        }
        Component::Drawer {
            title, children, ..
        } => {
            let block = Block::bordered()
                .title(title.clone())
                .border_style(theme.border_style());
            let inner = block.inner(area);
            block.render(area, buf);
            render_children(children, inner, buf, theme);
        }
        Component::AlertDialog {
            title,
            description,
            confirm,
            cancel,
            ..
        } => {
            let block = Block::bordered()
                .title(title.clone())
                .border_style(theme.border_style());
            let inner = block.inner(area);
            block.render(area, buf);
            let mut offset = 0u16;
            if let Some(text) = description {
                let height = text_height(text, inner.width).min(inner.height);
                Paragraph::new(text.as_str())
                    .style(dim(theme))
                    .wrap(Wrap { trim: false })
                    .render(Rect::new(inner.x, inner.y, inner.width, height), buf);
                offset = height;
            }
            if offset < inner.height {
                write_row(
                    buf,
                    inner,
                    offset,
                    &format!("[ {confirm} ]  [ {cancel} ]"),
                    theme.text_style(),
                );
            }
        }
        Component::Popover {
            trigger, children, ..
        } => {
            write_row(buf, area, 0, trigger, bold(theme));
            render_children(
                children,
                Rect::new(
                    area.x,
                    area.y.saturating_add(1),
                    area.width,
                    area.height.saturating_sub(1),
                ),
                buf,
                theme,
            );
        }
        Component::DropdownMenu { trigger, items, .. } => {
            write_row(buf, area, 0, &format!("{trigger} ▾"), bold(theme));
            render_menu_items(items, area, 1, buf, theme);
        }
        Component::ContextMenu { items } => render_menu_items(items, area, 0, buf, theme),
        Component::Menubar { menus } => {
            for (index, menu) in menus.iter().enumerate() {
                if index as u16 >= area.height {
                    break;
                }
                let labels = menu
                    .items
                    .iter()
                    .map(|item| item.label.clone())
                    .collect::<Vec<_>>()
                    .join("  ");
                write_row(
                    buf,
                    area,
                    index as u16,
                    &format!("{}: {labels}", menu.label),
                    theme.text_style(),
                );
            }
        }
        Component::Command { query, items } => {
            let mut offset = 0u16;
            if let Some(text) = query {
                write_row(buf, area, 0, &format!("> {text}"), dim(theme));
                offset = 1;
            }
            for (index, item) in items.iter().enumerate() {
                let row = offset.saturating_add(index as u16);
                if row >= area.height {
                    break;
                }
                let hint = item.hint.as_deref().unwrap_or("");
                write_row(
                    buf,
                    area,
                    row,
                    &format!("{}  {}  {hint}", item.label, item.action),
                    theme.text_style(),
                );
            }
        }
        Component::NavigationMenu { items, active } => {
            let text = items
                .iter()
                .map(|item| {
                    if active.as_deref() == Some(item.id.as_str()) {
                        format!("[{}]", item.label)
                    } else {
                        item.label.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join("  ");
            write_row(buf, area, 0, &text, theme.text_style());
        }
        Component::MessageList {
            entries,
            selected,
            unread,
        } => {
            let mut offset = 0u16;
            for (index, entry) in entries.iter().enumerate() {
                if offset >= area.height {
                    break;
                }
                let is_selected = *selected == Some(index);
                let is_unread = *unread == Some(index);
                let header = message_header(entry, is_selected, is_unread);
                let mut style = theme.text_style();
                if entry.mine {
                    style = style.fg(color(&theme.accent));
                }
                if entry.tone == Tone::Danger {
                    style = style.fg(color(&theme.danger));
                }
                if is_selected {
                    style = style
                        .fg(color(&theme.accent))
                        .add_modifier(Modifier::REVERSED | Modifier::BOLD);
                }
                write_row(buf, area, offset, &header, style);
                offset = offset.saturating_add(1);
                if offset >= area.height {
                    break;
                }
                let body_lines = wrap_body(&entry.body, area.width);
                let body_height = u16::try_from(body_lines.len())
                    .unwrap_or(u16::MAX)
                    .min(area.height.saturating_sub(offset));
                if body_height > 0 {
                    for (row, line) in body_lines.iter().take(usize::from(body_height)).enumerate()
                    {
                        let row = u16::try_from(row).unwrap_or(u16::MAX);
                        write_row(
                            buf,
                            area,
                            offset.saturating_add(row),
                            line,
                            theme.text_style(),
                        );
                    }
                    offset = offset.saturating_add(body_height);
                }
                if offset < area.height {
                    offset = offset.saturating_add(1);
                }
            }
        }
        Component::Composer {
            value,
            placeholder,
            mode,
            attachments,
        } => {
            let line = match mode {
                ComposerMode::Normal => {
                    let shown = if value.is_empty() {
                        placeholder.as_str()
                    } else {
                        value.as_str()
                    };
                    format!(" 草稿: {shown}")
                }
                ComposerMode::Insert => format!("> {value}▌"),
            };
            write_row(buf, area, 0, &line, theme.text_style());
            for (index, attachment) in attachments.iter().enumerate() {
                let row = u16::try_from(index.saturating_add(1)).unwrap_or(u16::MAX);
                if row >= area.height {
                    break;
                }
                write_row(buf, area, row, &format!("  📎 {attachment}"), dim(theme));
            }
        }
        Component::Pane {
            title,
            active,
            children,
            ..
        } => {
            let title = if *active {
                format!("▶ {title}")
            } else {
                title.clone()
            };
            let mut block = Block::bordered().title(title);
            if *active {
                block = block
                    .border_style(theme.accent_style())
                    .title_style(theme.accent_style());
            } else {
                block = block
                    .border_style(theme.border_style())
                    .title_style(theme.text_style());
            }
            let inner = block.inner(area);
            block.render(area, buf);
            render_children(children, inner, buf, theme);
        }
    }
}

fn render_children(children: &[Component], area: Rect, buf: &mut Buffer, theme: &Theme) {
    let bottom = area.y.saturating_add(area.height);
    let mut y = area.y;
    for child in children {
        if y >= bottom {
            break;
        }
        let height = measure(child, area.width).min(bottom.saturating_sub(y));
        render_with_theme(child, Rect::new(area.x, y, area.width, height), buf, theme);
        y = y.saturating_add(height);
    }
}

fn render_menu_items(
    items: &[lilyco_core::view::MenuItem],
    area: Rect,
    offset: u16,
    buf: &mut Buffer,
    theme: &Theme,
) {
    for (index, item) in items.iter().enumerate() {
        let row = offset.saturating_add(index as u16);
        if row >= area.height {
            break;
        }
        let style = if item.danger {
            theme.danger_style()
        } else {
            theme.text_style()
        };
        write_row(buf, area, row, &format!("  {}", item.label), style);
    }
}

// ── 样式与文本辅助 ───────────────────────────────────────

fn write_row(buf: &mut Buffer, area: Rect, row: u16, text: &str, style: Style) {
    if row >= area.height || area.width == 0 {
        return;
    }
    Paragraph::new(text)
        .style(style)
        .render(Rect::new(area.x, area.y + row, area.width, 1), buf);
}

fn write_spans(buf: &mut Buffer, area: Rect, row: u16, spans: Vec<Span<'static>>) {
    if row >= area.height || area.width == 0 {
        return;
    }
    Paragraph::new(Line::from(spans)).render(Rect::new(area.x, area.y + row, area.width, 1), buf);
}

fn message_header(entry: &MessageEntry, selected: bool, unread: bool) -> String {
    let mut prefix = String::new();
    if selected {
        prefix.push_str("› ");
    }
    if unread {
        prefix.push_str("● ");
    }
    let mut header = format!("{prefix}{} · {}", entry.author, entry.time);
    if entry.edited {
        header.push_str(" (已编辑)");
    }
    if let Some(reply_to) = &entry.reply_to {
        header.push_str(&format!(" ↩ {reply_to}"));
    }
    header
}

fn format_log(line: &LogLine) -> String {
    let level = match line.level {
        LogLevel::Debug => "DBG",
        LogLevel::Info => "INF",
        LogLevel::Warn => "WRN",
        LogLevel::Error => "ERR",
    };
    format!("[{level}] {}", line.text)
}

fn log_style(level: &LogLevel, theme: &Theme) -> Style {
    match level {
        LogLevel::Debug => theme.muted_style(),
        LogLevel::Info => theme.accent_style(),
        LogLevel::Warn => theme.warn_style(),
        LogLevel::Error => theme.danger_style(),
    }
}

fn tone_color(tone: Tone, theme: &Theme) -> Color {
    match tone {
        Tone::Neutral => color(&theme.muted),
        Tone::Info => color(&theme.accent),
        Tone::Success => color(&theme.success),
        Tone::Warn => color(&theme.warn),
        Tone::Danger => color(&theme.danger),
    }
}

fn pad(text: &str, width: usize) -> String {
    if text.chars().count() >= width {
        return text.chars().take(width).collect();
    }
    format!("{text:<width$}")
}

fn bold(theme: &Theme) -> Style {
    theme.text_style().add_modifier(Modifier::BOLD)
}

fn dim(theme: &Theme) -> Style {
    theme.muted_style()
}

fn cyan(theme: &Theme) -> Style {
    theme.accent_style()
}

// ── 测试 ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn buffer_text(buf: &Buffer, area: Rect) -> String {
        let mut out = String::new();
        for y in 0..area.height {
            let mut x = 0;
            while x < area.width {
                let symbol = buf.cell((x, y)).map_or(" ", |cell| cell.symbol());
                out.push_str(symbol);
                let width = Line::from(symbol).width().max(1);
                x += u16::try_from(width).unwrap_or(1);
            }
            out.push('\n');
        }
        out
    }

    fn render_to_string(component: &Component, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        render(component, area, &mut buf);
        buffer_text(&buf, area)
    }

    #[test]
    fn progress_with_label_renders_label_and_bar() {
        let out = render_to_string(
            &Component::Progress {
                label: Some("下载中".into()),
                current: Some(1),
                total: Some(2),
            },
            20,
            3,
        );
        assert!(out.contains("下载中"), "{out}");
    }

    #[test]
    fn card_renders_title_description_and_children() {
        let out = render_to_string(
            &Component::Card {
                title: Some("卡片".into()),
                description: Some("说明".into()),
                children: vec![Component::Badge {
                    text: "徽章".into(),
                    tone: Tone::Success,
                }],
            },
            24,
            6,
        );
        assert!(out.contains("卡片"), "{out}");
        assert!(out.contains("说明"), "{out}");
        assert!(out.contains("徽章"), "{out}");
    }

    #[test]
    fn table_renders_header_and_rows() {
        let out = render_to_string(
            &Component::Table {
                caption: Some("账号".into()),
                columns: vec!["名称".into(), "号码".into()],
                rows: vec![vec!["兰雀".into(), "1957374829".into()]],
                sortable: true,
            },
            40,
            4,
        );
        assert!(out.contains("名称"), "{out}");
        assert!(out.contains("兰雀"), "{out}");
        assert!(out.contains("1957374829"), "{out}");
    }

    #[test]
    fn nested_children_are_stacked_vertically() {
        let component = Component::Section {
            title: Some("区段".into()),
            children: vec![
                Component::Badge {
                    text: "第一".into(),
                    tone: Tone::Info,
                },
                Component::Badge {
                    text: "第二".into(),
                    tone: Tone::Info,
                },
            ],
        };
        let out = render_to_string(&component, 20, 4);
        let first = out.find("第一").unwrap_or(0);
        let second = out.find("第二").unwrap_or(0);
        assert!(first < second, "{out}");
    }

    #[test]
    fn measure_handles_narrow_width_and_chinese() {
        let component = Component::Alert {
            title: "警告".into(),
            description: Some("这是一段需要换行的中文描述".into()),
            tone: Tone::Warn,
        };
        let narrow = measure(&component, 8);
        let wide = measure(&component, 60);
        assert!(narrow >= 3, "narrow={narrow}");
        assert!(narrow > wide, "narrow={narrow} wide={wide}");
    }

    #[test]
    fn adapter_capabilities_follow_protocol() {
        let caps = adapter_capabilities();
        assert_eq!(caps.adapter_id, "lilyco-tui");
        assert_eq!(caps.fallback, FallbackStrategy::DegradeToText);
        assert!(caps.is_protocol_compatible());
        assert!(caps.supported_kinds.iter().any(|kind| kind == "progress"));
        assert!(caps.supported_kinds.iter().any(|kind| kind == "card"));
    }

    #[test]
    fn message_list_renders_selection_unread_edit_and_reply() {
        let component = Component::MessageList {
            entries: vec![
                MessageEntry {
                    author: "我".into(),
                    time: "09:00".into(),
                    body: "早上好".into(),
                    edited: false,
                    reply_to: None,
                    tone: Tone::Neutral,
                    mine: true,
                },
                MessageEntry {
                    author: "兰雀".into(),
                    time: "09:01".into(),
                    body: "收到\n明天见".into(),
                    edited: true,
                    reply_to: Some("早上好".into()),
                    tone: Tone::Danger,
                    mine: false,
                },
            ],
            selected: Some(1),
            unread: Some(1),
        };
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        render(&component, area, &mut buf);
        let out = buffer_text(&buf, area);

        assert!(out.contains('●'), "未读徽标缺失: {out}");
        assert!(out.contains('›'), "选中前缀缺失: {out}");
        assert!(out.contains("(已编辑)"), "编辑标记缺失: {out}");
        assert!(out.contains("↩ 早上好"), "回复引用缺失: {out}");
        assert!(out.contains("明天见"), "正文换行缺失: {out}");

        let mine_row = out
            .lines()
            .position(|line| line.contains("我 ·"))
            .unwrap_or(0) as u16;
        assert_eq!(
            buf.cell((0, mine_row)).map(|cell| cell.fg),
            Some(Color::Cyan),
            "自己的消息应为青色: {out}"
        );

        let selected_row = out
            .lines()
            .position(|line| line.contains("兰雀"))
            .unwrap_or(0) as u16;
        let cell = buf.cell((0, selected_row)).expect("选中的头部单元格");
        assert!(
            cell.modifier.contains(Modifier::REVERSED),
            "选中行应为反色: {out}"
        );
        assert_eq!(cell.fg, Color::Cyan, "选中行应为 accent: {out}");

        assert_eq!(measure(&component, 40), 7, "测量应与渲染行数一致");
    }

    #[test]
    fn composer_renders_normal_and_insert_modes() {
        let normal = Component::Composer {
            value: String::new(),
            placeholder: "写点什么".into(),
            mode: ComposerMode::Normal,
            attachments: vec![],
        };
        let out = render_to_string(&normal, 40, 2);
        assert!(out.contains(" 草稿: 写点什么"), "{out}");
        assert_eq!(measure(&normal, 40), 1);

        let insert = Component::Composer {
            value: "你好".into(),
            placeholder: "写点什么".into(),
            mode: ComposerMode::Insert,
            attachments: vec!["a.png".into(), "b.txt".into()],
        };
        let out = render_to_string(&insert, 40, 4);
        assert!(out.contains("> 你好▌"), "{out}");
        assert!(out.contains("📎 a.png"), "{out}");
        assert!(out.contains("📎 b.txt"), "{out}");
        assert_eq!(measure(&insert, 40), 3);
    }

    #[test]
    fn pane_active_renders_highlighted_title_and_children() {
        let component = Component::Pane {
            id: "main".into(),
            title: "聊天".into(),
            active: true,
            children: vec![Component::Badge {
                text: "消息".into(),
                tone: Tone::Info,
            }],
        };
        let area = Rect::new(0, 0, 30, 5);
        let mut buf = Buffer::empty(area);
        render(&component, area, &mut buf);
        let out = buffer_text(&buf, area);

        assert!(out.contains("▶ 聊天"), "激活标题缺失: {out}");
        assert!(out.contains("消息"), "子组件缺失: {out}");
        assert_eq!(
            buf.cell((0, 0)).map(|cell| cell.fg),
            Some(Color::Cyan),
            "激活边框应为青色: {out}"
        );
        assert_eq!(measure(&component, 30), 3, "边框 2 行 + 子组件高度");
    }

    #[test]
    fn render_uses_theme_accent() {
        let mut theme = Theme::default();
        theme.accent = crate::theme::ColorSpec("magenta".into());
        let component = Component::Pane {
            id: "main".into(),
            title: "聊天".into(),
            active: true,
            children: vec![],
        };
        let area = Rect::new(0, 0, 24, 5);
        let mut buf = Buffer::empty(area);
        render_with_theme(&component, area, &mut buf, &theme);

        let mut magenta = 0usize;
        for y in 0..area.height {
            for x in 0..area.width {
                if buf
                    .cell((x, y))
                    .is_some_and(|cell| cell.fg == Color::Magenta)
                {
                    magenta += 1;
                }
            }
        }
        assert!(magenta > 0, "边框/标题应有主题 accent 颜色");
        assert_eq!(
            buf.cell((0, 0)).map(|cell| cell.fg),
            Some(Color::Magenta),
            "Pane 左上边框应为 accent"
        );
        assert_eq!(
            buf.cell((1, 0)).map(|cell| cell.fg),
            Some(Color::Magenta),
            "Pane 标题应为 accent"
        );
    }

    #[test]
    fn zero_width_render_is_noop() {
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        render(
            &Component::Badge {
                text: "空".into(),
                tone: Tone::Neutral,
            },
            area,
            &mut buf,
        );
        assert_eq!(
            measure(
                &Component::Badge {
                    text: "空".into(),
                    tone: Tone::Neutral
                },
                0
            ),
            0
        );
    }
}
