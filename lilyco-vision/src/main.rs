//! lvision — 图像视觉工具包，作为 lilyco App 提供给 AI（DeepSeek Harness 的视觉工具）。
//!
//! 重写自 DSH Vision Toolkit 的本地操作，同一份定义天然四端 + AI 可调：
//! ```bash
//! lvision --mcp                                  # MCP stdio 服务器（Agent 直接调用 8 个视觉工具）
//! lvision --list                                 # 输出注册表 JSON（schema 清单）
//! ```
//!
//! 工具清单（含区域/输出约定）：
//! - `image-info`：读取格式 / 尺寸 / 文件字节数
//! - `crop`：像素框裁剪（超出边界自动收紧）+ 可选放大，输出 PNG
//! - `resize`：缩放（0 表示保持比例），输出 PNG
//! - `dominant-colors`：主色分析（缩图采样 + 贪心聚类合并）
//! - `pixel-diff`：原图 vs 重建图的网格级差异热力排名
//! - `extract-foreground`：背景透明化（边界连通域洪水填充，确定性近似）
//! - `trace`：位图矢量化（vtracer，与上游 DSH 工具同一引擎），输出 SVG
//! - `html-screenshot`：无头浏览器截图（30s 超时 kill）
//!
//! 输出文件默认写到输入同目录（如 `photo-crop.png`），可用 `--out` 覆盖。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use image::imageops::FilterType;
use image::{GenericImageView, ImageBuffer, Rgba, RgbaImage};
use lilyco::prelude::*;

// ── 公共辅助 ───────────────────────────────────────────────

/// color 模式判定“背景”的颜色容差（通道差上限，确定性近似）
const BG_TOLERANCE: u8 = 48;
/// dark 模式的亮度阈值（低于该值视为背景）
const DARK_LUMINANCE_THRESHOLD: u8 = 60;

/// 输出文件默认路径：输入同目录 + stem + 后缀
fn default_output(input: &Path, suffix: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".into());
    match input.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(format!("{stem}{suffix}")),
        _ => PathBuf::from(format!("{stem}{suffix}")),
    }
}

/// 解析 "X1,Y1,X2,Y2" 像素框
fn parse_box(s: &str) -> Result<[u32; 4], AppError> {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err(AppError::InvalidArg(
            "region must be a pixel box \"X1,Y1,X2,Y2\"".into(),
        ));
    }
    let mut out = [0u32; 4];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p.parse().map_err(|_| {
            AppError::InvalidArg(format!("region component {i} is not an integer: {p}"))
        })?;
    }
    Ok(out)
}

/// 把像素框收敛到 [0, w-1]×[0, h-1] 内并保证 x1<=x2 / y1<=y2（闭合框）
fn clamp_box(b: [u32; 4], w: u32, h: u32) -> (u32, u32, u32, u32) {
    if w == 0 || h == 0 {
        return (0, 0, 0, 0);
    }
    let mut x1 = b[0].min(w - 1);
    let mut y1 = b[1].min(h - 1);
    let mut x2 = b[2].min(w - 1);
    let mut y2 = b[3].min(h - 1);
    if x2 < x1 {
        std::mem::swap(&mut x1, &mut x2);
    }
    if y2 < y1 {
        std::mem::swap(&mut y1, &mut y2);
    }
    (x1, y1, x2, y2)
}

/// 解码为 RGBA（统一后续处理的数据形态）
fn load_rgba(p: &Path) -> Result<RgbaImage, AppError> {
    let d =
        image::open(p).map_err(|e| AppError::Runtime(format!("decode {}: {e}", p.display())))?;
    Ok(d.to_rgba8())
}

/// 保存 PNG（格式由文件扩展名推导）
fn save_png(img: &RgbaImage, p: &Path) -> Result<(), AppError> {
    img.save(p)
        .map_err(|e| AppError::Runtime(format!("write {}: {e}", p.display())))
}

// ── 1. image-info ──────────────────────────────────────────

/// 读取图片元信息（格式 / 宽高 / 字节数）
#[derive(App)]
#[app(
    run = "run_image_info",
    about = "Inspect an image file and return its format, width, height and size in bytes as JSON."
)]
struct ImageInfo {
    /// 输入图像文件
    #[arg(about = "Input image file (png/jpeg/webp/gif)", must_exist = true)]
    image: PathBuf,
}

/// 业务逻辑：解码 → 上报格式/尺寸/字节数
fn run_image_info(app: &ImageInfo, ctx: &Context) -> Result<serde_json::Value, AppError> {
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
// ── 2. crop ────────────────────────────────────────────────

/// 按像素框裁剪，超出图像边界自动收紧；scale>1 时用 Lanczos3 放大
#[derive(App)]
#[app(
    run = "run_crop",
    about = "Crop an image to a pixel box X1,Y1,X2,Y2 (clamped to the image bounds), optionally upscale, and save a PNG."
)]
struct Crop {
    /// 输入图像文件
    #[arg(about = "Input image file", must_exist = true)]
    image: PathBuf,

    /// 裁剪像素框
    #[arg(about = "Pixel box X1,Y1,X2,Y2")]
    region: String,

