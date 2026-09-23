//! `crop` —— 像素框裁剪（越界自动收紧）+ 可选 Lanczos3 放大。

use image::imageops::FilterType;
use lilyco::prelude::*;
use std::path::PathBuf;
use std::time::Instant;

use crate::common::{default_output, load_rgba, parse_region, save_png};

// ── 2. crop ────────────────────────────────────────────────

/// 按像素框裁剪，超出图像边界自动收紧；scale>1 时用 Lanczos3 放大
#[derive(App)]
#[app(
    run = "run_crop",
    about = "Crop an image to a pixel box X1,Y1,X2,Y2 (clamped to the image bounds), optionally upscale, and save a PNG."
)]
pub(crate) struct Crop {
    /// 输入图像文件
    #[arg(about = "Input image file", must_exist = true)]
    pub(crate) image: PathBuf,

    /// 裁剪像素框
    #[arg(about = "Pixel box X1,Y1,X2,Y2")]
    pub(crate) region: String,

    /// 放大倍率（1 不缩放）
    #[arg(default = 1, range = 1..=8)]
    pub(crate) scale: u8,

    /// 输出路径（默认：输入同目录 {stem}-crop.png）
    #[arg(about = "Output PNG path")]
    pub(crate) out: Option<String>,
}

/// 业务逻辑：解析区域 → 收敛到边界 → 裁剪 → （放大）→ 保存 PNG
pub(crate) fn run_crop(app: &Crop, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("crop {}", app.image.display())),
    });

    let img = load_rgba(&app.image)?;
    let (w, h) = img.dimensions();
    let b = parse_region(&app.region, w, h)?;
    let (x1, y1, x2, y2) = (b[0], b[1], b[2], b[3]);
    if app.scale == 0 {
        return Err(AppError::InvalidArg("--scale must be >= 1".into()));
    }
    let scale = app.scale;

    let cw = x2 - x1;
    let ch = y2 - y1;
    let cropped = image::imageops::crop_imm(&img, x1, y1, cw, ch).to_image();
    let final_img = if scale > 1 {
        image::imageops::resize(
            &cropped,
            cw * scale as u32,
            ch * scale as u32,
            FilterType::Lanczos3,
        )
    } else {
        cropped
    };

    // 默认命名对齐上游：scale==1 → {stem}.crop.png；scale>1 → {stem}.crop@{scale}x.png
    let suffix = if scale > 1 {
        format!(".crop@{scale}x.png")
    } else {
        ".crop.png".into()
    };
    let out = app
        .out
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output(&app.image, &suffix));
    save_png(&final_img, &out)?;
    let (ow, oh) = final_img.dimensions();

    let result = serde_json::json!({
        "width": ow,
        "height": oh,
        "out": out.display().to_string(),
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}
