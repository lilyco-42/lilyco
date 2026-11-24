//! GTK3 悬浮窗口：透明、无边框、置顶、可拖动；由 `FloatSpec` + `Feed` 驱动。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use gtk::cairo;
use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::feed::{Feed, FloatItem};
use crate::spec::FloatSpec;

const FPS_MS: u64 = 16;

struct Flight {
    text: String,
    x: f64,
    lane: usize,
}

struct State {
    spec: FloatSpec,
    flights: Vec<Flight>,
    drag: Option<(f64, f64, i32, i32)>,
}

/// 拉起悬浮层（阻塞在 GTK 主循环；关闭窗口即退出）。
pub fn run(spec: FloatSpec, feed: impl Feed) -> Result<()> {
    let spec = spec.normalized();

    // GTK3 在 Wayland 会话下默认走 Wayland backend，KWin 会忽略 set_decorated(false)；
    // 走 XWayland 才能用 _MOTIF_WM_HINTS + Dock 类型做真正的无边框悬浮层。
    if std::env::var_os("GDK_BACKEND").is_none() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
    gtk::init().map_err(|e| anyhow!("GTK 初始化失败: {e}"))?;

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_decorated(false);
    window.set_resizable(false);
    window.set_keep_above(true);
    window.set_skip_taskbar_hint(true);
    window.set_skip_pager_hint(true);
    window.set_accept_focus(true);
    window.set_type_hint(gdk::WindowTypeHint::Dock);
    window.stick();
    window.set_default_size(spec.width, spec.height());
    window.set_position(gtk::WindowPosition::Center);

    if let Some(screen) = gtk::prelude::GtkWindowExt::screen(&window) {
        if let Some(visual) = screen.rgba_visual() {
            window.set_visual(Some(&visual));
        }
    }
    window.set_app_paintable(true);
    window.connect_draw(|_, cr| {
        cr.set_operator(cairo::Operator::Source);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();
        glib::Propagation::Proceed
    });

    let area = gtk::DrawingArea::new();
    window.add(&area);

    let state = Rc::new(RefCell::new(State {
        spec: spec.clone(),
        flights: Vec::new(),
        drag: None,
    }));

    // 后台：按 Feed 的节奏收条目
    let (tx, rx) = mpsc::channel::<Vec<FloatItem>>();
    let mut feed = feed;
    thread::spawn(move || loop {
        let items = feed.poll();
        if !items.is_empty() && tx.send(items).is_err() {
            return;
        }
    });

    {
        let state = state.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            while let Ok(items) = rx.try_recv() {
                let mut st = state.borrow_mut();
                for item in items {
                    let lane = st.flights.len() % st.spec.lanes;
                    let start_x = st.spec.width as f64;
                    st.flights.push(Flight {
                        text: item.text,
                        x: start_x,
                        lane,
                    });
                }
                if st.flights.len() > st.spec.max_items {
                    let overflow = st.flights.len() - st.spec.max_items;
                    st.flights.drain(0..overflow);
                }
            }
            glib::ControlFlow::Continue
        });
    }

    {
        let state = state.clone();
        area.connect_draw(move |_, cr| {
            let st = state.borrow();
            cr.set_operator(cairo::Operator::Source);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            let _ = cr.paint();
            cr.set_operator(cairo::Operator::Over);

            for flight in &st.flights {
                let y = (flight.lane as i32 * st.spec.lane_height + st.spec.lane_height - 8) as f64;
                cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                cr.set_font_size(st.spec.font_size);
                cr.move_to(flight.x, y);
                if st.spec.stroke > 0.0 {
                    cr.set_line_width(st.spec.stroke);
                    cr.set_source_rgba(0.0, 0.0, 0.0, 0.85);
                    let _ = cr.text_path(&flight.text);
                    let _ = cr.stroke_preserve();
                }
                let (r, g, b) = st.spec.color;
                cr.set_source_rgba(r, g, b, st.spec.opacity);
                let _ = cr.fill();
            }
            glib::Propagation::Proceed
        });
    }

    {
        let state = state.clone();
        let area = area.clone();
        glib::timeout_add_local(Duration::from_millis(FPS_MS), move || {
            {
                let mut st = state.borrow_mut();
                let speed = st.spec.speed;
                for flight in st.flights.iter_mut() {
                    flight.x -= speed;
                }
                st.flights.retain(|f| f.x > -800.0);
            }
            area.queue_draw();
            glib::ControlFlow::Continue
        });
    }

    {
        let state = state.clone();
        area.connect_size_allocate(move |_, alloc| {
            state.borrow_mut().spec.width = alloc.width();
        });
    }

    // 拖动
    {
        let state = state.clone();
        let window = window.clone();
        area.add_events(gdk::EventMask::BUTTON_PRESS_MASK | gdk::EventMask::BUTTON_RELEASE_MASK);
        area.connect_button_press_event(move |_, ev| {
            if ev.button() == 1 {
                let (wx, wy) = window.position();
                state.borrow_mut().drag = Some((ev.root().0, ev.root().1, wx, wy));
            }
            glib::Propagation::Proceed
        });
    }
    {
        let state = state.clone();
        let window = window.clone();
        area.add_events(gdk::EventMask::POINTER_MOTION_MASK);
        area.connect_motion_notify_event(move |_, ev| {
            if let Some((rx, ry, wx, wy)) = state.borrow().drag {
                let dx = ev.root().0 - rx;
                let dy = ev.root().1 - ry;
                window.move_(wx + dx as i32, wy + dy as i32);
            }
            glib::Propagation::Proceed
        });
    }
    {
        let state = state.clone();
        area.connect_button_release_event(move |_, _| {
            state.borrow_mut().drag = None;
            glib::Propagation::Proceed
        });
    }

    // 托盘（可选）
    #[cfg(feature = "tray")]
    let visible = Arc::new(AtomicBool::new(true));
    #[cfg(feature = "tray")]
    {
        let tray = crate::tray::FloatTray {
            visible: visible.clone(),
            url: "http://127.0.0.1:8768/".to_string(),
            label: "lilyco 悬浮层".to_string(),
        };
        ksni::TrayService::new(tray).spawn();

        let visible = visible.clone();
        let win = window.clone();
        glib::timeout_add_local(Duration::from_millis(250), move || {
            let want = visible.load(Ordering::SeqCst);
            if win.is_visible() != want {
                win.set_visible(want);
            }
            glib::ControlFlow::Continue
        });
    }

    window.show_all();
    gtk::main();
    Ok(())
}
