//! Minimal GPUI shell for the tiny agent.
//!
//! Scope: one window, three regions (sidebar / trace / composer), three
//! switchable themes, and a desktop-facts badge. Every colour comes from
//! `tiny_lilyco::ui`, so nothing here is hard-coded.
//!
//! This crate is standalone (`[workspace]` in Cargo.toml) and is not part of
//! the headless CI. GPUI's API moves fast: pin the git revision before relying
//! on it.

use gpui::prelude::*;
use gpui::{
    div, px, size, App, Application, Bounds, Context, IntoElement, Render, Window, WindowBounds,
    WindowOptions,
};
use tiny_lilyco::adapters::image_inspect::Format;
use tiny_lilyco::agent::Phase;
use tiny_lilyco::ui::{tokens, Color, RowBody, Theme, TraceRow};
use tiny_lilyco_uda::DesktopFacts;

/// Convert a design token into a GPUI colour.
fn paint(c: Color) -> gpui::Rgba {
    let packed = ((c.r() as u32) << 16) | ((c.g() as u32) << 8) | c.b() as u32;
    gpui::rgb(packed)
}

/// Row built at compile time so the call site stays shallow.
const IMAGE_ROW: TraceRow<'static> = TraceRow::new(
    5,
    Phase::Observe,
    "image_probe",
    RowBody::Image {
        format: Format::Png,
        width: 1024,
        height: 768,
    },
);

struct TinyAgent {
    theme: Theme,
    facts: DesktopFacts,
    trace: [TraceRow<'static>; 3],
}

impl TinyAgent {
    fn new(theme: Theme, facts: DesktopFacts) -> Self {
        TinyAgent {
            theme,
            facts,
            trace: [
                TraceRow::new(3, Phase::Act, "edit_file", RowBody::Diff { added: 12, removed: 4 }),
                TraceRow::new(4, Phase::Verify, "cargo test", RowBody::Exit { code: 0 }),
                IMAGE_ROW,
            ],
        }
    }

    fn next_theme(&mut self) {
        let all = Theme::all();
        let at = all.iter().position(|t| t.id == self.theme.id).unwrap_or(0);
        self.theme = all[(at + 1) % all.len()];
    }
}

impl Render for TinyAgent {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.theme.palette;
        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(paint(p.background))
            .text_color(paint(p.foreground))
            .child(self.sidebar())
            .child(self.workspace(cx))
    }
}

impl TinyAgent {
    fn sidebar(&self) -> impl IntoElement {
        let p = self.theme.palette;
        div()
            .flex()
            .flex_col()
            .h_full()
            .w(px(240.))
            .p(px(tokens::SPACE_3 as f32))
            .gap(px(tokens::SPACE_2 as f32))
            .bg(paint(p.sidebar))
            .border_r(px(tokens::BORDER_WIDTH as f32))
            .border_color(paint(p.sidebar_border))
            .child(self.wordmark())
            .child(self.badge("new session", p.primary, p.primary_foreground))
            .child(self.nav_row("Trace", true))
            .child(self.nav_row("Diff", false))
            .child(self.nav_row("Preview", false))
    }

    fn wordmark(&self) -> impl IntoElement {
        div()
            .text_size(px(tokens::TEXT_TITLE as f32))
            .child("Tiny Agent")
    }

    fn workspace(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.theme.palette;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .border_l(px(tokens::BORDER_WIDTH as f32))
            .border_color(paint(p.border))
            .child(self.header(cx))
            .child(self.trace_list())
            .child(self.composer())
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.theme.palette;
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px(px(tokens::SPACE_4 as f32))
            .h(px(52.))
            .border_b(px(tokens::BORDER_WIDTH as f32))
            .border_color(paint(p.border))
            .child(self.session_title())
            .child(self.theme_button(cx))
    }

    fn session_title(&self) -> impl IntoElement {
        div()
            .text_size(px(tokens::TEXT_EMPHASIS as f32))
            .child("Untitled session")
    }