    /// 放大倍率（1 不缩放）
    #[arg(default = 1, range = 1..=8)]
    scale: u8,

    /// 输出路径（默认：输入同目录 {stem}-crop.png）
    #[arg(about = "Output PNG path")]
    out: Option<String>,
}

/// 业务逻辑：解析区域 → 收敛到边界 → 裁剪 → （放大）→ 保存 PNG
fn run_crop(app: &Crop, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("crop {}", app.image.display())),
    });

    let img = load_rgba(&app.image)?;
    let (w, h) = img.dimensions();
    let (x1, y1, x2, y2) = clamp_box(parse_box(&app.region)?, w, h);
    let scale = if app.scale == 0 { 1 } else { app.scale };

    let cw = x2 - x1 + 1;
    let ch = y2 - y1 + 1;
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

    let out = app
        .out
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output(&app.image, "-crop.png"));
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
// ── 3. resize ──────────────────────────────────────────────

/// 尺寸缩放；宽或高为 0 表示按另一维等比缩放
#[derive(App)]
#[app(
    run = "run_resize",
    about = "Resize an image to a target width/height (0 keeps the aspect ratio from the other dimension) and save a PNG (filter: Lanczos3)."
)]
struct Resize {
    /// 输入图像文件
    #[arg(about = "Input image file", must_exist = true)]
    image: PathBuf,

    /// 目标宽度（0 = 按高度等比）
    #[arg(default = 0, range = 0..=16384)]
    width: u32,

    /// 目标高度（0 = 按宽度等比）
    #[arg(default = 0, range = 0..=16384)]
    height: u32,

    /// 输出路径（缺省：输入同目录 {stem}-resize.png）
    #[arg(about = "Output PNG path")]
    out: Option<String>,
}

/// 业务逻辑：双零拒绝 → 等比换算 → Lanczos3 缩放 → 保存
fn run_resize(app: &Resize, ctx: &Context) -> Result<serde_json::Value, AppError> {
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
// ── 4. dominant-colors ─────────────────────────────────────

/// 单色聚类状态（运行和，合并时求平均色）
#[derive(Clone, Copy)]
struct Cluster {
    r: u32,
    g: u32,
    b: u32,
    n: usize,
}

impl Cluster {
    fn avg(self) -> (u8, u8, u8) {
        (
            (self.r / self.n.max(1) as u32) as u8,
            (self.g / self.n.max(1) as u32) as u8,
            (self.b / self.n.max(1) as u32) as u8,
        )
    }
}

/// 主色分析：缩到 ≤64px → 贪心聚类（通道距离 ≤ tolerance 合并）→ 按像素数排序
#[derive(App)]
#[app(
    run = "run_dominant_colors",
    about = "Find dominant colors: downscale to <=64px, greedy-cluster sampled pixels by channel distance `tolerance`, return ranked { hex, count, percent }."
)]
struct DominantColors {
    /// 输入图像文件
    #[arg(about = "Input image file", must_exist = true)]
    image: PathBuf,

    /// 返回的颜色数
    #[arg(default = 5, range = 1..=32)]
    top: u8,

    /// 聚类容差（0..=255，通道最大差）
    #[arg(default = 16, range = 0..=255)]
    tolerance: u8,

    /// 可选分析区域 "X1,Y1,X2,Y2"
    #[arg(about = "Optional pixel box X1,Y1,X2,Y2 to analyze")]
    region: Option<String>,
}

/// 业务逻辑：可选区域裁剪 → 缩略图 → 采样 → 贪心聚类 → 排序
fn run_dominant_colors(app: &DominantColors, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("dominant-colors {}", app.image.display())),
    });

    let top = if app.top == 0 { 5 } else { app.top };
    let tolerance = if app.tolerance == 0 {
        16
    } else {
        app.tolerance
    };

    let img = load_rgba(&app.image)?;
    let (w, h) = img.dimensions();
    let selection = match &app.region {
        Some(reg) => {
            let (x1, y1, x2, y2) = clamp_box(parse_box(reg)?, w, h);
            image::imageops::crop_imm(&img, x1, y1, x2 - x1 + 1, y2 - y1 + 1).to_image()
        }
        None => img,
    };
    let (sw, sh) = selection.dimensions();

    // 缩略图：最长边压到 64px
    let max_dim = sw.max(sh);
    let (tw, th) = if max_dim > 64 {
        let s = 64.0 / max_dim as f64;
        (
            (sw as f64 * s).round().max(1.0) as u32,
            (sh as f64 * s).round().max(1.0) as u32,
        )
    } else {
        (sw, sh)
    };
    let thumb = if tw != sw || th != sh {
        image::imageops::resize(&selection, tw, th, FilterType::Lanczos3)
    } else {
        selection
    };

    // 贪心聚类：与已存在（平均色）通道距离 ≤ tolerance 则并入
    let total = thumb.pixels().count().max(1);
    let mut clusters: Vec<Cluster> = Vec::new();
    for px in thumb.pixels() {
        let (r, g, b) = (px[0], px[1], px[2]);
        let mut merged = false;
        for c in clusters.iter_mut() {
            let (cr, cg, cb) = c.avg();
            let dist = cr.abs_diff(r).max(cg.abs_diff(g)).max(cb.abs_diff(b));
            if dist as u8 <= tolerance {
                c.r += r as u32;
                c.g += g as u32;
                c.b += b as u32;
                c.n += 1;
                merged = true;
                break;
            }
        }
        if !merged {
            clusters.push(Cluster {
                r: r as u32,
                g: g as u32,
                b: b as u32,
                n: 1,
            });
        }
    }

    clusters.sort_by(|a, b| b.n.cmp(&a.n));
    let colors: Vec<serde_json::Value> = clusters
        .iter()
        .take(top as usize)
        .map(|c| {
            let (cr, cg, cb) = c.avg();
            serde_json::json!({
                "hex": format!("#{cr:02x}{cg:02x}{cb:02x}"),
                "count": c.n,
                // 百分比保留两位小数
                "percent": (c.n as f64 / total as f64 * 100.0 * 100.0).round() / 100.0,
            })
        })
        .collect();

    let result = serde_json::json!({
        "top": colors.len(),
        "colors": colors,
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}
// ── 5. pixel-diff ──────────────────────────────────────────

/// 重建图网格化对比原图：逐格平均通道差，按得分排序，可选输出红色热力图
#[derive(App)]
#[app(
    run = "run_pixel_diff",
    about = "Diff an original image against a rebuilt one: resize the rebuilt to the original size, split both into a grid x grid, rank cells by mean absolute channel difference, and return { grid, mean_diff, worst: [{x1,y1,x2,y2,score}] } in original-image pixel coordinates; optionally write a red-intensity PNG heatmap."
)]
struct PixelDiff {
    /// 原图
    #[arg(about = "Original image file", must_exist = true)]
    original: PathBuf,

