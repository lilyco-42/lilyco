//! Lilyco Ultra UI — NSH 原生渲染后端。
//!
//! 同一份 `UiSpec`（lilyco-ultra-ui::spec）既能生成 React HTML 页面，
//! 也能直接渲染成 NSH 原生 GTK 视图：无 WebView、无 QuickJS、无 JS 运行时。
//! 组件树只构建一次，状态变化只重跑读了该信号的绑定（细粒度更新）。

pub mod render;

use std::sync::OnceLock;

use lilyco_ultra_ui::spec::UiSpec;
use nsh_base::theme;
use nsh_core::prelude::*;

static SPEC: OnceLock<UiSpec> = OnceLock::new();

/// 在窗口启动前装入要渲染的规格。返回 Err 表示重复装入。
pub fn set_spec(spec: UiSpec) -> Result<(), UiSpec> {
    SPEC.set(spec)
}

fn spec() -> &'static UiSpec {
    SPEC.get()
        .expect("set_spec() must run before the native window starts")
}

#[native_module("lilyco/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// 根组件。NSH 的组件函数只执行一次，之后由响应式绑定做局部更新。
#[component]
pub fn App() -> impl IntoElement {
    let values: Signal<std::collections::BTreeMap<String, String>> = signal(Default::default());
    let notice: Signal<Option<String>> = signal(None);
    let state = render::UiState { values, notice };

    let s = spec();
    let children = render::render_elements(&s.window.elements, &state);

    let col = view()
        .flex_col()
        .gap_2()
        .child(
            text(s.window.title.clone())
                .text_size(px(24.0))
                .font_weight(FontWeight::BOLD),
        )
        .children(children)
        .child(
            text(move || format!("状态: {}", render::fmt_map(&values.get()))).text_size(px(12.0)),
        )
        .child(text(move || notice.get().unwrap_or_default()).text_size(px(13.0)));

    view()
        .flex_1()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .p_4()
        .child(scroll().scrollbar(false).flex_1().child(col))
}
