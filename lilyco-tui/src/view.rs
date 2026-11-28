//! 语义视图状态：收集 `ViewSlot` 与运行时 `Progress::View`，按槽位顺序渲染。
//!
//! 借鉴 yazi / gitui / lazygit 的终端习惯：
//! - 内容超过视口时右侧显示滚动条，`j/k`、`PgUp/PgDn`、`Home` 可滚动；
//! - 槽位标题固定在内容流里，随内容一起滚动；
//! - 组件仍由 `crate::component` 负责绘制，本层只管布局与裁剪。

use std::collections::BTreeMap;

use lilyco_core::view::{Component, ComposerMode, ViewSlot};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Paragraph, Widget};

use crate::component;
use crate::theme::{color, Theme};

/// TUI 视图状态
#[derive(Debug, Clone, Default)]
pub struct ViewState {
    slots: Vec<ViewSlot>,
    components: BTreeMap<String, Component>,
    scroll: u16,
}

impl ViewState {
    /// 用静态槽位初始化
    pub fn new(slots: Vec<ViewSlot>) -> Self {
        Self {
            slots,
            components: BTreeMap::new(),
            scroll: 0,
        }
    }

    /// 静态槽位
    pub fn slots(&self) -> &[ViewSlot] {
        &self.slots
    }

    /// 替换静态槽位（切换命令时使用）
    pub fn set_slots(&mut self, slots: Vec<ViewSlot>) {
        self.slots = slots;
        self.scroll = 0;
    }

    /// 写入/更新一个视图组件；同一 id 后续调用覆盖旧值
    pub fn apply(&mut self, id: impl Into<String>, component: Component) {
        self.components.insert(id.into(), component);
    }

    /// 读取某个槽位的最新组件
    pub fn get(&self, id: &str) -> Option<&Component> {
        self.components.get(id)
    }

    /// 遍历全部已收到的组件（按 id 顺序）
    pub fn iter_components(&self) -> impl Iterator<Item = &Component> {
        self.components.values()
    }

    /// 把聊天状态写回语义组件，使渲染跟随本地选中与草稿
    pub fn apply_chat_state(&mut self, selected: usize, draft: &str, insert: bool) {
        let mode = if insert {
            ComposerMode::Insert
        } else {
            ComposerMode::Normal
        };
        for component in self.components.values_mut() {
            apply_chat_state_to(component, selected, draft, mode);
        }
    }

    /// 已收到的组件数量
    pub fn component_count(&self) -> usize {
        self.components.len()
    }

