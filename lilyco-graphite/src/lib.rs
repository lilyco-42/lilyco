// graphite 的消息类型拉入 wgpu/naga 深泛型 trait 链，会撑爆默认 128 的
// trait 求解器（与 graphite 上游 lib.rs 的同款注释同源），这里照搬其解法
#![recursion_limit = "512"]

//! # Lilyco Graphite — AI 画画最小闭环（P0 spike）
//!
//! headless 驱动 [Graphite](https://github.com/GraphiteEditor/Graphite) 编辑器内核：
//! 不开 GUI、不碰 GPU，直接把消息派发进内核的消息总线（`Dispatcher`），
//! 通过收集内核发往前端的 `FrontendMessage` 来观测结果。
//!
//! 设计要点：
//! - **操作面 = 消息总线**：Graphite 约 405 个消息变体就是它的全部操作面
//!   （见 `docs/GRAPHITE_OPERATIONS.md` 清点）。本 crate 先接通 4 个，
//!   打通 "新建文档 → 画矩形 → 改填充 → 序列化保存" 的最小闭环
//! - **官方先例**：驱动模式复刻 `graphite-editor` 的 `test_utils.rs`
//!   （测试如何无 GUI 模拟鼠标拖拽画形状），但不依赖其 `#[cfg(test)]` 门控
//! - **天生受门控**：`GraphiteSetFill` 声明 `safety = "t1"`，注册进
//!   [`lilyco_core::registry::Registry`] 即被安全门包住——自动化面默认拒绝，
//!   交互面（`Interactive` 策略）放行
//! - **进程级单宿主**：Graphite 的 `ENVIRONMENT` 是 `OnceLock`（每进程只能建一个
//!   Editor），因此所有工具共享 [`shared_host`] 返回的唯一内核实例
//!
//! ```ignore
//! let mut host = GraphiteHost::new();
//! host.new_document("demo");
//! host.set_primary_color(parse_hex_color("#E14D2A")?);
//! host.draw_rectangle(0., 0., 200., 100.);
//! host.set_fill(0.5);
//! let (name, bytes) = host.save_content()?; // .graphite 序列化字节
//! ```

pub mod app;
pub mod export;
pub mod host;

pub use export::{gpu_available, render_png, render_svg};
pub use host::{parse_hex_color, shared_host, GraphiteHost};

pub use app::{
    GraphiteAddRect, GraphiteDocNew, GraphiteExportPng, GraphiteExportSvg, GraphiteSave,
    GraphiteSetFill,
};
