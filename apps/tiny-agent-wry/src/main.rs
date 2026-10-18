//! Tiny Agent — a desktop window that actually runs the agent loop.
//!
//! `tao` owns the window, `wry` renders the document, and the page talks to
//! Rust over IPC: pressing **Run** sends the goal, Rust executes the loop in the
//! chosen workspace, and every step is pushed back into the trace.
//!
//! Provider selection is automatic:
//!   1. cc-switch's active provider (when its settings files exist),
//!   2. otherwise a deterministic scripted plan, so the app is useful with no
//!      key at all — it will list the folder and read one file.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use tiny_lilyco::adapters::image_inspect::Format;
use tiny_lilyco::agent::llm::{Llm, ScriptedLlm};
use tiny_lilyco::agent::registry::Registry;
use tiny_lilyco::agent::runtime::{drive, Budget, Host, Outcome};
use tiny_lilyco::agent::tool::{ObsBuf, ObsKind, Tool, ToolError};
use tiny_lilyco::agent::tools::{EditFile, ListDir, ReadFile};
use tiny_lilyco::agent::Phase;
use tiny_lilyco::ui::{RowBody, Theme, TraceRow};
use wry::{WebContext, WebViewBuilder};

/// Everything the page needs to know after a run.
#[derive(Default)]
struct Ui {
    lines: Vec<String>,
    dirty: bool,
}

fn main() {
    declare_dpi_awareness();
    if let Err(err) = run() {
        report(&format!("Tiny Agent failed to start: {err}"));
    }
}

fn declare_dpi_awareness() {
    const PER_MONITOR_V2: isize = -4;
    #[link(name = "user32")]
    extern "system" {
        fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    }
    unsafe {
        SetProcessDpiAwarenessContext(PER_MONITOR_V2);
    }
}

fn run() -> wry::Result<()> {
    let event_loop = EventLoop::new();
    let workspace = default_workspace();
    let registry = build_registry(&workspace);
    let ui = Arc::new(Mutex::new(Ui::default()));
    let shared = Arc::clone(&ui);

    let window = WindowBuilder::new()
        .with_title("Tiny Agent")
        .with_inner_size(LogicalSize::new(1100.0, 720.0))
        .with_min_inner_size(LogicalSize::new(880.0, 560.0))
        .build(&event_loop)
        .expect("failed to create the native window");
    let layout_note = fit_to_webview_scale(&window);
    trace(&layout_note);

    let mut context = WebContext::new(Some(webview_data_dir()));
    let html = build_html(selftest_requested(), &workspace);
    dump_html(&html);
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_html(html)
        .with_devtools(true)
        .with_ipc_handler(move |req| {
            let msg = req.body().clone();
            trace(&format!("ipc: {msg}"));
            let Some(goal) = msg.strip_prefix("run|") else {
                return;
            };
            let report = execute(goal.trim(), &workspace, registry, &shared);
            let mut guard = shared.lock().expect("ui lock");
            guard.lines = report;
            guard.dirty = true;
        })
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if matches!(event, Event::MainEventsCleared) {
            flush(&webview, &ui);
            return;
        }
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

/// Push whatever the last run produced into the page.
fn flush(webview: &wry::WebView, ui: &Arc<Mutex<Ui>>) {
    let lines = {
        let mut guard = ui.lock().expect("ui lock");
        if !guard.dirty {
            return;
        }
        guard.dirty = false;
        guard.lines.clone()
    };
    let array: Vec<String> = lines
        .iter()
        .map(|l| format!("\"{}\"", escape(l)))
        .collect();
    let js = format!("window.__applyTrace([{}]);", array.join(","));
    let _ = webview.evaluate_script(&js);
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("  "),
            c => out.push(c),
        }
    }
    out
}

