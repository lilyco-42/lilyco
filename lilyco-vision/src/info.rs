//! `image-info` —— 格式 / 宽高 / 字节数。

use image::GenericImageView;
use lilyco::prelude::*;
use std::path::PathBuf;
use std::time::Instant;

// ── 1. image-info ──────────────────────────────────────────

/// 读取图片元信息（格式 / 宽高 / 字节数）
#[derive(App)]
#[app(
    run = "run_image_info",
    about = "Inspect an image file and return its format, width, height and size in bytes as JSON."
)]
pub(crate) struct ImageInfo {
    /// 输入图像文件
    #[arg(about = "Input image file (png/jpeg/webp/gif)", must_exist = true)]
    pub(crate) image: PathBuf,
}

/// 业务逻辑：解码 → 上报格式/尺寸/字节数
pub(crate) fn run_image_info(
    app: &ImageInfo,
    ctx: &Context,
) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("image-info {}", app.image.display())),
    });

    let d = image::open(&app.image)
        .map_err(|e| AppError::Runtime(format!("decode {}: {e}", app.image.display())))?;
    let (width, height) = d.dimensions();
    // 0.25 的 DynamicImage 没有 format()：用文件头魔数猜格式
    let mut file = std::fs::File::open(&app.image)?;
    let mut magic = [0u8; 16];
    let n = std::io::Read::read(&mut file, &mut magic)?;
    let format = image::guess_format(&magic[..n])
        .map_err(|_| {
            AppError::Runtime(format!(
                "unrecognized image format: {}",
                app.image.display()
            ))
        })
        .map(|f| format!("{:?}", f).to_ascii_lowercase())?;
    let file_size_bytes = std::fs::metadata(&app.image).map(|m| m.len()).unwrap_or(0);

    let result = serde_json::json!({
        "format": format,
        "width": width,
        "height": height,
        "file_size_bytes": file_size_bytes,
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}