    /// 是否既无槽位也无组件
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty() && self.components.is_empty()
    }

    /// 当前滚动偏移（行）
    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    /// 按行滚动；负值向上，越界自动夹回
    pub fn scroll_by(&mut self, delta: i32) {
        let next = i32::from(self.scroll).saturating_add(delta);
        self.scroll = u16::try_from(next.max(0)).unwrap_or(u16::MAX);
    }

    /// 回到顶部
    pub fn reset_scroll(&mut self) {
        self.scroll = 0;
    }

    /// 内容总高度（行）
    pub fn content_height(&self, width: u16) -> u16 {
        let mut total = 0u16;
        for slot in &self.slots {
            let Some(component) = self.components.get(&slot.id) else {
                continue;
            };
            total = total
                .saturating_add(u16::from(slot.title.is_some()))
                .saturating_add(component::measure(component, width));
        }
        for (id, component) in &self.components {
            if self.slots.iter().any(|slot| slot.id == *id) {
                continue;
            }
            total = total.saturating_add(component::measure(component, width));
        }
        total
    }

    /// 给定视口是否需要滚动条
    pub fn needs_scrollbar(&self, width: u16, height: u16) -> bool {
        self.content_height(width) > height
    }

    /// 渲染全部槽位；没有对应槽位的组件按 id 顺序追加在末尾。
    ///
    /// 内容高于视口时使用临时 Buffer 渲染完整内容，再裁剪可见窗口，
    /// 并在最右列画滚动条。
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        if area.is_empty() || self.components.is_empty() {
            return;
        }
        let total = self.content_height(area.width);
        if total <= area.height {
            self.render_body(area, buf, theme);
            return;
        }
        let content_width = area.width.saturating_sub(1);
        if content_width == 0 {
            return;
        }
        let offset = self.scroll.min(total.saturating_sub(area.height));
        let temp_area = Rect::new(0, 0, content_width, total);
        let mut temp = Buffer::empty(temp_area);
        self.render_body(temp_area, &mut temp, theme);
        for row in 0..area.height {
            for col in 0..content_width {
                let source = temp.cell((col, row.saturating_add(offset)));
                let target = buf.cell_mut((area.x.saturating_add(col), area.y.saturating_add(row)));
                if let (Some(source), Some(target)) = (source, target) {
                    *target = source.clone();
                }
            }
        }
        self.render_scrollbar(area, buf, total, offset, theme);
    }

    fn render_body(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let bottom = area.y.saturating_add(area.height);
        let mut y = area.y;

        for slot in &self.slots {
            if y >= bottom {
                break;
            }
            let Some(component) = self.components.get(&slot.id) else {
                continue;
            };
            y = self.render_slot(slot, component, area, y, buf, theme);
        }

        for (id, component) in &self.components {
            if y >= bottom {
                break;
            }
            if self.slots.iter().any(|slot| slot.id == *id) {
                continue;
            }
            let height = component::measure(component, area.width).min(bottom.saturating_sub(y));
            component::render_with_theme(
                component,
                Rect::new(area.x, y, area.width, height),
                buf,
                theme,
            );
            y = y.saturating_add(height);
        }
    }

    fn render_slot(
        &self,
        slot: &ViewSlot,
        component: &Component,
        area: Rect,
        y: u16,
        buf: &mut Buffer,
        theme: &Theme,
    ) -> u16 {
        let bottom = area.y.saturating_add(area.height);
        let title_height = u16::from(slot.title.is_some());
        let total = title_height
            .saturating_add(component::measure(component, area.width))
            .min(bottom.saturating_sub(y));
        if total == 0 {
            return y;
        }
        if let Some(title) = &slot.title {
            Paragraph::new(format!(" {title} "))
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(color(&theme.accent))
                        .add_modifier(Modifier::BOLD),
                )
                .render(Rect::new(area.x, y, area.width, 1), buf);
        }
        if total > title_height {
            component::render_with_theme(
                component,
                Rect::new(
                    area.x,
                    y.saturating_add(title_height),
                    area.width,
                    total.saturating_sub(title_height),
                ),
                buf,
                theme,
            );
        }
        y.saturating_add(total)
    }

    fn render_scrollbar(
        &self,
        area: Rect,
        buf: &mut Buffer,
        total: u16,
        offset: u16,
        theme: &Theme,
    ) {
        let track = usize::from(area.height);
        if track == 0 {
            return;
        }
        let total = usize::from(total);
        if total <= track {
            return;
        }
        let thumb = (track * track / total).max(1).min(track);
        let max_offset = total - track;
        let position = usize::from(offset) * (track - thumb) / max_offset.max(1);
        let x = area.x.saturating_add(area.width.saturating_sub(1));
        for row in 0..track {
            let symbol = if row >= position && row < position + thumb {
                "█"
            } else {
                "│"
            };
            if let Some(cell) = buf.cell_mut((x, area.y.saturating_add(row as u16))) {
                cell.set_symbol(symbol);
                cell.set_style(theme.border_style());
            }
        }
    }
}

fn apply_chat_state_to(
    component: &mut Component,
    selected: usize,
    draft: &str,
    mode: ComposerMode,
) {
    match component {
        Component::MessageList { selected: slot, .. } => *slot = Some(selected),
        Component::Composer {
            value, mode: slot, ..
        } => {
            *value = draft.to_string();
            *slot = mode;
        }
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
        | Component::Pane { children, .. } => {
            apply_chat_state_to_children(children, selected, draft, mode);
        }
        Component::Resizable { start, end, .. } => {
            apply_chat_state_to_children(start, selected, draft, mode);
            apply_chat_state_to_children(end, selected, draft, mode);
        }
        Component::Tabs { items, .. } => {
            for item in items {
                apply_chat_state_to_children(&mut item.children, selected, draft, mode);
            }
        }
        Component::Carousel { items, .. } => {
            for item in items {
                apply_chat_state_to_children(&mut item.children, selected, draft, mode);
            }
        }
        _ => {}
    }
}

