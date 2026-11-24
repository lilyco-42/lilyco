//! 系统托盘（KDE StatusNotifierItem）：关闭悬浮层后仍可从这里恢复/退出。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ksni::menu::StandardItem;

pub struct FloatTray {
    pub visible: Arc<AtomicBool>,
    /// 点击“打开控制台”时打开的地址
    pub url: String,
    pub label: String,
}

impl ksni::Tray for FloatTray {
    fn id(&self) -> String {
        "lilyco-float".into()
    }

    fn title(&self) -> String {
        self.label.clone()
    }

    fn icon_name(&self) -> String {
        "applications-multimedia".into()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let next = !self.visible.load(Ordering::SeqCst);
        self.visible.store(next, Ordering::SeqCst);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            StandardItem {
                label: "显示 / 隐藏悬浮层".into(),
                activate: Box::new(|this: &mut Self| {
                    let next = !this.visible.load(Ordering::SeqCst);
                    this.visible.store(next, Ordering::SeqCst);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "打开控制台".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = webbrowser::open(&this.url);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "退出悬浮层".into(),
                activate: Box::new(|_: &mut Self| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
