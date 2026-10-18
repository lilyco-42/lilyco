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
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use tiny_lilyco::agent::llm::{Llm, ScriptedLlm};
use tiny_lilyco::agent::registry::Registry;
use tiny_lilyco::agent::runtime::{drive, Budget, Host, Outcome};
use tiny_lilyco::agent::tool::{ObsBuf, ObsKind, Tool, ToolError};
use tiny_lilyco::agent::tools::{EditFile, ListDir, ReadFile};
use tiny_lilyco::ui::Theme;
use wry::{WebContext, WebViewBuilder};

mod providers;

/// Everything the page needs to know after a run.
#[derive(Default)]
struct Ui {
    lines: Vec<String>,
    /// `(id, label, selected)` rows for the provider list.
    providers: Option<Vec<(String, String, bool)>>,
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
    // A user event is the only reliable way to wake the loop after WebView2
    // delivers an IPC message: `MainEventsCleared` does not fire while idle.
    let event_loop: EventLoop<()> = EventLoopBuilder::<()>::with_user_event().build();
    let proxy: EventLoopProxy<()> = event_loop.create_proxy();
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
            handle_ipc(&msg, &workspace, registry, &shared);
            let _ = proxy.send_event(());
        })
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(()) => flush(&webview, &ui),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    });
}

/// Push whatever the last run produced into the page.
fn flush(webview: &wry::WebView, ui: &Arc<Mutex<Ui>>) {
    let (lines, providers) = {
        let mut guard = ui.lock().expect("ui lock");
        if !guard.dirty {
            return;
        }
        guard.dirty = false;
        (guard.lines.clone(), guard.providers.take())
    };
    let array: Vec<String> = lines
        .iter()
        .map(|l| format!("\"{}\"", escape(l)))
        .collect();
    let js = format!("window.__applyTrace([{}]);", array.join(","));
    let _ = webview.evaluate_script(&js);
    if let Some(rows) = providers {
        let json: Vec<String> = rows
            .iter()
            .map(|(id, label, on)| {
                format!("[\"{}\",\"{}\",{}]", escape(id), escape(label), on)
            })
            .collect();
        let js = format!("window.__applyProviders([{}]);", json.join(","));
        let _ = webview.evaluate_script(&js);
    }
}

/// One place for every message the page can send.
fn handle_ipc(msg: &str, workspace: &Path, registry: &'static Registry, ui: &Arc<Mutex<Ui>>) {
    let mut guard = ui.lock().expect("ui lock");
    if msg == "providers" {
        guard.providers = Some(provider_rows());
        guard.dirty = true;
        return;
    }
    if let Some(id) = msg.strip_prefix("use|") {
        let line = match providers::choose(id) {
            Ok(()) => format!("provider selected · {id}"),
            Err(e) => format!("provider select failed · {e}"),
        };
        guard.lines.push(line);
        guard.providers = Some(provider_rows());
        guard.dirty = true;
        return;
    }
    if let Some(rest) = msg.strip_prefix("save|") {
        let mut parts = rest.split('|');
        let settings = Settings {
            base: parts.next().unwrap_or("").trim().to_string(),
            key: parts.next().unwrap_or("").trim().to_string(),
            model: parts.next().unwrap_or("").trim().to_string(),
        };
        let line = match save_settings(&settings) {
            Ok(()) if settings.usable() => "设置已保存，下次开始时生效".to_string(),
            Ok(()) => "设置已保存（未填完，仍用本地模式）".to_string(),
            Err(e) => format!("保存失败：{e}"),
        };
        guard.lines.push(line);
        guard.dirty = true;
        return;
    }
    if let Some(id) = msg.strip_prefix("test|") {
        let line = test_provider(id);
        guard.lines.push(line);
        guard.providers = Some(provider_rows());
        guard.dirty = true;
        return;
    }
    if let Some(goal) = msg.strip_prefix("run|") {
        drop(guard);
        let report = execute(goal.trim(), workspace, registry, ui);
        let mut guard = ui.lock().expect("ui lock");
        guard.lines = report;
        guard.dirty = true;
    }
}

/// `(id, label, selected)` for every provider cc-switch knows.
fn provider_rows() -> Vec<(String, String, bool)> {
    let Ok(list) = providers::list() else {
        return Vec::new();
    };
    let active = providers::resolve(&list).map(|p| p.id);
    list.iter()
        .map(|p| {
            let selected = active.as_deref() == Some(p.id.as_str());
            (p.id.clone(), p.label(), selected)
        })
        .collect()
}

