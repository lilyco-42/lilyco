//! `resize` —— 缩放，0 表示按另一维等比。

use image::imageops::FilterType;
use lilyco::prelude::*;
use std::path::PathBuf;
use std::time::Instant;

use crate::common::{default_output, load_rgba, save_png};

// ── 3. resize ──────────────────────────────────────────────

/// 尺寸缩放；宽或高为 0 表示按另一维等比缩放
#[derive(App)]
#[app(
    run = "run_resize",
    about = "Resize an image to a target width/height (0 keeps the aspect ratio from the other dimension) and save a PNG (filter: Lanczos3)."
)]
pub(crate) struct Resize {
    /// 输入图像文件
    #[arg(about = "Input image file", must_exist = true)]
    pub(crate) image: PathBuf,

    /// 目标宽度（0 = 按高度等比）
    #[arg(default = 0, range = 0..=16384)]
    pub(crate) width: u32,

    /// 目标高度（0 = 按宽度等比）
    #[arg(default = 0, range = 0..=16384)]
    pub(crate) height: u32,

    /// 输出路径（缺省：输入同目录 {stem}-resize.png）
    #[arg(about = "Output PNG path")]
    pub(crate) out: Option<String>,
}

/// 业务逻辑：双零拒绝 → 等比换算 → Lanczos3 缩放 → 保存
pub(crate) fn run_resize(app: &Resize, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("resize {}", app.image.display())),
    });

    if app.width == 0 && app.height == 0 {
        return Err(AppError::InvalidArg(
            "at least one of width or height must be > 0".into(),
        ));
    }
    let img = load_rgba(&app.image)?;
    let (w, h) = img.dimensions();
    let (nw, nh) = match (app.width, app.height) {
        (0, nh) => {
            let nw = ((w as f64 * nh as f64 / h as f64).round() as u32).max(1);
            (nw, nh)
        }
        (nw, 0) => {
            let nh = ((h as f64 * nw as f64 / w as f64).round() as u32).max(1);
            (nw, nh)
        }
        (nw, nh) => (nw, nh),
    };

    let resized = image::imageops::resize(&img, nw, nh, FilterType::Lanczos3);
    let out = app
        .out
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output(&app.image, "-resize.png"));
    save_png(&resized, &out)?;

    let result = serde_json::json!({
        "width": nw,
        "height": nh,
        "out": out.display().to_string(),
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}