    /// 重建图
    #[arg(about = "Rebuilt/regenerated image file", must_exist = true)]
    rebuilt: PathBuf,

    /// 网格数（每边）
    #[arg(default = 6, range = 1..=32)]
    grid: u8,

    /// 返回的“最差格子”数量
    #[arg(default = 5, range = 1..=16)]
    top: u8,

    /// 可选热力图输出路径（默认：输入同目录 {stem}-heatmap.png）
    #[arg(about = "Optional red-intensity heatmap PNG path")]
    out_heatmap: Option<String>,
}

/// 业务逻辑：对齐尺寸 → 逐格平均差排名 → 可选热力图
fn run_pixel_diff(app: &PixelDiff, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!(
            "pixel-diff {} vs {}",
            app.original.display(),
            app.rebuilt.display()
        )),
    });

    let grid = if app.grid == 0 { 6 } else { app.grid };
    let top = if app.top == 0 { 5 } else { app.top };

    let a = load_rgba(&app.original)?;
    let b_raw = load_rgba(&app.rebuilt)?;
    let (w, h) = a.dimensions();
    // 重建图对齐到原图尺寸
    let b = image::imageops::resize(&b_raw, w, h, FilterType::Lanczos3);

    let cell_w = (w as usize + grid as usize - 1) / grid as usize;
    let cell_h = (h as usize + grid as usize - 1) / grid as usize;

    let mut cells: Vec<(u32, u32, u32, u32, f64)> = Vec::new();
    let mut total_sum = 0.0;
    let mut total_n = 0usize;
    for gy in 0..grid as usize {
        for gx in 0..grid as usize {
            let x1 = (gx * cell_w) as u32;
            let y1 = (gy * cell_h) as u32;
            if x1 >= w || y1 >= h {
                continue;
            }
            let x2 = ((x1 as usize + cell_w).min(w as usize)) as u32;
            let y2 = ((y1 as usize + cell_h).min(h as usize)) as u32;
            let mut sum = 0.0;
            let mut n = 0usize;
            for y in y1..y2 {
                for x in x1..x2 {
                    let pa = a.get_pixel(x, y);
                    let pb = b.get_pixel(x, y);
                    // 三通道差先提升到 u16，避免 debug 模式下 u8 相加溢出
                    sum += (pa[0].abs_diff(pb[0]) as u16
                        + pa[1].abs_diff(pb[1]) as u16
                        + pa[2].abs_diff(pb[2]) as u16) as f64
                        / 3.0;
                    n += 1;
                }
            }
            let score = sum / n.max(1) as f64;
            total_sum += score * n as f64;
            total_n += n;
            // 闭合像素框（原图坐标）
            cells.push((
                x1,
                y1,
                x2.saturating_sub(1).max(x1),
                y2.saturating_sub(1).max(y1),
                score,
            ));
        }
    }

    let mean_diff = if total_n > 0 {
        (total_sum / total_n as f64 * 100.0).round() / 100.0
    } else {
        0.0
    };

    cells.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap_or(std::cmp::Ordering::Equal));
    let worst: Vec<serde_json::Value> = cells
        .iter()
        .take(top as usize)
        .map(|(x1, y1, x2, y2, s)| {
            serde_json::json!({
                "x1": x1, "y1": y1, "x2": x2, "y2": y2,
                "score": (s * 100.0).round() / 100.0,
            })
        })
        .collect();

    if let Some(heat) = &app.out_heatmap {
        write_heatmap(&a, &cells, Path::new(heat))?;
    } else if cells.iter().any(|c| c.4 > 0.0) {
        let heat = default_output(&app.original, "-heatmap.png");
        write_heatmap(&a, &cells, &heat)?;
    }

    let result = serde_json::json!({
        "grid": grid,
        "mean_diff": mean_diff,
        "worst": worst,
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// 热力图：每格填充红色强度 = 该格平均差（原尺寸）
fn write_heatmap(
    a: &RgbaImage,
    cells: &[(u32, u32, u32, u32, f64)],
    out: &Path,
) -> Result<(), AppError> {
    let (w, h) = a.dimensions();
    let mut heat = ImageBuffer::from_pixel(w, h, Rgba([0u8, 0, 0, 255]));
    for (x1, y1, x2, y2, s) in cells {
        let v = (s.round() as u8).min(255);
        for y in *y1..=*y2 {
            for x in *x1..=*x2 {
                heat.put_pixel(x, y, Rgba([v, 0, 0, 255]));
            }
        }
    }
    save_png(&heat, out)
}
// ── 6. extract-foreground ──────────────────────────────────

/// 前景抠取：从所有边界像素做连通域洪水填充标记背景，背景透明化，输出 RGBA PNG。
///
/// 说明（确定性近似，比上游算法简单但可复现）：
/// - `color` 模式：与边界背景参考色通道距离 ≤ [`BG_TOLERANCE`] 且与边界连通的像素视为背景；
///   参考色默认取边界像素的平均色，`exclude_color` 提供时优先用其作参考色。
/// - `dark` 模式：亮度 < [`DARK_LUMINANCE_THRESHOLD`] 且与边界连通的像素视为背景。
#[derive(App)]
#[app(
    run = "run_extract_foreground",
    about = "Make the background transparent: flood-fill background from all border pixels (color mode: within tolerance of the border color; dark mode: luminance < 60) and save an RGBA PNG with the foreground kept."
)]
struct ExtractForeground {
    /// 输入图像文件
    #[arg(about = "Input image file", must_exist = true)]
    image: PathBuf,