fn apply_chat_state_to_children(
    children: &mut [Component],
    selected: usize,
    draft: &str,
    mode: ComposerMode,
) {
    for child in children {
        apply_chat_state_to(child, selected, draft, mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco_core::view::Tone;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::text::Line;

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

    fn render_to_string(state: &ViewState, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        state.render(area, &mut buf, &Theme::default());
        buffer_text(&buf, area)
    }

    fn badges(count: usize) -> ViewState {
        let mut state = ViewState::default();
        for index in 0..count {
            state.apply(
                format!("item{index}"),
                Component::Badge {
                    text: format!("第{index}条"),
                    tone: Tone::Neutral,
                },
            );
        }
        state
    }

    #[test]
    fn apply_overwrites_same_id() {
        let mut state = ViewState::new(vec![ViewSlot::new("job")]);
        state.apply(
            "job",
            Component::Badge {
                text: "旧".into(),
                tone: Tone::Neutral,
            },
        );
        state.apply(
            "job",
            Component::Badge {
                text: "新".into(),
                tone: Tone::Success,
            },
        );
        assert_eq!(state.component_count(), 1);
        assert!(matches!(
            state.get("job"),
            Some(Component::Badge { text, .. }) if text == "新"
        ));
    }

    #[test]
    fn render_follows_slot_order() {
        let mut state = ViewState::new(vec![ViewSlot::new("a"), ViewSlot::new("b")]);
        state.apply(
            "b",
            Component::Badge {
                text: "B".into(),
                tone: Tone::Info,
            },
        );
        state.apply(
            "a",
            Component::Badge {
                text: "A".into(),
                tone: Tone::Info,
            },
        );
        let out = render_to_string(&state, 20, 4);
        let a = out.find('A').unwrap_or(usize::MAX);
        let b = out.find('B').unwrap_or(0);
        assert!(a < b, "{out}");
    }

    #[test]
    fn components_without_slots_still_render() {
        let mut state = ViewState::new(vec![]);
        state.apply(
            "free",
            Component::Badge {
                text: "自由".into(),
                tone: Tone::Neutral,
            },
        );
        assert!(!state.is_empty());
        let out = render_to_string(&state, 20, 2);
        assert!(out.contains("自由"), "{out}");
    }

    #[test]
    fn empty_state_renders_nothing() {
        let state = ViewState::default();
        assert!(state.is_empty());
        let out = render_to_string(&state, 10, 2);
        assert_eq!(out.trim(), "");
    }

    #[test]
    fn scroll_clamps_and_moves_window() {
        let mut state = badges(10);
        assert!(state.needs_scrollbar(20, 4));
        assert_eq!(state.content_height(20), 10);
        state.scroll_by(3);
        assert_eq!(state.scroll(), 3);
        let out = render_to_string(&state, 20, 4);
        assert!(out.contains("第3条"), "{out}");
        assert!(!out.contains("第0条"), "{out}");
        state.scroll_by(-100);
        assert_eq!(state.scroll(), 0);
        state.scroll_by(i32::MAX);
        assert!(state.scroll() > 0);
    }

    #[test]
    fn scrollbar_is_rendered_when_content_overflows() {
        let state = badges(10);
        let out = render_to_string(&state, 20, 4);
        assert!(out.contains('█'), "滚动条应有滑块: {out}");
        assert!(out.contains('│'), "滚动条应有轨道: {out}");
    }

    #[test]
    fn short_content_has_no_scrollbar() {
        let state = badges(2);
        assert!(!state.needs_scrollbar(20, 4));
        let out = render_to_string(&state, 20, 4);
        assert!(!out.contains('█'), "{out}");
    }
}