/// One cheap round trip against a specific provider.
fn test_provider(id: &str) -> String {
    let Ok(list) = providers::list() else {
        return "test failed · cannot read cc-switch database".to_string();
    };
    let Some(p) = list.iter().find(|p| p.id == id) else {
        return format!("test failed · unknown provider {id}");
    };
    #[cfg(feature = "llm-anthropic")]
    {
        use tiny_lilyco::agent::llm_anthropic::{probe, Config, SYSTEM};
        let config = Config {
            base_url: p.base_url.clone(),
            api_key: p.token.clone(),
            model: p.model.clone(),
            system: SYSTEM.to_string(),
        };
        return match probe(&config) {
            Ok(text) => format!("test ok · {} · {text}", p.name),
            Err(err) => format!("test failed · {} · {err}", p.name),
        };
    }
    #[allow(unreachable_code)]
    format!("test skipped · {} · built without llm-anthropic", p.name)
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
        taken: vec![format!("我在 {} 里处理这件事：{}", workspace.display(), goal)],
    };
    let (mut llm, provider) = choose_llm(registry);
    if provider.starts_with("model") {
        host.taken.push(format!("用 {provider} 来思考"));
    }
    let outcome = drive(goal, &mut *llm, registry, &mut host, Budget::new(8, 3));
    let mut lines = host.taken;
    lines.push(summary(&outcome));
    lines
}

fn summary(outcome: &Outcome) -> String {
    if outcome.failures == 0 {
        format!("做完了（{} 步）。", outcome.calls)
    } else {
        format!(
            "做完了，但有 {} 步没成功（共 {} 步）。",
            outcome.failures, outcome.calls
        )
    }
}

/// cc-switch when configured, otherwise a deterministic two-step plan.
fn choose_llm(registry: &'static Registry) -> (Box<dyn Llm>, String) {
    // Priority: what the user saved in the gear panel, then an explicit
    // environment override, then cc-switch, then the offline plan.
    let settings = load_settings();
    #[cfg(feature = "llm-http")]
    if settings.usable() && settings.openai_style() {
        use tiny_lilyco::agent::llm_http::{Config, HttpLlm};
        let config = Config {
            base_url: settings.base.clone(),
            api_key: settings.key.clone(),
            model: settings.model.clone(),
        };
        let label = format!("model · {}", settings.model);
        return (Box::new(HttpLlm::new(config, registry)), label);
    }
    #[cfg(feature = "llm-anthropic")]
    if settings.usable() {
        use tiny_lilyco::agent::llm_anthropic::{AnthropicLlm, Config, SYSTEM};
        let config = Config {
            base_url: settings.base.clone(),
            api_key: settings.key.clone(),
            model: settings.model.clone(),
            system: SYSTEM.to_string(),
        };
        let label = format!("model · {}", settings.model);
        return (Box::new(AnthropicLlm::new(config, registry)), label);
    }
    #[cfg(feature = "llm-anthropic")]
    if std::env::var("TINY_AGENT_PROVIDER").is_ok() || providers::chosen_id().is_some() {
        use tiny_lilyco::agent::llm_anthropic::{AnthropicLlm, Config, SYSTEM};
        if let Ok(list) = providers::list() {
            if let Some(p) = providers::resolve(&list) {
                let config = Config {
                    base_url: p.base_url.clone(),
                    api_key: p.token.clone(),
                    model: p.model.clone(),
                    system: SYSTEM.to_string(),
                };
                let label = format!("cc-switch · {}", p.name);
                return (Box::new(AnthropicLlm::new(config, registry)), label);
            }
        }
    }
    let _ = registry;
    let mut plan = ScriptedLlm::new();
    let _ = plan.push("list_dir", "");
    (Box::new(plan), "本地模式（不联网）".to_string())
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
            self.taken.push("改文件这件事我暂时不敢做：还没接上自动备份。".to_string());
            return Err(ToolError::NotPermitted("dry run"));
        }
        let kind = handle.call(args, out)?;
        let head = match tool {
            "list_dir" => "这个文件夹里有：".to_string(),
            "read_file" => format!("{args} 的内容："),
            _ => format!("{tool} 做完了"),
        };
        self.taken.push(head);
        for line in out.as_str().lines().take(40) {
            self.taken.push(format!("  {line}"));
        }
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

/// What the gear button stores. Empty fields mean "no model".
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Settings {
    base: String,
    key: String,
    model: String,
}

impl Settings {
    fn usable(&self) -> bool {
        !self.base.trim().is_empty() && !self.key.trim().is_empty() && !self.model.trim().is_empty()
    }