    /// 背景判定模式
    #[arg(default = "color", about = "Background mode: \"color\" or \"dark\"")]
    mode: String,

    /// 背景参考色 "#RRGGBB"（color 模式，缺省用边界平均色）
    #[arg(about = "Explicit background color #RRGGBB (color mode)")]
    exclude_color: Option<String>,

    /// 输出路径（缺省：输入同目录 {stem}-fg.png）
    #[arg(about = "Output RGBA PNG path")]
    out: Option<String>,
}

/// 解析 "#RRGGBB" 颜色
fn parse_hex_color(s: &str) -> Result<(u8, u8, u8), AppError> {
    let s = s.trim();
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return Err(AppError::InvalidArg(format!(
            "exclude_color must be #RRGGBB, got: {s}"
        )));
    }
    match (
        u8::from_str_radix(&hex[0..2], 16),
        u8::from_str_radix(&hex[2..4], 16),
        u8::from_str_radix(&hex[4..6], 16),
    ) {
        (Ok(r), Ok(g), Ok(b)) => Ok((r, g, b)),
        _ => Err(AppError::InvalidArg(format!(
            "exclude_color must be #RRGGBB hex, got: {s}"
        ))),
    }
}

/// 边界连通域洪水填充：返回背景掩码（true = 背景）
fn flood_background(img: &RgbaImage, is_bg: impl Fn(u32, u32) -> bool) -> Vec<bool> {
    let (w, h) = img.dimensions();
    let mut bg = vec![false; (w as usize) * (h as usize)];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    let push = |x: u32, y: u32, bg: &mut Vec<bool>, stack: &mut Vec<(u32, u32)>| {
        if is_bg(x, y) && !bg[(y as usize) * (w as usize) + x as usize] {
            bg[(y as usize) * (w as usize) + x as usize] = true;
            stack.push((x, y));
        }
    };
    for x in 0..w {
        push(x, 0, &mut bg, &mut stack);
        if h > 1 {
            push(x, h - 1, &mut bg, &mut stack);
        }
    }
    for y in 1..h.saturating_sub(1) {
        push(0, y, &mut bg, &mut stack);
        if w > 1 {
            push(w - 1, y, &mut bg, &mut stack);
        }
    }
    while let Some((x, y)) = stack.pop() {
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if nx < w && ny < h && is_bg(nx, ny) {
                let i = (ny * w + nx) as usize;
                if !bg[i] {
                    bg[i] = true;
                    stack.push((nx, ny));
                }
            }
        }
    }
    bg
}
/// 业务逻辑：选模式 → 洪水填充 → 透明化背景 → 保存 RGBA PNG
fn run_extract_foreground(
    app: &ExtractForeground,
    ctx: &Context,
) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("extract-foreground {}", app.image.display())),
    });

    let img = load_rgba(&app.image)?;
    let (w, h) = img.dimensions();
    let mode = if app.mode.is_empty() {
        "color"
    } else {
        app.mode.as_str()
    };

    match mode {
        "color" | "dark" => {}
        other => {
            return Err(AppError::InvalidArg(format!(
                "mode must be \"color\" or \"dark\", got: {other}"
            )));
        }
    }

    // is_bg 判定：color 色阶 vs dark 亮度（闭包借用 img，避免 move 后无法复用）
    let ref_color: Option<(u8, u8, u8)> = if mode == "color" {
        Some(match &app.exclude_color {
            Some(s) => parse_hex_color(s)?,
            None => {
                // 边界平均色作为背景参考
                let (mut sr, mut sg, mut sb, mut n) = (0u64, 0u64, 0u64, 0u64);
                for (x, y, px) in img.enumerate_pixels() {
                    if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                        sr += px[0] as u64;
                        sg += px[1] as u64;
                        sb += px[2] as u64;
                        n += 1;
                    }
                }
                let n = n.max(1);
                ((sr / n) as u8, (sg / n) as u8, (sb / n) as u8)
            }
        })
    } else {
        None
    };
    let is_bg = |x: u32, y: u32| -> bool {
        let px = img.get_pixel(x, y);
        match ref_color {
            Some((wr, wg, wb)) => {
                px[0].abs_diff(wr) <= BG_TOLERANCE
                    && px[1].abs_diff(wg) <= BG_TOLERANCE
                    && px[2].abs_diff(wb) <= BG_TOLERANCE
            }
            None => {
                let lum = (px[0] as u32 * 299 + px[1] as u32 * 587 + px[2] as u32 * 114) / 1000;
                lum < DARK_LUMINANCE_THRESHOLD as u32
            }
        }
    };

    let bg = flood_background(&img, is_bg);

    let mut out_img = img.clone();
    let mut background_pixels = 0u64;
    for (x, y, px) in out_img.enumerate_pixels_mut() {
        if bg[(y * w + x) as usize] {
            px[3] = 0;
            background_pixels += 1;
        }
    }
    let kept_pixels = (w as u64 * h as u64).saturating_sub(background_pixels);

    let out = app
        .out
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output(&app.image, "-fg.png"));
    save_png(&out_img, &out)?;

    let result = serde_json::json!({
        "out": out.display().to_string(),
        "background_pixels": background_pixels,
        "kept_pixels": kept_pixels,
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

// ── 7. trace ───────────────────────────────────────────────

/// 位图矢量化（vtracer 引擎，与上游 DSH Vision Toolkit 的 Python 包装同一内核）。
///
/// `polygon` → FitMode：多段线 vs 贝塞尔样条；`color` → Clustering：彩色簇 vs 二值前景。
#[derive(App)]
#[app(
    run = "run_trace",
    about = "Vectorize a raster image to SVG with the vtracer engine: `polygon` selects polygon vs bezier paths, `color` toggles color vs binary tracing, `scale` pre-upscales the bitmap by an integer factor."
)]
struct Trace {
    /// 输入图像文件
    #[arg(about = "Input image file", must_exist = true)]
    image: PathBuf,

