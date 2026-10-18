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
use wry::{WebContext, WebViewBuilder};

fn main() {
    declare_dpi_awareness();
    if let Err(err) = run() {
        report(&format!("Tiny Agent failed to start: {err}"));
    }
}

/// Without this the process is DPI-unaware: `tao` creates a 1100x720 *physical*
/// window while WebView2 renders at the monitor scale (125%), so the page is
/// magnified by 1.25 and its right/bottom edges get clipped.
fn declare_dpi_awareness() {
    // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 == -4
    const PER_MONITOR_V2: isize = -4;
    #[link(name = "user32")]
    extern "system" {
        fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    }
    // Fails harmlessly when a manifest already declared awareness.
    unsafe {
        SetProcessDpiAwarenessContext(PER_MONITOR_V2);
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
    let layout_note = fit_to_webview_scale(&window);
    trace(&layout_note);

    // WebView2 must NOT put its user-data folder under Program Files: the
    // install directory is read-only for a per-machine MSI, which fails with
    // E_ACCESSDENIED. Point it at %LOCALAPPDATA% instead.
    let mut context = WebContext::new(Some(webview_data_dir()));
    let _webview = WebViewBuilder::new_with_web_context(&mut context)
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

/// Grow the window so its *physical* size matches the CSS pixels WebView2 renders.
///
/// `tao` reports a 1.0 scale factor here while WebView2 renders at the monitor's
/// real scale (125%), which leaves the page 25% wider than the window and clips
/// its right edge and the composer. Two details matter:
///
/// * ask the OS for the DPI instead of trusting `tao`'s reading;
/// * resize through `SetWindowPos`, because `set_inner_size` before the event
///   loop starts is dropped.
fn fit_to_webview_scale(window: &tao::window::Window) -> String {
    let scale = monitor_scale(window);
    let before = window.inner_size();
    let want_w = (1100.0 * scale).round() as u32;
    let want_h = (720.0 * scale).round() as u32;
    if before.width >= want_w {
        return format!("dpi x{scale} kept {}x{}", before.width, before.height);
    }
    resize_physical(window, want_w, want_h);
    let after = window.inner_size();
    format!(
        "dpi x{scale} resized {}x{} -> {}x{}",
        before.width, before.height, after.width, after.height
    )
}

#[cfg(windows)]
fn resize_physical(window: &tao::window::Window, w: u32, h: u32) {
    #[link(name = "user32")]
    extern "system" {
        fn SetWindowPos(h: isize, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    use tao::platform::windows::WindowExtWindows;
    // SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
    const FLAGS: u32 = 0x0002 | 0x0004 | 0x0010;
    let hwnd = window.hwnd() as isize;
    unsafe {
        SetWindowPos(hwnd, 0, 0, 0, w as i32, h as i32, FLAGS);
    }
}

#[cfg(not(windows))]
fn resize_physical(window: &tao::window::Window, w: u32, h: u32) {
    window.set_inner_size(tao::dpi::PhysicalSize::new(w, h));
}

/// Real per-window DPI scale, bypassing `tao`'s own (unreliable here) reading.
#[cfg(windows)]
fn monitor_scale(window: &tao::window::Window) -> f64 {
    #[link(name = "user32")]
    extern "system" {
        fn GetDpiForWindow(hwnd: isize) -> u32;
    }
    use tao::platform::windows::WindowExtWindows;
    let hwnd = window.hwnd() as isize;
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        1.0
    } else {
        f64::from(dpi) / 96.0
    }
}

#[cfg(not(windows))]
fn monitor_scale(window: &tao::window::Window) -> f64 {
    window.scale_factor()
}

/// A per-user, always-writable directory for the WebView2 profile.
fn webview_data_dir() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = std::path::Path::new(&base).join("Tiny Agent").join("webview2");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Append a line to `%LOCALAPPDATA%\tiny-agent\startup.log`.
fn trace(message: &str) {
    use std::io::Write as _;
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = std::path::Path::new(&base).join("tiny-agent");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("startup.log");
    let opened = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path);
    if let Ok(mut file) = opened {
        let _ = file.write_all(format!("{message}\n").as_bytes());
    }
}

/// Persist and show a startup failure: a GUI build has no console to print to.
fn report(message: &str) {
    eprintln!("{message}");
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = std::path::Path::new(&base).join("tiny-agent");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("error.log"), message);
    popup(message);
}

/// A dependency-free message box, so a failed launch is never silent.
fn popup(message: &str) {
    let script = "[void][System.Reflection.Assembly]::LoadWithPartialName(\
                  'PresentationFramework'); \
                  [System.Windows.MessageBox]::Show($env:TINY_AGENT_ERROR, 'Tiny Agent')";
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script])
        .env("TINY_AGENT_ERROR", message)
        .spawn();
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