    fn openai_style(&self) -> bool {
        self.base.trim_end_matches('/').ends_with("/v1")
    }
}

fn settings_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    Path::new(&base).join("Tiny Agent").join("settings.json")
}

fn load_settings() -> Settings {
    let Ok(raw) = std::fs::read_to_string(settings_path()) else {
        return Settings::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Settings::default();
    };
    Settings {
        base: text_field(&value, "base"),
        key: text_field(&value, "key"),
        model: text_field(&value, "model"),
    }
}

fn text_field(value: &serde_json::Value, name: &str) -> String {
    value
        .get(name)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let body = serde_json::json!({
        "base": settings.base,
        "key": settings.key,
        "model": settings.model,
    })
    .to_string();
    std::fs::write(&path, body).map_err(|e| format!("cannot write settings: {e}"))
}

fn default_workspace() -> PathBuf {
    if let Ok(explicit) = std::env::var("TINY_AGENT_WORKSPACE") {
        return PathBuf::from(explicit);
    }
    // Launched from the Start Menu the CWD is System32, which is useless as a
    // workspace; the home directory at least contains the user's own files.
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    home.map(PathBuf::from)
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
    out.push_str("<div class=\"app\"><header class=\"top\">");
    out.push_str("<div class=\"brand\">Tiny Agent</div>");
    out.push_str("<button class=\"gear\" data-action=\"settings\">设置</button>");
    out.push_str("</header><main class=\"body\"><div class=\"answer\" id=\"answer\">");
    out.push_str("<p class=\"line big\">你想让我做什么？</p>");
    out.push_str("<p class=\"line\">比如：这个文件夹里有什么 &middot; 看看 README 写了什么 &middot; 列一下文件</p>");
    out.push_str(&format!(
        "<p class=\"line small\">当前文件夹：{}</p>",
        workspace.display()
    ));
    out.push_str("</div></main>");
    out.push_str("<footer class=\"bar\">");
    out.push_str("<input id=\"prompt\" class=\"ask\" placeholder=\"用一句话告诉我要做什么…\" autofocus>");
    out.push_str("<button id=\"go\" class=\"go\" data-action=\"run\">开始</button>");
    out.push_str("</footer>");
    out.push_str(&sheet());
    out.push_str("</div>");
    out
}

fn sheet() -> String {
    let mut out = String::new();
    out.push_str("<div class=\"sheet hidden\" id=\"sheet\">");
    out.push_str("<h3>模型设置（可选）</h3>");
    out.push_str("<p class=\"hint\">不填也能用：只做本地文件操作，不联网、不花钱。</p>");
    out.push_str("<label>接口地址<input id=\"base\" placeholder=\"https://openrouter.ai/api/v1\"></label>");
    out.push_str("<label>API Key<input id=\"key\" type=\"password\" placeholder=\"sk-...\"></label>");
    out.push_str("<label>模型<input id=\"model\" placeholder=\"例如 deepseek-chat\"></label>");
    out.push_str("<p class=\"hint\">地址以 /v1 结尾按 OpenAI 协议，否则按 Anthropic 协议。</p>");
    out.push_str("<div class=\"sheet-row\"><button class=\"go\" data-action=\"save\">保存</button>");
    out.push_str("<button class=\"ghost\" data-action=\"close\">关闭</button></div>");
    out.push_str("</div>");
    out
}

