//! Native GPUI client for `tiny-agent-server`.
//!
//! The UI holds **no agent logic**: it posts a goal to the backend and renders
//! the Server-Sent Events that come back. That keeps the window at 60 fps even
//! while a model is thinking, because nothing on the UI thread ever blocks.
//!
//! The SSE read happens on a plain thread (blocking `ureq`); the view polls the
//! shared buffer once per frame. No async runtime in the window process.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    px, rgb, App, AppContext, Application, Bounds, Context, Entity, FocusHandle, Focusable,
    KeyDownEvent, Render, SharedString, TitlebarOptions, WeakEntity, Window, WindowBounds,
    WindowOptions,
};

/// Colours mirror the web page so both clients look like one product.
const BG: u32 = 0x1a1b26;
const FG: u32 = 0xc0caf5;
const MUTED: u32 = 0x565f89;
const ACCENT: u32 = 0x7aa2f7;
const OK: u32 = 0x9ece6a;
const CARD: u32 = 0x1f2335;

/// Event kinds the backend can emit.
#[derive(Clone)]
struct Line {
    kind: String,
    text: String,
}

/// Shared between the UI and the SSE worker thread.
#[derive(Default)]
struct Feed {
    lines: Vec<Line>,
    status: String,
    busy: bool,
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, gpui::size(px(980.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Tiny Agent".into()),
                    ..Default::default()
                }),
                app_id: Some("com.lilyco.tinyagent".into()),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(AgentView::new);
                view.read(cx).focus_handle.focus(window);
                view
            },
        )
        .expect("failed to open the Tiny Agent window");
        cx.activate(true);
    });
}

struct AgentView {
    focus_handle: FocusHandle,
    feed: Arc<Mutex<Feed>>,
    input: String,
    base: String,
    /// Bumped every poll so `render` re-reads the feed.
    tick: u64,
    _poll: gpui::Task<()>,
}

impl AgentView {
    fn new(cx: &mut Context<Self>) -> Self {
        let feed = Arc::new(Mutex::new(Feed {
            status: "后端未连接".to_string(),
            ..Default::default()
        }));
        let base = std::env::var("TINY_AGENT_SERVER")
            .unwrap_or_else(|_| "http://127.0.0.1:8787".to_string());
        let poll = Self::start_polling(cx);
        let probe_feed = Arc::clone(&feed);
        let probe_base = base.clone();
        std::thread::spawn(move || probe(&probe_base, &probe_feed));
        if std::env::var("TINY_AGENT_SELFTEST").is_ok() {
            cx.spawn(|this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    cx.background_executor()
                        .timer(Duration::from_millis(900))
                        .await;
                    let _ = this.update(&mut cx, |view, cx| {
                        view.input = "这个文件夹里有什么".to_string();
                        view.submit(cx);
                    });
                }
            })
            .detach();
        }
        Self {
            focus_handle: cx.focus_handle(),
            feed,
            input: String::new(),
            base,
            tick: 0,
            _poll: poll,
        }
    }

    /// Repaint a few times a second; the worker thread cannot touch `cx`.
    fn start_polling(cx: &mut Context<Self>) -> gpui::Task<()> {
        cx.spawn(|this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let mut cx = cx.clone();
            async move {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(120))
                        .await;
                    let alive = this.update(&mut cx, |view, cx| {
                        view.tick = view.tick.wrapping_add(1);
                        cx.notify();
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            }
        })
    }

    /// Fire one run against the backend.
    fn submit(&mut self, cx: &mut Context<Self>) {
        let goal = self.input.trim().to_string();
        if goal.is_empty() {
            return;
        }
        self.input.clear();
        let feed = Arc::clone(&self.feed);
        let base = self.base.clone();
        {
            let mut guard = feed.lock().expect("feed");
            guard.lines.clear();
            guard.lines.push(Line {
                kind: "goal".into(),
                text: format!("你说：{goal}"),
            });
            guard.busy = true;
            guard.status = "运行中…".into();
        }
        cx.notify();
        std::thread::spawn(move || stream_run(&base, &goal, &feed));
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if key == "enter" {
            self.submit(cx);
            return;
        }
        if key == "backspace" {
            self.input.pop();
            cx.notify();
            return;
        }
        if let Some(text) = event.keystroke.key_char.as_ref() {
            self.input.push_str(text);
            cx.notify();
            return;
        }
        // Fallback for plain ASCII without an IME: `key` is the character.
        if key.len() == 1 && key.is_ascii() {
            self.input.push_str(key);
            cx.notify();
        }
    }
}

