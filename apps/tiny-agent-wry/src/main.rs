//! Tiny Agent — a real desktop window.
//!
//! `tao` owns the native window, `wry` renders an embedded HTML document. The
//! whole visual layer is the project's own design system: every colour comes
//! from `tiny_lilyco::ui::Theme::emit_css()`, so the GUI, the CLI and any future
//! webview front-end share one palette.
//!
//! Three themes ship (`tokyo-night`, `neutral-light`, `neutral-dark`) and are
//! switchable at runtime by setting `data-theme` on `<html>`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use tiny_lilyco::adapters::image_inspect::Format;
use tiny_lilyco::agent::Phase;
use tiny_lilyco::ui::{RowBody, Theme, TraceRow};
use wry::WebViewBuilder;

fn main() {
    if let Err(err) = run() {
        report(&format!("Tiny Agent failed to start: {err}"));
    }
}

fn run() -> wry::Result<()> {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Tiny Agent")
        .with_inner_size(LogicalSize::new(1100.0, 720.0))
        .with_min_inner_size(LogicalSize::new(880.0, 560.0))
        .build(&event_loop)
        .expect("failed to create the native window");

    let _webview = WebViewBuilder::new()
        .with_html(build_html())
        .with_devtools(true)
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let closing = matches!(
            event,
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
        );
        if closing {
            *control_flow = ControlFlow::Exit;
        }
    });
}

/// Persist a startup failure, because a GUI build has no console to print to.
fn report(message: &str) {
    eprintln!("{message}");
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = std::path::Path::new(&base).join("tiny-agent");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("error.log"), message);
}

fn build_html() -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html data-theme=\"");
    html.push_str(Theme::default_theme().id);
    html.push_str("\"><head><meta charset=\"utf-8\">");
    html.push_str("<title>Tiny Agent</title><style>");
    html.push_str(&theme_css());
    html.push_str(BASE_CSS);
    html.push_str("</style></head><body>");
    html.push_str(&chrome());
    html.push_str(&script());
    html.push_str("</body></html>");
    html
}

/// Scope every theme's custom properties to `[data-theme="<id>"]`.
fn theme_css() -> String {
    let mut out = String::new();
    for theme in Theme::all() {
        let mut buf = heapless::String::<4096>::new();
        if theme.emit_css(&mut buf).is_err() {
            continue;
        }
        let selector = format!("[data-theme=\"{}\"]", theme.id);
        out.push_str(&buf.as_str().replace(":root", &selector));
    }
    out
}

fn chrome() -> String {
    let mut out = String::new();
    out.push_str("<div class=\"app\"><aside class=\"sidebar\">");
    out.push_str("<div class=\"wordmark\">Tiny Agent</div>");
    out.push_str("<button class=\"primary\">New session</button>");
    out.push_str("<nav><a class=\"nav active\">Trace</a><a class=\"nav\">Diff</a>");
    out.push_str("<a class=\"nav\">Preview</a></nav>");
    out.push_str("<div class=\"spacer\"></div>");
    out.push_str("<div class=\"theme\"><span>Theme</span>");
    for theme in Theme::all() {
        out.push_str(&format!(
            "<button class=\"chip\" data-set=\"{}\">{}</button>",
            theme.id, theme.label
        ));
    }
    out.push_str("</div></aside><main class=\"main\">");
    out.push_str("<header class=\"header\"><span>Untitled session</span>");
    out.push_str("<span class=\"muted\">Tokyo Night · design tokens</span></header>");
    out.push_str("<section class=\"trace\"><p class=\"section\">OODA trace</p>");
    out.push_str(&trace_rows());
    out.push_str("</section><footer class=\"composer\">");
    out.push_str("<div class=\"input\">Ask the agent, or paste an image…</div>");
    out.push_str("<button class=\"primary\">Run</button></footer></main></div>");
    out
}