fn script() -> String {
    let mut js = String::from("<script>");
    js.push_str("function post(m){ if (window.ipc) { window.ipc.postMessage(m); } }");
    js.push_str("function go(){ var i=document.getElementById('prompt');");
    js.push_str(" var v=(i.value||'').trim(); if(!v){ i.focus(); return; }");
    js.push_str(" var b=document.getElementById('go'); b.textContent='正在做…'; b.disabled=true;");
    js.push_str(" document.getElementById('answer').innerHTML='<p class=\"line\">正在处理…</p>';");
    js.push_str(" post('run|'+v); i.value=''; }");
    js.push_str("document.querySelector('[data-action=\"run\"]').addEventListener('click', go);");
    js.push_str("document.getElementById('prompt').addEventListener('keydown', function(e){");
    js.push_str(" if(e.key==='Enter'){ go(); } });");
    js.push_str("document.querySelector('[data-action=\"settings\"]').addEventListener('click',");
    js.push_str(" function(){ document.getElementById('sheet').classList.remove('hidden'); });");
    js.push_str("document.querySelector('[data-action=\"close\"]').addEventListener('click',");
    js.push_str(" function(){ document.getElementById('sheet').classList.add('hidden'); });");
    js.push_str("document.querySelector('[data-action=\"save\"]').addEventListener('click', function(){");
    js.push_str(" var b=document.getElementById('base').value||'';");
    js.push_str(" var k=document.getElementById('key').value||'';");
    js.push_str(" var m=document.getElementById('model').value||'';");
    js.push_str(" post('save|'+b+'|'+k+'|'+m);");
    js.push_str(" document.getElementById('sheet').classList.add('hidden'); });");
    js.push_str("window.__applyTrace = function (lines) {");
    js.push_str(" var box=document.getElementById('answer'); box.innerHTML='';");
    js.push_str(" lines.forEach(function(t){ var p=document.createElement('p'); p.className='line';");
    js.push_str(" p.textContent=String(t).replace(/^\\s+/,''); box.appendChild(p); });");
    js.push_str(" document.querySelector('.body').scrollTop = 0;");
    js.push_str(" var b=document.getElementById('go'); b.textContent='开始'; b.disabled=false; };");
    js.push_str("window.__applyProviders = function(){};");
    js.push_str("if(window.__SELFTEST){ setTimeout(function(){");
    js.push_str(" document.getElementById('prompt').value='这个文件夹里有什么'; go(); }, 1200); }");
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
:root { --radius-md: 10px; --radius-lg: 14px; }
* { box-sizing: border-box; }
html, body { height: 100%; }
body {
  margin: 0; background: var(--background); color: var(--foreground);
  font-family: "Segoe UI", "Microsoft YaHei", Inter, system-ui, sans-serif;
  font-size: 15px; line-height: 24px;
}
.app { display: flex; flex-direction: column; height: 100vh; }
.top {
  display: flex; align-items: center; justify-content: space-between;
  padding: 14px 22px; border-bottom: 1px solid var(--border);
}
.brand { font-size: 17px; font-weight: 600; letter-spacing: .2px; }
.gear {
  border: 1px solid var(--border); background: var(--secondary); color: var(--secondary-foreground);
  border-radius: var(--radius-md); padding: 6px 14px; font: inherit; font-size: 13px; cursor: pointer;
}
.gear:hover { background: var(--accent); color: var(--accent-foreground); }
.body { flex: 1; overflow: auto; padding: 40px 22px; display: flex; justify-content: center; }
.answer { width: 100%; max-width: 760px; }
.line { margin: 0 0 10px; color: var(--foreground); word-break: break-word; }
.line.big { font-size: 24px; font-weight: 600; margin-bottom: 6px; }
.line.small { font-size: 13px; color: var(--muted-foreground); }
.bar {
  display: flex; gap: 10px; padding: 16px 22px 22px;
  border-top: 1px solid var(--border); justify-content: center;
}
.ask {
  flex: 1; max-width: 640px; padding: 12px 16px; font: inherit;
  border-radius: var(--radius-md); border: 1px solid var(--border);
  background: var(--card); color: var(--foreground); outline: none;
}
.ask:focus { box-shadow: 0 0 0 2px var(--ring); }
.go {
  padding: 12px 26px; font: inherit; font-weight: 600; cursor: pointer;
  border-radius: var(--radius-md); border: 1px solid var(--border);
  background: var(--primary); color: var(--primary-foreground);
}
.go:disabled { opacity: .6; cursor: default; }
.ghost {
  padding: 12px 20px; font: inherit; cursor: pointer;
  border-radius: var(--radius-md); border: 1px solid var(--border);
  background: transparent; color: var(--foreground);
}
.sheet {
  position: fixed; inset: 0; margin: auto; width: min(520px, 92vw); height: fit-content;
  background: var(--card); color: var(--card-foreground);
  border: 1px solid var(--border); border-radius: var(--radius-lg);
  padding: 24px; display: flex; flex-direction: column; gap: 12px;
  box-shadow: 0 20px 60px rgba(0,0,0,.35);
}
.sheet.hidden { display: none; }
.sheet h3 { margin: 0; font-size: 18px; }
.hint { margin: 0; font-size: 13px; color: var(--muted-foreground); }
.sheet label { display: flex; flex-direction: column; gap: 6px; font-size: 13px; color: var(--muted-foreground); }
.sheet input {
  padding: 10px 12px; font: inherit; border-radius: var(--radius-md);
  border: 1px solid var(--border); background: var(--muted); color: var(--foreground); outline: none;
}
.sheet input:focus { box-shadow: 0 0 0 2px var(--ring); }
.sheet-row { display: flex; gap: 10px; justify-content: flex-end; margin-top: 4px; }
"#;