/// Run one goal and return the trace lines.
fn execute(
    goal: &str,
    workspace: &Path,
    registry: &'static Registry,
    ui: &Arc<Mutex<Ui>>,
) -> Vec<String> {
    let mut host = UiHost {
        registry,
        apply: false,
        lines: Arc::clone(ui),
        taken: vec![format!("goal · {goal}"), format!("workspace · {}", workspace.display())],
    };
    let (mut llm, provider) = choose_llm(registry);
    host.taken.push(format!("provider · {provider}"));
    let outcome = drive(goal, &mut *llm, registry, &mut host, Budget::new(8, 3));
    let mut lines = host.taken;
    lines.push(summary(&outcome));
    lines
}

fn summary(outcome: &Outcome) -> String {
    format!(
        "— {} calls · {} failures · {} checkpoints · rolled_back={} · {}",
        outcome.calls, outcome.failures, outcome.checkpoints, outcome.rolled_back, outcome.phase
    )
}

/// cc-switch when configured, otherwise a deterministic two-step plan.
fn choose_llm(registry: &'static Registry) -> (Box<dyn Llm>, &'static str) {
    // A real model is opt-in: pointing at a dead provider would block the window
    // for the whole HTTP timeout, and the offline plan is genuinely useful.
    let wants_model = std::env::var("TINY_AGENT_PROVIDER")
        .map(|v| v.eq_ignore_ascii_case("ccswitch"))
        .unwrap_or(false);
    #[cfg(feature = "llm-anthropic")]
    if wants_model {
        use tiny_lilyco::agent::ccswitch;
        use tiny_lilyco::agent::llm_anthropic::{AnthropicLlm, Config, SYSTEM};
        if let Ok(active) = ccswitch::load() {
            let config = Config {
                base_url: active.base_url,
                api_key: active.api_key,
                model: active.model,
                system: SYSTEM.to_string(),
            };
            return (Box::new(AnthropicLlm::new(config, registry)), "cc-switch");
        }
    }
    let mut plan = ScriptedLlm::new();
    let _ = plan.push("list_dir", "");
    let _ = plan.push("read_file", "README.md");
    (Box::new(plan), "offline plan")
}

struct UiHost {
    registry: &'static Registry,
    apply: bool,
    lines: Arc<Mutex<Ui>>,
    taken: Vec<String>,
}

impl Host for UiHost {
    fn checkpoint(&mut self, label: &str) -> Result<(), &'static str> {
        self.taken.push(format!("checkpoint · {label}"));
        Ok(())
    }

    fn undo(&mut self) -> Result<(), &'static str> {
        self.taken.push("rollback".to_string());
        Ok(())
    }

    fn call(&mut self, tool: &str, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let handle = self.registry.find(tool).ok_or(ToolError::UnknownTool)?;
        if handle.spec().mutating && !self.apply {
            self.taken.push(format!("{tool}({args}) → refused (dry run)"));
            return Err(ToolError::NotPermitted("dry run"));
        }
        let kind = handle.call(args, out)?;
        self.taken.push(format!("{tool}({args}) → {}", kind.label()));
        for line in out.as_str().lines().take(12) {
            self.taken.push(format!("   {line}"));
        }
        self.taken.push(String::new());
        let _ = &self.lines;
        Ok(kind)
    }
}

fn build_registry(root: &Path) -> &'static Registry {
    let read: &'static dyn Tool = Box::leak(Box::new(ReadFile::new(root)));
    let list: &'static dyn Tool = Box::leak(Box::new(ListDir::new(root)));
    let edit: &'static dyn Tool = Box::leak(Box::new(EditFile::new(root)));
    let reg: &'static mut Registry = Box::leak(Box::new(Registry::new()));
    let _ = reg.register(read);
    let _ = reg.register(list);
    let _ = reg.register(edit);
    reg
}

fn default_workspace() -> PathBuf {
    std::env::var("TINY_AGENT_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn selftest_requested() -> bool {
    std::env::var("TINY_AGENT_SELFTEST")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn webview_data_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = Path::new(&base).join("Tiny Agent").join("webview2");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn trace(message: &str) {
    use std::io::Write as _;
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = Path::new(&base).join("tiny-agent");
    let _ = std::fs::create_dir_all(&dir);
    let opened = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("startup.log"));
    if let Ok(mut file) = opened {
        let _ = file.write_all(format!("{message}\n").as_bytes());
    }
}

fn report(message: &str) {
    trace(message);
    eprintln!("{message}");
    let script = "[void][System.Reflection.Assembly]::LoadWithPartialName(\
                  'PresentationFramework'); \
                  [System.Windows.MessageBox]::Show($env:TINY_AGENT_ERROR, 'Tiny Agent')";
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script])
        .env("TINY_AGENT_ERROR", message)
        .spawn();
}

