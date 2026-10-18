//! Design system: one palette, one scale, three themes, one trace model.
//!
//! Front-end agnostic on purpose — GPUI reads [`Color`] directly, a webview
//! reads [`Theme::emit_css`], the CLI reads [`Color::to_hex`].

pub mod color;
pub mod theme;
pub mod tokens;
pub mod trace;

pub use color::Color;
pub use theme::{Palette, Theme};
pub use trace::{diff_stat, image_label, RowBody, Status, TraceRow};