/// Rows are rendered from the same view-model the future GPUI shell would use.
fn trace_rows() -> String {
    let rows = [
        TraceRow::new(3, Phase::Act, "edit_file", RowBody::Diff { added: 12, removed: 4 }),
        TraceRow::new(4, Phase::Verify, "cargo test", RowBody::Exit { code: 0 }),
        TraceRow::new(
            5,
            Phase::Observe,
            "image_probe",
            RowBody::Image {
                format: Format::Png,
                width: 1024,
                height: 768,
            },
        ),
    ];
    let mut out = String::new();
    for row in rows {
        out.push_str(&format!(
            "<div class=\"row\"><span class=\"dot {}\"></span>\
             <span class=\"badge\">{}</span><span class=\"text\">{}</span></div>",
            row.status().label(),
            row.phase.label(),
            row.summary_buf().as_str()
        ));
    }
    out
}

fn script() -> String {
    let mut js = String::from("<script>document.querySelectorAll('[data-set]')");
    js.push_str(".forEach(function (b) { b.addEventListener('click', function () {");
    js.push_str(" document.documentElement.dataset.theme = b.dataset.set; }); });</script>");
    js
}

const BASE_CSS: &str = r#"
:root { --radius-md: 8px; --radius-lg: 12px; }
* { box-sizing: border-box; }
html, body { height: 100%; }
body {
  margin: 0;
  background: var(--background);
  color: var(--foreground);
  font-family: Inter, "Segoe UI", "Microsoft YaHei", system-ui, sans-serif;
  font-size: 13px;
  line-height: 20px;
}
.app { display: flex; height: 100vh; overflow: hidden; }
.sidebar {
  width: 240px; flex: 0 0 240px; padding: 12px; gap: 8px;
  display: flex; flex-direction: column;
  background: var(--sidebar); color: var(--sidebar-foreground);
  border-right: 1px solid var(--sidebar-border);
}
.wordmark { font-size: 16px; color: var(--foreground); margin-bottom: 4px; }
.spacer { flex: 1; }
.nav { display: block; padding: 8px 12px; border-radius: var(--radius-md); color: inherit; cursor: pointer; }
.nav:hover { background: var(--sidebar-accent); }
.nav.active { background: var(--accent); color: var(--accent-foreground); }
.primary {
  border: 1px solid var(--border); border-radius: var(--radius-md);
  background: var(--primary); color: var(--primary-foreground);
  padding: 8px 12px; font: inherit; cursor: pointer;
}
.primary:hover { filter: brightness(1.08); }
.theme { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; font-size: 12px; color: var(--muted-foreground); }
.chip {
  border: 1px solid var(--border); border-radius: var(--radius-md);
  background: var(--secondary); color: var(--secondary-foreground);
  padding: 4px 8px; font: inherit; font-size: 11px; cursor: pointer;
}
.chip:hover { background: var(--accent); color: var(--accent-foreground); }
.main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
.header {
  height: 52px; padding: 0 16px; display: flex; align-items: center;
  justify-content: space-between; border-bottom: 1px solid var(--border);
  font-size: 14px;
}
.muted { color: var(--muted-foreground); font-size: 12px; }
.trace { flex: 1; overflow: auto; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
.section { margin: 0 0 4px; font-size: 12px; color: var(--muted-foreground); }
.row {
  display: flex; align-items: center; gap: 12px; padding: 8px 12px;
  background: var(--card); color: var(--card-foreground);
  border: 1px solid var(--border); border-radius: var(--radius-lg);
}
.dot { width: 8px; height: 8px; border-radius: 9999px; flex: 0 0 8px; }
.dot.ok { background: var(--success); }
.dot.warn { background: var(--warning); }
.dot.fail { background: var(--destructive); }
.badge {
  font-size: 11px; padding: 2px 8px; border-radius: 4px;
  background: var(--secondary); color: var(--secondary-foreground);
}
.text { font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.composer {
  display: flex; gap: 8px; align-items: center; padding: 16px;
  border-top: 1px solid var(--border);
}
.input {
  flex: 1; padding: 8px 12px; border-radius: var(--radius-md);
  background: var(--muted); color: var(--muted-foreground);
}
"#;