fn dump_html(html: &str) {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = Path::new(&base).join("tiny-agent");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("ui.html"), html);
}

fn build_html(selftest: bool, workspace: &Path) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html data-theme=\"");
    html.push_str(Theme::default_theme().id);
    html.push_str("\"><head><meta charset=\"utf-8\"><title>Tiny Agent</title><style>");
    html.push_str(&theme_css());
    html.push_str(BASE_CSS);
    html.push_str("</style></head><body>");
    if selftest {
        html.push_str("<script>window.__SELFTEST = true;</script>");
    }
    html.push_str(&chrome(workspace));
    html.push_str(&script());
    html.push_str("</body></html>");
    html
}

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

fn chrome(workspace: &Path) -> String {
    let mut out = String::new();
    out.push_str("<div class=\"app\"><aside class=\"sidebar\">");
    out.push_str("<div class=\"wordmark\">Tiny Agent</div>");
    out.push_str("<button class=\"primary\" data-action=\"new\">New session</button>");
    out.push_str("<nav><a class=\"nav active\" data-view=\"trace\">Trace</a>");
    out.push_str("<a class=\"nav\" data-view=\"diff\">Diff</a>");
    out.push_str("<a class=\"nav\" data-view=\"preview\">Preview</a></nav>");
    out.push_str("<div class=\"spacer\"></div>");
    out.push_str("<div class=\"theme\"><span>Theme</span>");
    for theme in Theme::all() {
        out.push_str(&format!(
            "<button class=\"chip\" data-set=\"{}\" data-label=\"{}\">{}</button>",
            theme.id, theme.label, theme.label
        ));
    }
    out.push_str("</div></aside><main class=\"main\">");
    out.push_str("<header class=\"header\"><span id=\"title\">Ready</span>");
    out.push_str("<span class=\"muted\" id=\"theme-label\">Tokyo Night · design tokens</span></header>");
    out.push_str(&views(workspace));
    out.push_str("<footer class=\"composer\">");
    out.push_str("<input id=\"prompt\" class=\"input\" placeholder=\"Tell the agent what to do, then press Enter\">");
    out.push_str("<button class=\"primary\" data-action=\"run\">Run</button>");
    out.push_str("</footer></main></div>");
    out
}

fn views(workspace: &Path) -> String {
    let mut out = String::new();
    out.push_str("<section class=\"view active\" id=\"view-trace\">");
    out.push_str("<p class=\"section\">OODA trace</p><div id=\"rows\">");
    out.push_str(&trace_rows());
    out.push_str("</div></section>");
    out.push_str("<section class=\"view\" id=\"view-diff\">");
    out.push_str("<p class=\"section\">Last edit</p>");
    out.push_str("<pre class=\"card mono\">--- src/ui/theme.rs\n+++ src/ui/theme.rs\n");
    out.push_str("@@ -12,6 +12,8 @@\n+    pub const fn tokyo_night() -&gt; Self {\n");
    out.push_str("+        Palette::night()\n-    pub const fn dark() -&gt; Self\n</pre></section>");
    out.push_str("<section class=\"view\" id=\"view-preview\"><p class=\"section\">Preview</p>");
    out.push_str("<div class=\"card\">image_probe · png 1024x768 → 1568x1176</div></section>");
    out.push_str(&format!(
        "<p class=\"section\">workspace · {}</p>",
        workspace.display()
    ));
    out
}

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
    out.push_str("<p class=\"section\">how it works · your goal &rarr; model &rarr; one tool &rarr; observation &rarr; repeat</p>");
    for row in rows {
        out.push_str(&format!(
            "<div class=\"row\"><span class=\"dot {}\"></span><span class=\"badge\">{}</span>\
             <span class=\"text\">{}</span></div>",
            row.status().label(),
            row.phase.label(),
            row.summary_buf().as_str()
        ));
    }
    out
}