impl Focusable for AgentView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AgentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (lines, status) = {
            let guard = self.feed.lock().expect("feed");
            (guard.lines.clone(), guard.status.clone())
        };
        let _ = self.tick;
        // The newest events matter most; a long run would otherwise paint past
        // the window bottom with no way to scroll back yet.
        let total = lines.len();
        let shown: Vec<Line> = lines.into_iter().skip(total.saturating_sub(22)).collect();
        let mut column = gpui::div().flex().flex_col().gap_1().w_full();
        if total > shown.len() {
            column = column.child(
                gpui::div()
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(SharedString::from(format!(
                        "… 上面还有 {} 行",
                        total - shown.len()
                    ))),
            );
        }
        for line in shown {
            column = column.child(row(line));
        }
        gpui::div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(FG))
            .text_size(px(15.))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key))
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x292e42))
                    .child(gpui::div().text_size(px(17.)).child("Tiny Agent"))
                    .child(
                        gpui::div()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .child(SharedString::from(status)),
                    ),
            )
            .child(
                gpui::div()
                    .id("log")
                    .flex_1()
                    .overflow_hidden()
                    .px_6()
                    .py_5()
                    .child(column),
            )
            .child(
                gpui::div()
                    .flex()
                    .gap_3()
                    .px_6()
                    .py_4()
                    .border_t_1()
                    .border_color(rgb(0x292e42))
                    .child(
                        gpui::div()
                            .flex_1()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(CARD))
                            .child(SharedString::from(if self.input.is_empty() {
                                "用一句话告诉我要做什么…（回车发送）".to_string()
                            } else {
                                self.input.clone()
                            })),
                    )
                    .child(
                        gpui::div()
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(ACCENT))
                            .text_color(rgb(BG))
                            .child("发送"),
                    ),
            )
    }
}

/// One rendered event.
fn row(line: Line) -> impl IntoElement {
    let color = match line.kind.as_str() {
        "goal" => ACCENT,
        "tool" => ACCENT,
        "done" => OK,
        "output" => MUTED,
        _ => FG,
    };
    let size = if line.kind == "goal" { 20. } else { 14. };
    gpui::div()
        .text_size(px(size))
        .text_color(rgb(color))
        .child(SharedString::from(line.text))
}


/// One cheap health probe so the header tells the truth before the first run.
fn probe(base: &str, feed: &Arc<Mutex<Feed>>) {
    let url = format!("{base}/api/health");
    let text = match ureq::get(&url).call() {
        Ok(resp) => resp.into_string().unwrap_or_default(),
        Err(err) => {
            let mut guard = feed.lock().expect("feed");
            guard.status = format!("后端未连接：{err}");
            return;
        }
    };
    let model = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["model"].as_str().map(str::to_string))
        .unwrap_or_else(|| "?".to_string());
    let mut guard = feed.lock().expect("feed");
    guard.status = format!("后端就绪 · 模型: {model}");
}

/// Blocking SSE reader: one POST, then line-by-line dispatch.
fn stream_run(base: &str, goal: &str, feed: &Arc<Mutex<Feed>>) {
    let url = format!("{base}/api/run");
    let body = serde_json::json!({ "goal": goal }).to_string();
    let request = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("Accept", "text/event-stream");
    let response = match request.send_string(&body) {
        Ok(response) => response,
        Err(err) => return fail(feed, &format!("连接后端失败：{err}")),
    };
    let reader = std::io::BufReader::new(response.into_reader());
    for line in std::io::BufRead::lines(reader).map_while(Result::ok) {
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload.trim()) else {
            continue;
        };
        let kind = value["kind"].as_str().unwrap_or("step").to_string();
        let text = value["text"].as_str().unwrap_or("").to_string();
        let mut guard = feed.lock().expect("feed");
        let finished = kind == "done";
        guard.lines.push(Line { kind, text });
        if finished {
            guard.busy = false;
            guard.status = "完成".to_string();
        }
    }
    let mut guard = feed.lock().expect("feed");
    guard.busy = false;
    if guard.status == "运行中…" {
        guard.status = "已结束".to_string();
    }
}

fn fail(feed: &Arc<Mutex<Feed>>, message: &str) {
    let mut guard = feed.lock().expect("feed");
    guard.busy = false;
    guard.status = "出错".to_string();
    guard.lines.push(Line {
        kind: "done".into(),
        text: message.to_string(),
    });
}

/// Keep `Entity` in scope for the `cx.new` inference in `main`.
#[allow(dead_code)]
fn _assert_entity_type(_: Entity<AgentView>) {}