    /// 彩色追踪（false = 二值轮廓）
    #[arg(default = true, about = "Color trace (false = binary)")]
    color: bool,

    /// 多边形路径（false = 贝塞尔曲线）
    #[arg(default = false, about = "Polygon paths instead of bezier curves")]
    polygon: bool,

    /// 预处理放大倍率（小图放大后矢量质量更好）
    #[arg(default = 1, range = 1..=16)]
    scale: u8,

    /// 输出路径（缺省：输入同目录 {stem}.svg）
    #[arg(about = "Output SVG path")]
    out: Option<String>,
}

/// 业务逻辑：解码 →（放大）→ 装配 vtracer pipeline → SVG → 落盘
fn run_trace(app: &Trace, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("trace {}", app.image.display())),
    });

    let scale = if app.scale == 0 { 1 } else { app.scale };
    let img = load_rgba(&app.image)?;
    let (w, h) = img.dimensions();

    // 可选预放大：1..=16，Lanczos3
    let scaled = if scale > 1 {
        image::imageops::resize(
            &img,
            w * scale as u32,
            h * scale as u32,
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

    // 参数映射：polygon → FitMode；color → Clustering
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
        ..vtracer::Config::default()
    };
    let pipeline = config
        .build()
        .map_err(|e| AppError::Runtime(format!("vtracer config: {e}")))?;
    let svg = pipeline
        .to_svg(&vtracer_img)
        .map_err(|e| AppError::Runtime(format!("vtracer: {e}")))?;

    let out = app
        .out
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output(&app.image, ".svg"));
    std::fs::write(&out, &svg)
        .map_err(|e| AppError::Runtime(format!("write {}: {e}", out.display())))?;
    let byte_size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);

    let result = serde_json::json!({
        "out": out.display().to_string(),
        "byte_size": byte_size,
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}
// ── 8. html-screenshot ─────────────────────────────────────

/// 无头浏览器截图（无额外依赖；找不到浏览器时报 Runtime 错）
#[derive(App)]
#[app(
    run = "run_html_screenshot",
    about = "Screenshot an HTML file with a headless browser: CHROME_PATH, then platform browser installs (Chrome/Edge on Windows, chromium/google-chrome on Unix) are used; waits up to 30s and returns { out, browser, exit_code }."
)]
struct HtmlScreenshot {
    /// 本地 html/htm 文件
    #[arg(about = "HTML file (.html/.htm)", must_exist = true)]
    source: PathBuf,