fn script() -> String {
    let mut js = String::from("<script>");
    js.push_str("function post(m){ if (window.ipc) { window.ipc.postMessage(m); } else { document.getElementById('title').textContent = 'no ipc'; } }");
    js.push_str("function setTheme(id, label){ document.documentElement.dataset.theme = id;");
    js.push_str(" document.getElementById('theme-label').textContent = label + ' \u{00b7} design tokens'; }");
    js.push_str("document.querySelectorAll('[data-set]').forEach(function (b) {");
    js.push_str(" b.addEventListener('click', function () { setTheme(b.dataset.set, b.dataset.label); }); });");
    js.push_str("document.querySelectorAll('[data-view]').forEach(function (a) {");
    js.push_str(" a.addEventListener('click', function () {");
    js.push_str("  document.querySelectorAll('.nav').forEach(function (x) { x.classList.remove('active'); });");
    js.push_str("  a.classList.add('active');");
    js.push_str("  document.querySelectorAll('.view').forEach(function (v) { v.classList.remove('active'); });");
    js.push_str("  document.getElementById('view-' + a.dataset.view).classList.add('active'); }); });");
    js.push_str("window.__applyTrace = function (lines) {");
    js.push_str(" var rows = document.getElementById('rows'); rows.innerHTML = '';");
    js.push_str(" lines.forEach(function (t, i) {");
    js.push_str("  var d = document.createElement('div'); d.className = 'row';");
    js.push_str("  var dot = document.createElement('span');");
    js.push_str("  dot.className = 'dot ' + (t.indexOf('\\u2192') > 0 ? 'ok' : 'warn');");
    js.push_str("  var badge = document.createElement('span'); badge.className = 'badge';");
    js.push_str("  badge.textContent = String(i + 1);");
    js.push_str("  var text = document.createElement('span'); text.className = 'text'; text.textContent = t;");
    js.push_str("  d.appendChild(dot); d.appendChild(badge); d.appendChild(text); rows.appendChild(d); });");
    js.push_str(" document.getElementById('title').textContent = 'Done \\u00b7 ' + lines.length + ' lines'; };");
    js.push_str("function run() {");
    js.push_str(" var i = document.getElementById('prompt'); var v = (i.value || '').trim();");
    js.push_str(" if (!v) { v = 'list the workspace'; }");
    js.push_str(" document.getElementById('title').textContent = 'Running\\u2026';");
    js.push_str(" post('run|' + v); i.value = ''; }");
    js.push_str("document.querySelector('[data-action=\"run\"]').addEventListener('click', run);");
    js.push_str("document.getElementById('prompt').addEventListener('keydown', function (e) {");
    js.push_str(" if (e.key === 'Enter') { run(); } });");
    js.push_str("document.querySelector('[data-action=\"new\"]').addEventListener('click', function () {");
    js.push_str(" document.getElementById('rows').innerHTML = '';");
    js.push_str(" document.getElementById('title').textContent = 'Ready'; });");
    js.push_str("post('boot');");
    js.push_str("if (window.__SELFTEST) { setTimeout(function () { run(); }, 1200); }");
    js.push_str("</script>");
    js
}

fn fit_to_webview_scale(window: &tao::window::Window) -> String {
    let scale = monitor_scale(window);
    let (cw, ch) = client_size(window);
    let want_w = (1100.0 * scale).round() as u32;
    let want_h = (720.0 * scale).round() as u32;
    if cw >= want_w && ch >= want_h {
        return format!("dpi x{scale} ok {cw}x{ch}");
    }
    resize_physical(window, want_w, want_h);
    let (aw, ah) = client_size(window);
    format!("dpi x{scale} {cw}x{ch} -> {aw}x{ah}")
}

