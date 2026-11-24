//! # lilyco-float
//!
//! 桌面悬浮弹幕 / 歌词层插件：透明、无边框、置顶、可拖动，带系统托盘。
//!
//! 形态对标 Tauri plugin：**一个 crate + 一个前端片段 + 一个示例**，任何 lilyco 应用都能复用。
//!
//! - Rust 侧：`FloatSpec`（外观）+ `Feed`（数据源）+ `run()`（拉起 GTK 主循环与托盘）
//! - 数据源内置两种：`HttpJsonFeed`（轮询任意 JSON 接口）与 `ChannelFeed`（应用自己推送）
//! - 前端片段见 `web/FloatStage.md`（React，透明浮层 + 横向滚动）
//!
//! ```no_run
//! # #[cfg(feature = "gtk")]
//! # fn demo() -> anyhow::Result<()> {
//! use lilyco_float::{FloatSpec, HttpJsonFeed};
//! let feed = HttpJsonFeed::new("http://127.0.0.1:8768/api/danmu/status", "recent", "ts", "text");
//! lilyco_float::run(FloatSpec::default(), feed)?;
//! # Ok(())
//! # }
//! ```

pub mod feed;
pub mod spec;

#[cfg(feature = "gtk")]
pub mod gtk_win;
#[cfg(feature = "tray")]
pub mod tray;

#[cfg(feature = "http")]
pub use feed::HttpJsonFeed;
pub use feed::{ChannelFeed, Feed, FloatItem};
pub use spec::FloatSpec;

#[cfg(feature = "gtk")]
pub use gtk_win::run;