    fn theme_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.theme.palette;
        div()
            .id("theme-toggle")
            .px(px(tokens::SPACE_3 as f32))
            .py(px(tokens::SPACE_1 as f32))
            .rounded(px(tokens::RADIUS_MD as f32))
            .border(px(tokens::BORDER_WIDTH as f32))
            .border_color(paint(p.border))
            .on_click(cx.listener(|this, _ev, _win, cx| {
                this.next_theme();
                cx.notify();
            }))
            .child(self.theme.label)
    }

    fn trace_list(&self) -> impl IntoElement {
        let p = self.theme.palette;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .p(px(tokens::SPACE_4 as f32))
            .gap(px(tokens::SPACE_2 as f32))
            .child(self.section_label("OODA trace", p.muted_foreground))
            .children(self.trace.iter().map(|row| self.trace_row(row)))
    }

    fn section_label(&self, text: &'static str, color: Color) -> impl IntoElement {
        div()
            .text_size(px(tokens::TEXT_SMALL as f32))
            .text_color(paint(color))
            .child(text)
    }

    fn trace_row(&self, row: &TraceRow<'static>) -> impl IntoElement {
        let p = self.theme.palette;
        let (dot, _) = row.status_colors(&p);
        let summary = row.summary_buf();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(tokens::SPACE_3 as f32))
            .px(px(tokens::SPACE_3 as f32))
            .py(px(tokens::SPACE_2 as f32))
            .rounded(px(tokens::RADIUS_LG as f32))
            .bg(paint(p.card))
            .border(px(tokens::BORDER_WIDTH as f32))
            .border_color(paint(p.border))
            .child(self.status_dot(dot))
            .child(self.badge(row.phase.label(), p.secondary, p.secondary_foreground))
            .child(self.body_label(summary.as_str()))
    }

    fn status_dot(&self, color: Color) -> impl IntoElement {
        div().w(px(8.)).h(px(8.)).rounded(px(4.)).bg(paint(color))
    }

    fn body_label(&self, text: &str) -> impl IntoElement {
        div()
            .text_size(px(tokens::TEXT_BODY as f32))
            .child(text.to_string())
    }

    fn composer(&self) -> impl IntoElement {
        let p = self.theme.palette;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(tokens::SPACE_2 as f32))
            .p(px(tokens::SPACE_4 as f32))
            .border_t(px(tokens::BORDER_WIDTH as f32))
            .border_color(paint(p.border))
            .child(self.input_placeholder())
            .child(self.badge("Run", p.primary, p.primary_foreground))
            .child(self.uda_chip())
    }

    fn input_placeholder(&self) -> impl IntoElement {
        let p = self.theme.palette;
        div()
            .flex_1()
            .px(px(tokens::SPACE_3 as f32))
            .py(px(tokens::SPACE_2 as f32))
            .rounded(px(tokens::RADIUS_MD as f32))
            .bg(paint(p.muted))
            .text_color(paint(p.muted_foreground))
            .child("Ask the agent, or paste an image…")
    }

    fn uda_chip(&self) -> impl IntoElement {
        let p = self.theme.palette;
        let label = self.facts.chip_label();
        let bg = if self.facts.degraded.is_none() { p.success } else { p.warning };
        self.badge(&label, bg, p.background)
    }

    fn nav_row(&self, label: &'static str, active: bool) -> impl IntoElement {
        let p = self.theme.palette;
        let bg = if active { p.accent } else { p.sidebar };
        div()
            .px(px(tokens::SPACE_3 as f32))
            .py(px(tokens::SPACE_2 as f32))
            .rounded(px(tokens::RADIUS_MD as f32))
            .bg(paint(bg))
            .child(label)
    }

    fn badge(&self, label: &str, bg: Color, fg: Color) -> impl IntoElement {
        div()
            .px(px(tokens::SPACE_2 as f32))
            .py(px(2.))
            .rounded(px(tokens::RADIUS_SM as f32))
            .bg(paint(bg))
            .text_color(paint(fg))
            .text_size(px(tokens::TEXT_CAPTION as f32))
            .child(label.to_string())
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let facts = tiny_lilyco_uda::probe();
        let theme = facts.preferred_theme();
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        cx.open_window(options, |_window, cx| {
            cx.new(|_| TinyAgent::new(theme, facts))
        })
        .expect("failed to open window");
    });
}