#[cfg(windows)]
fn client_size(window: &tao::window::Window) -> (u32, u32) {
    #[link(name = "user32")]
    extern "system" {
        fn GetClientRect(hwnd: isize, rect: *mut RECT) -> i32;
    }
    #[repr(C)]
    struct RECT {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    use tao::platform::windows::WindowExtWindows;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let hwnd = window.hwnd() as isize;
    if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
        return (0, 0);
    }
    (
        (rect.right - rect.left).max(0) as u32,
        (rect.bottom - rect.top).max(0) as u32,
    )
}

#[cfg(not(windows))]
fn client_size(window: &tao::window::Window) -> (u32, u32) {
    let s = window.inner_size();
    (s.width, s.height)
}

#[cfg(windows)]
fn resize_physical(window: &tao::window::Window, w: u32, h: u32) {
    #[link(name = "user32")]
    extern "system" {
        fn SetWindowPos(h: isize, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    use tao::platform::windows::WindowExtWindows;
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

#[cfg(windows)]
fn monitor_scale(window: &tao::window::Window) -> f64 {
    #[link(name = "user32")]
    extern "system" {
        fn GetDpiForWindow(hwnd: isize) -> u32;
    }
    use tao::platform::windows::WindowExtWindows;
    let dpi = unsafe { GetDpiForWindow(window.hwnd() as isize) };
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

const BASE_CSS: &str = r#"
:root { --radius-md: 8px; --radius-lg: 12px; }
* { box-sizing: border-box; }
html, body { height: 100%; }
body {
  margin: 0; background: var(--background); color: var(--foreground);
  font-family: Inter, "Segoe UI", "Microsoft YaHei", system-ui, sans-serif;
  font-size: 13px; line-height: 20px;
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
.nav { display: block; padding: 8px 12px; border-radius: var(--radius-md); cursor: pointer; }
.nav:hover { background: var(--sidebar-accent); }
.nav.active { background: var(--accent); color: var(--accent-foreground); }
.primary {
  border: 1px solid var(--border); border-radius: var(--radius-md);
  background: var(--primary); color: var(--primary-foreground);
  padding: 8px 12px; font: inherit; cursor: pointer;
}
.theme { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; font-size: 12px; color: var(--muted-foreground); }
.chip {
  border: 1px solid var(--border); border-radius: var(--radius-md);
  background: var(--secondary); color: var(--secondary-foreground);
  padding: 4px 8px; font: inherit; font-size: 11px; cursor: pointer;
}
.main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
.header {
  height: 52px; padding: 0 16px; display: flex; align-items: center;
  justify-content: space-between; border-bottom: 1px solid var(--border); font-size: 14px;
}
.muted { color: var(--muted-foreground); font-size: 12px; }
.view { display: none; }
.view.active {
  display: flex; flex-direction: column; flex: 1;
  overflow: auto; padding: 16px; gap: 8px;
}
.card {
  background: var(--card); color: var(--card-foreground);
  border: 1px solid var(--border); border-radius: var(--radius-lg); padding: 12px;
}
.mono { font-family: Consolas, "Cascadia Mono", monospace; font-size: 12px; white-space: pre; margin: 0; }
.section { margin: 0 0 4px; font-size: 12px; color: var(--muted-foreground); }
.row {
  display: flex; align-items: center; gap: 12px; padding: 8px 12px;
  background: var(--card); color: var(--card-foreground);
  border: 1px solid var(--border); border-radius: var(--radius-lg);
}
.dot { width: 8px; height: 8px; border-radius: 9999px; flex: 0 0 8px; }
.dot.ok { background: var(--success); }
.dot.warn { background: var(--warning); }
.badge {
  font-size: 11px; padding: 2px 8px; border-radius: 4px;
  background: var(--secondary); color: var(--secondary-foreground); flex: 0 0 auto;
}
.text { font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: pre; }
.composer { display: flex; gap: 8px; align-items: center; padding: 16px; border-top: 1px solid var(--border); }
.input {
  flex: 1; padding: 8px 12px; border-radius: var(--radius-md);
  background: var(--muted); color: var(--foreground);
  border: none; outline: none; font: inherit;
}
.input:focus { box-shadow: 0 0 0 2px var(--ring); }
"#;
