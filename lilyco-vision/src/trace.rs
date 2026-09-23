//! `trace` —— 位图矢量化（vtracer 引擎），输出 SVG。

use image::imageops::FilterType;
use lilyco::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::common::{default_output, load_rgba};

// ── 7. trace ───────────────────────────────────────────────

/// 位图矢量化（vtracer 引擎，与上游 DSH Vision Toolkit 的 Python 包装同一内核）。
///
/// `polygon` → FitMode：多段线 vs 贝塞尔样条；`color` → Clustering：彩色簇 vs 二值前景。
#[derive(App)]
#[app(
    run = "run_trace",
    about = "Vectorize a raster image to SVG with the vtracer engine: `polygon` selects polygon vs bezier paths, `color` toggles color vs binary tracing, `scale` pre-upscales the bitmap by an integer factor."
)]
pub(crate) struct Trace {
    /// 输入图像文件
    #[arg(about = "Input image file", must_exist = true)]
    pub(crate) image: PathBuf,

    /// 彩色追踪（false = 二值轮廓）
    #[arg(default = true, about = "Color trace (false = binary)")]
    pub(crate) color: bool,

    /// 多边形路径（false = 贝塞尔曲线）
    #[arg(default = false, about = "Polygon paths instead of bezier curves")]
    pub(crate) polygon: bool,

    /// 预处理放大倍率（0 = 自动：短边 <256px 时放大到 256，上限 16x）
    #[arg(default = 0, range = 0..=16)]
    pub(crate) scale: u8,

    /// 输出路径（缺省：输入同目录 {stem}.svg）
    #[arg(about = "Output SVG path")]
    pub(crate) out: Option<String>,
}

/// 剥掉 vtracer 输出的首个白色整底 path（自闭合 "<path ... />"）。
/// 对齐上游 strip_background：白色整底不进 SVG，省 token。
pub(crate) fn strip_background(svg: &str) -> String {
    if let Some(start) = svg.find("<path") {
        if let Some(rel) = svg[start..].find("/>") {
            let end = start + rel + 2;
            let first = &svg[start..end];
            if first.contains("fill=\"#FFFFFF\"") {
                let mut out = String::with_capacity(svg.len());
                out.push_str(&svg[..start]);
                out.push_str(&svg[end..]);
                return out;
            }
        }
    }
    svg.to_string()
}

/// 把每个小数截断到 2 位小数（"1.23456" → "1.23"）。
/// 无需 regex：'.' 后保留恰好 2 位数字并跳过该数字串的其余部分。
pub(crate) fn truncate_decimals(svg: &str) -> String {
    let chars: Vec<char> = svg.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(svg.len());
    let mut i = 0usize;
    while i < n {
        out.push(chars[i]);
        if chars[i] == '.' {
            let mut kept = 0usize;
            i += 1;
            while i < n && kept < 2 && chars[i].is_ascii_digit() {
                out.push(chars[i]);
                kept += 1;
                i += 1;
            }
            while i < n && chars[i].is_ascii_digit() {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    out
}

/// 按字节长度写入 SVG（UTF-8），返回磁盘字节数（对齐上游 write_svg 字节契约）
pub(crate) fn write_svg(p: &Path, svg: &str) -> Result<usize, AppError> {
    let payload = svg.as_bytes();
    std::fs::write(p, payload)
        .map_err(|e| AppError::Runtime(format!("write {}: {e}", p.display())))?;
    Ok(payload.len())
}

/// 业务逻辑：解码 →（自动/显式放大）→ vtracer pipeline → 后处理 → 落盘
pub(crate) fn run_trace(app: &Trace, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("trace {}", app.image.display())),
    });

    let img = load_rgba(&app.image)?;
    let (w, h) = img.dimensions();

    // 放大倍率：0 = 自动；短边 <256px 时 factor = ceil(256/短边)，夹紧 1..=16
    let factor = if app.scale == 0 {
        let shortest = w.min(h);
        if shortest >= 256 {
            1u8
        } else {
            ((256 + shortest - 1) / shortest).clamp(1, 16) as u8
        }
    } else {
        app.scale
    };
    let scaled = if factor > 1 {
        image::imageops::resize(
            &img,
            w * factor as u32,
            h * factor as u32,
            FilterType::Lanczos3,
        )
    } else {
        img
    };
    let (sw, sh) = scaled.dimensions();
    let vtracer_img = vtracer::ColorImage {
        pixels: scaled.into_raw(),
        width: sw as usize,
        height: sh as usize,
    };

    // 参数映射：polygon → FitMode；color → Clustering；speckle=8 对齐上游
    let config = vtracer::Config {
        clustering: if app.color {
            vtracer::Clustering::ColorCluster
        } else {
            vtracer::Clustering::Binary
        },
        mode: if app.polygon {
            vtracer::FitMode::Polygon
        } else {
            vtracer::FitMode::Spline
        },
        filter_speckle: 8,
        ..vtracer::Config::default()
    };
    let pipeline = config
        .build()
        .map_err(|e| AppError::Runtime(format!("vtracer config: {e}")))?;
    let raw_svg = pipeline
        .to_svg(&vtracer_img)
        .map_err(|e| AppError::Runtime(format!("vtracer: {e}")))?;

    // 后处理：先剥掉开头的白色整底 path，再截断所有小数到 2 位
    let svg = truncate_decimals(&strip_background(&raw_svg));

    let paths = svg.matches("<path").count();
    if paths == 0 {
        return Err(AppError::Runtime(
            "trace produced 0 paths — small/empty image may need a larger --scale (default auto upscales to min 256px)"
                .into(),
        ));
    }

    let out = app
        .out
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output(&app.image, ".svg"));
    let byte_size = write_svg(&out, &svg)?;

    let result = serde_json::json!({
        "out": out.display().to_string(),
        "byte_size": byte_size,
        "paths": paths,
        "traced_at": factor,
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}