    /// 视口宽度
    #[arg(default = 1280, range = 320..=8192)]
    width: u16,

    /// 视口高度
    #[arg(default = 800, range = 240..=8192)]
    height: u16,

    /// 输出路径（缺省：输入同目录 {stem}-screenshot.png）
    #[arg(about = "Output PNG path")]
    out: Option<String>,
}

/// Windows 常见浏览器安装路径
const WINDOWS_BROWSER_CANDIDATES: &[&str] = &[
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
];

/// Unix 常见浏览器可执行名（PATH 查找）
#[cfg(unix)]
const UNIX_BROWSER_NAMES: &[&str] = &["chromium", "google-chrome", "chromium-browser"];

/// PATHEXT 扩展名列表（Windows 解析 PATH 用；仅 Unix PATH 查找使用）
#[cfg(unix)]
fn path_exts() -> Vec<String> {
    std::env::var("PATHEXT")
        .map(|e| e.split(';').map(|s| s.trim().to_lowercase()).collect())
        .unwrap_or_else(|_| vec![".exe".into(), ".bat".into(), ".cmd".into()])
}

/// 按 PATH 找可执行文件（Unix 浏览器查找）
#[cfg(unix)]
fn find_on_path(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    let sep = if cfg!(windows) { ';' } else { ':' };
    for dir in path.split(sep).filter(|s| !s.is_empty()) {
        let base = Path::new(dir).join(name);
        if cfg!(windows) {
            for ext in path_exts() {
                let candidate = format!("{}{}", base.display(), ext);
                if Path::new(&candidate).is_file() {
                    return Some(candidate);
                }
            }
        } else if base.is_file() {
            return Some(base.display().to_string());
        }
    }
    None
}

/// 解析可用浏览器。
///
/// 优先级：`CHROME_PATH` 环境变量（显式设置但不可用时直接判无，不回落到系统安装，
/// 便于 CI/测试确定性）→ 平台候选路径 → Unix PATH 查找。
fn find_browser() -> Option<String> {
    if let Ok(p) = std::env::var("CHROME_PATH") {
        let p = p.trim().to_string();
        if !p.is_empty() {
            if Path::new(&p).is_file() {
                return Some(p);
            }
            return None;
        }
    }
    for c in WINDOWS_BROWSER_CANDIDATES {
        if Path::new(c).is_file() {
            return Some(c.to_string());
        }
    }
    #[cfg(unix)]
    for name in UNIX_BROWSER_NAMES {
        if let Some(p) = find_on_path(name) {
            return Some(p);
        }
    }
    None
}

/// 无浏览器时的错误消息（列出所有已检查项）
fn no_browser_error() -> AppError {
    let extra = if cfg!(unix) {
        "chromium / google-chrome / chromium-browser on PATH".to_string()
    } else {
        WINDOWS_BROWSER_CANDIDATES.to_vec().join(", ")
    };
    AppError::Runtime(format!(
        "no browser found for html screenshot: checked CHROME_PATH, then {extra}"
    ))
}
/// 业务逻辑：校验扩展名 → 找浏览器 → 无头截图（30s 超时 kill）→ 校验产物
fn run_html_screenshot(app: &HtmlScreenshot, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("html-screenshot {}", app.source.display())),
    });

    // 1. 扩展名校验
    let ext = app
        .source
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if ext != "html" && ext != "htm" {
        return Err(AppError::InvalidArg(format!(
            "source must be an .html/.htm file, got {}",
            app.source.display()
        )));
    }

    // 2. 浏览器解析
    let browser = find_browser().ok_or_else(no_browser_error)?;

    // 3. 组装 file:// URL（绝对路径 + 正斜杠）
    let abs = std::fs::canonicalize(&app.source)?;
    let file_url = format!("file:///{}", abs.display().to_string().replace('\\', "/"));

    let width = if app.width == 0 { 1280 } else { app.width };
    let height = if app.height == 0 { 800 } else { app.height };
    let out = app
        .out
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output(&app.source, "-screenshot.png"));

    let mut cmd = Command::new(&browser);
    cmd.arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--hide-scrollbars")
        .arg(format!("--screenshot={}", out.display()))
        .arg(format!("--window-size={width},{height}"))
        .arg(&file_url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Runtime(format!("spawn {browser}: {e}")))?;

    // 4. try_wait 轮询 + 30s 超时 kill（不引入 wait-timeout 依赖）
    let timeout = Duration::from_secs(30);
    let deadline = start + timeout;
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::Runtime(format!(
                        "browser timed out after {}s: {browser}",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(AppError::Runtime(format!("wait {browser}: {e}"))),
        }
    };

    // 5. 回收 stderr 尾部（子进程已退出，读不阻塞）
    let stderr_tail = child
        .stderr
        .take()
        .map(|mut s| {
            let mut buf = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut buf);
            let start_i = buf.len().saturating_sub(1000);
            buf[start_i..].to_string()
        })
        .unwrap_or_default();

    // 6. 校验产物非空
    if !out.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        let tail = if stderr_tail.trim().is_empty() {
            "(empty)"
        } else {
            stderr_tail.trim()
        };
        return Err(AppError::Runtime(format!(
            "browser exited {browser:?} with code {exit_code:?} and produced no non-empty screenshot at {}; stderr tail: {tail}",
            out.display()
        )));
    }

    let result = serde_json::json!({
        "out": out.display().to_string(),
        "browser": browser,
        "exit_code": exit_code,
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

// ── registry + main ────────────────────────────────────────

/// 注册全部 8 个视觉工具，供 MCP / --list 使用
fn build_registry() -> Registry {
    let mut r = Registry::new();
    for cmd in [
        RegisteredCommand::from_app::<ImageInfo>(),
        RegisteredCommand::from_app::<Crop>(),
        RegisteredCommand::from_app::<Resize>(),
        RegisteredCommand::from_app::<DominantColors>(),
        RegisteredCommand::from_app::<PixelDiff>(),
        RegisteredCommand::from_app::<ExtractForeground>(),
        RegisteredCommand::from_app::<Trace>(),
        RegisteredCommand::from_app::<HtmlScreenshot>(),
    ] {
        r.register(cmd).expect("register vision tool");
    }
    r
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--mcp") {
        lilyco::serve_mcp(build_registry());
        return;
    }
    if args.iter().any(|a| a == "--list") {
        println!("{}", build_registry().to_json());
        return;
    }
    eprintln!("usage: lvision --mcp | --list");
    std::process::exit(2);
}
// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn ctx() -> Context {
        let (tx, _) = mpsc::channel();
        Context::new_test(tx)
    }

    /// 64×64 白底红方块测试图（白色包围红色）
    fn red_on_white(w: u32, h: u32) -> RgbaImage {
        ImageBuffer::from_fn(w, h, |x, y| {
            if x >= w / 4 && x < 3 * w / 4 && y >= h / 4 && y < 3 * h / 4 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        })
    }

    fn save_png_in(img: &RgbaImage, dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        save_png(img, &p).expect("save png");
        p
    }

    #[test]
    fn image_info_reports_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let src = save_png_in(&red_on_white(64, 64), dir.path(), "in.png");

        let app = ImageInfo { image: src };
        let r = run_image_info(&app, &ctx()).unwrap();
        assert_eq!(r["format"], "png");
        assert_eq!(r["width"], 64);
        assert_eq!(r["height"], 64);
        assert!(r["file_size_bytes"].as_u64().unwrap() > 0);
        assert_eq!(r["ok"], true);
    }

    #[test]
    fn crop_clamps_and_saves_png() {
        let dir = tempfile::tempdir().unwrap();
        let src = save_png_in(&red_on_white(64, 64), dir.path(), "in.png");
        let out = dir.path().join("crop.png").display().to_string();

        // 区域部分出界：50,50 → 右边/下边超出 64 边界，应收敛到 63
        let app = Crop {
            image: src,
            region: "50,50,120,90".into(),
            scale: 1,
            out: Some(out.clone()),
        };
        let r = run_crop(&app, &ctx()).unwrap();
        assert_eq!(r["width"], 14);
        assert_eq!(r["height"], 14);
        assert_eq!(r["out"], out);
        assert!(Path::new(&out).is_file(), "crop 产物应存在");
    }

    #[test]
    fn resize_preserves_aspect_ratio() {
        let dir = tempfile::tempdir().unwrap();
        // 40×20 → 宽 20 → 高 10
        let img = ImageBuffer::from_fn(40, 20, |_, _| Rgba([128, 128, 128, 255]));
        let src = save_png_in(&img, dir.path(), "in.png");
        let out = dir.path().join("resize.png").display().to_string();

        let app = Resize {
            image: src,
            width: 20,
            height: 0,
            out: Some(out.clone()),
        };
        let r = run_resize(&app, &ctx()).unwrap();
        assert_eq!(r["width"], 20);
        assert_eq!(r["height"], 10);
        assert!(Path::new(&out).is_file());
    }

    #[test]
    fn resize_rejects_zero_both() {
        let dir = tempfile::tempdir().unwrap();
        let src = save_png_in(&red_on_white(16, 16), dir.path(), "in.png");
        let app = Resize {
            image: src,
            width: 0,
            height: 0,
            out: None,
        };
        let err = run_resize(&app, &ctx()).unwrap_err();
        assert!(err.to_string().contains("width"), "got: {err}");
    }

    #[test]
    fn dominant_colors_finds_red_on_white() {
        let dir = tempfile::tempdir().unwrap();
        let src = save_png_in(&red_on_white(64, 64), dir.path(), "in.png");
        let app = DominantColors {
            image: src,
            top: 8,
            tolerance: 2,
            region: None,
        };
        let r = run_dominant_colors(&app, &ctx()).unwrap();
        let colors = r["colors"].as_array().unwrap();
        assert!(!colors.is_empty());
        let hexes: Vec<String> = colors
            .iter()
            .filter_map(|c| c["hex"].as_str().map(String::from))
            .collect();
        assert!(
            hexes.contains(&"#ff0000".to_string()),
            "纯红应在主色里，got: {hexes:?}"
        );
        assert_eq!(r["top"], colors.len());
    }
    #[test]
    fn pixel_diff_ranks_changed_region() {
        let dir = tempfile::tempdir().unwrap();
        // 原图：整片灰；重建图：左上角 16×16 变黑
        let a = ImageBuffer::from_fn(64, 64, |_, _| Rgba([200, 200, 200, 255]));
        let b = ImageBuffer::from_fn(64, 64, |x, y| {
            if x < 16 && y < 16 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([200, 200, 200, 255])
            }
        });
        let orig = save_png_in(&a, dir.path(), "orig.png");
        let rebuilt = save_png_in(&b, dir.path(), "rebuilt.png");

        let app = PixelDiff {
            original: orig,
            rebuilt,
            grid: 4,
            top: 3,
            out_heatmap: Some(dir.path().join("heat.png").display().to_string()),
        };
        let r = run_pixel_diff(&app, &ctx()).unwrap();
        assert_eq!(r["grid"], 4);
        let worst = r["worst"].as_array().unwrap();
        assert!(!worst.is_empty());
        // 左上格应排第一（x1=0,y1=0,x2<=15）
        assert_eq!(worst[0]["x1"], 0);
        assert_eq!(worst[0]["y1"], 0);
        assert!(worst[0]["score"].as_f64().unwrap() > 0.0);
        // 其余格子得分应小于第一名
        let top = worst[0]["score"].as_f64().unwrap();
        assert!(worst.iter().all(|c| c["score"].as_f64().unwrap() <= top));
        assert!(r["mean_diff"].as_f64().unwrap() > 0.0);
        assert!(
            Path::new(&app.out_heatmap.clone().unwrap()).is_file(),
            "热力图产物应存在"
        );
    }

    #[test]
    fn extract_foreground_keeps_center_red_on_white() {
        let dir = tempfile::tempdir().unwrap();
        let src = save_png_in(&red_on_white(64, 64), dir.path(), "in.png");
        let out = save_png_in(
            &ImageBuffer::from_fn(2, 2, |_, _| Rgba([0, 0, 0, 255])),
            dir.path(),
            "placeholder.png",
        );

        let app = ExtractForeground {
            image: src,
            mode: "color".into(),
            exclude_color: None,
            out: Some(out.display().to_string()),
        };
        let r = run_extract_foreground(&app, &ctx()).unwrap();
        assert!(r["kept_pixels"].as_u64().unwrap() > 0);
        assert!(r["background_pixels"].as_u64().unwrap() > 0);

        let result = load_rgba(Path::new(r["out"].as_str().unwrap())).unwrap();
        // 角落（背景）透明，中心（红方块）不透明
        assert_eq!(result.get_pixel(0, 0)[3], 0);
        assert_eq!(result.get_pixel(32, 32)[3], 255);
    }

    #[test]
    fn trace_produces_svg() {
        let dir = tempfile::tempdir().unwrap();
        let src = save_png_in(&red_on_white(48, 48), dir.path(), "in.png");
        let out = dir.path().join("out.svg");

        let app = Trace {
            image: src,
            color: true,
            polygon: false,
            scale: 1,
            out: Some(out.display().to_string()),
        };
        let r = run_trace(&app, &ctx()).unwrap();
        assert!(r["byte_size"].as_u64().unwrap() > 0);
        let svg = std::fs::read_to_string(&out).expect("read svg");
        assert!(
            svg.trim_start().starts_with("<svg") || svg.contains("<path"),
            "SVG 应含 <svg> 或 <path>，got: {}",
            &svg[..svg.len().min(120)]
        );
    }

    #[test]
    fn extract_foreground_rejects_bad_mode() {
        let dir = tempfile::tempdir().unwrap();
        let src = save_png_in(&red_on_white(16, 16), dir.path(), "in.png");
        let app = ExtractForeground {
            image: src,
            mode: "neon".into(),
            exclude_color: None,
            out: None,
        };
        let err = run_extract_foreground(&app, &ctx()).unwrap_err();
        assert!(err.to_string().contains("color"), "got: {err}");
    }

    #[test]
    fn html_screenshot_errors_without_browser() {
        let dir = tempfile::tempdir().unwrap();
        let html_path = dir.path().join("page.html");
        std::fs::write(&html_path, "<h1>hi</h1>").unwrap();
        // CHROME_PATH 指向不存在的浏览器 → 必须报错（不需要真实浏览器）
        let fake = dir.path().join("no-browser.exe");
        std::env::set_var("CHROME_PATH", fake.display().to_string());

        let app = HtmlScreenshot {
            source: html_path,
            width: 1280,
            height: 800,
            out: None,
        };
        let err = run_html_screenshot(&app, &ctx()).unwrap_err();
        std::env::remove_var("CHROME_PATH");
        assert!(
            err.to_string().contains("browser"),
            "应报浏览器缺失，got: {err}"
        );
    }

    #[test]
    fn registry_has_eight_tools() {
        assert_eq!(build_registry().visible().count(), 8);
    }
}
