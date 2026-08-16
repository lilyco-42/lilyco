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

/// 解析 "X1,Y1,X2,Y2" 像素框（共享：crop / dominant-colors / extract-fg）。
///
/// 对齐上游语义：
/// - 每分量按 i64 解析（允许负数），分量数必须为 4、必须可转整数；
/// - 各分量收敛到 [0, w]/[0, h]（左闭右开，x2==w 表示整行）并归一化颠倒角点；
/// - 空盒子（x1>=x2 或 y1>=y2）直接报错。
fn parse_region(s: &str, w: u32, h: u32) -> Result<[u32; 4], AppError> {
    let malformed = || AppError::InvalidArg("region must be a pixel box \"X1,Y1,X2,Y2\"".into());
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err(malformed());
    }
    let mut c = [0i64; 4];
    for (i, p) in parts.iter().enumerate() {
        c[i] = p.parse::<i64>().map_err(|_| malformed())?;
    }
    let clamp = |v: i64, hi: u32| v.clamp(0, hi as i64) as u32;
    let x1 = clamp(c[0], w);
    let x2 = clamp(c[2], w);
    let y1 = clamp(c[1], h);
    let y2 = clamp(c[3], h);
    let (x1, x2) = (x1.min(x2), x1.max(x2));
    let (y1, y2) = (y1.min(y2), y1.max(y2));
    if x1 >= x2 || y1 >= y2 {
        return Err(AppError::InvalidArg("empty region".into()));
    }
    Ok([x1, y1, x2, y2])
}

/// 白底合成：RGBA 按 alpha 叠加到白色（对齐上游：透明像素当空白而不是黑色）
fn flatten_on_white(img: &RgbaImage) -> RgbaImage {
    ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        let a = p[3] as u32;
        let mix = |c: u8| -> u8 { ((c as u32 * a + 255 * (255 - a)) / 255) as u8 };
        Rgba([mix(p[0]), mix(p[1]), mix(p[2]), 255])
    })
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

    /// 聚类容差（0..=255，通道最大差，缺省 8 对齐上游 merge_tol）
    #[arg(default = 8, range = 0..=255)]
    tolerance: u8,

    /// 可选分析区域 "X1,Y1,X2,Y2"
    #[arg(about = "Optional pixel box X1,Y1,X2,Y2 to analyze")]
    region: Option<String>,

    /// pick 模式：逗号分隔的候选色（如 "#F9FAFA,#F5F5F5,#EDEDED"）
    #[arg(about = "Pick mode: comma-separated candidate palette #RRGGBB (e.g. #F9FAFA,#F5F5F5)")]
    candidates: Option<String>,
}

/// 业务逻辑：可选区域裁剪 → 缩略图 → 采样 → 贪心聚类 → 排序
fn run_dominant_colors(app: &DominantColors, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("dominant-colors {}", app.image.display())),
    });

    let top = if app.top == 0 { 5 } else { app.top };
    let tolerance = if app.tolerance == 0 { 8 } else { app.tolerance };

    let img = load_rgba(&app.image)?;
    let (w, h) = img.dimensions();
    let selection = match &app.region {
        Some(reg) => {
            // parse_region：负值夹紧、空盒报错、左闭右开
            let b = parse_region(reg, w, h)?;
            image::imageops::crop_imm(&img, b[0], b[1], b[2] - b[0], b[3] - b[1]).to_image()
        }
        None => img,
    };
    let (sw, sh) = selection.dimensions();

    // pick 模式（对齐上游候选色挑选）：精确命中 > 容差内支持数 > 无命中报最近色
    if let Some(cands) = &app.candidates {
        let parsed: Vec<(String, (u8, u8, u8))> = cands
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| {
                let text = s.trim().to_string();
                let rgb = parse_hex_color(&text)?;
                Ok((text, rgb))
            })
            .collect::<Result<_, AppError>>()?;
        if parsed.is_empty() {
            return Err(AppError::InvalidArg(
                "--candidates needs at least one #RRGGBB".into(),
            ));
        }
        let result = pick_colors(&selection, &parsed, tolerance);
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }

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

/// pick 模式：候选色打分（对齐上游 dominant_colors.pick）
///
/// - 精确命中（像素与候选色完全相等）→ 该候选为 winner，share 100；
/// - 否则取容差内硬支持像素数最多的候选为 winner；
/// - 全无命中 → winner 为 null（hard=0），另报平均距离最近的 closest。
fn pick_colors(
    selection: &RgbaImage,
    candidates: &[(String, (u8, u8, u8))],
    tolerance: u8,
) -> serde_json::Value {
    let (w, h) = selection.dimensions();
    let total = (w as usize * h as usize).max(1);
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut stats: Vec<(usize, f64, bool)> = Vec::new(); // (hard, mean_dist, exact)
    for (text, (cr, cg, cb)) in candidates {
        let mut hard = 0usize;
        let mut dist_sum = 0.0f64;
        let mut exact = false;
        for p in selection.pixels() {
            let d = (p[0].abs_diff(*cr) + p[1].abs_diff(*cg) + p[2].abs_diff(*cb)) as f64 / 3.0;
            dist_sum += d;
            if p[0] == *cr && p[1] == *cg && p[2] == *cb {
                exact = true;
                hard += 1;
            } else if d <= tolerance as f64 {
                hard += 1;
            }
        }
        stats.push((hard, dist_sum / total as f64, exact));
        rows.push(serde_json::json!({
            "text": text,
            "share": (hard as f64 / total as f64 * 100.0 * 100.0).round() / 100.0,
            "hard": hard,
        }));
    }

    // winner：精确命中优先，其次硬支持最多（>0）
    let winner_idx = stats.iter().position(|s| s.2).or_else(|| {
        let mx = stats.iter().map(|s| s.0).max().unwrap_or(0);
        if mx > 0 {
            stats.iter().position(|s| s.0 == mx)
        } else {
            None
        }
    });
    let winner = winner_idx.map(|i| rows[i].clone());

    // closest：平均距离最小的候选
    let closest_idx = stats
        .iter()
        .enumerate()
        .min_by(|a, b| {
            a.1 .1
                .partial_cmp(&b.1 .1)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i);
    let closest = closest_idx.map(|i| {
        serde_json::json!({
            "text": candidates[i].0,
            "distance": (stats[i].1 * 100.0).round() / 100.0,
        })
    });

    serde_json::json!({
        "candidates": rows,
        "winner": winner,
        "closest": closest,
        "ok": true,
    })
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

    let a = flatten_on_white(&load_rgba(&app.original)?);
    let b_raw = flatten_on_white(&load_rgba(&app.rebuilt)?);
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
            // 归一化得分：平均通道差 / 255 → 0..1（对齐上游 100% 语义）
            let score = sum / n.max(1) as f64 / 255.0;
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
    // 总体差异百分比（对齐上游 "overall difference: X.XX%"）
    let overall_diff_pct = if total_n > 0 {
        ((total_sum / total_n as f64) * 100.0 * 100.0).round() / 100.0
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
        "overall_diff_pct": overall_diff_pct,
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
        let v = ((s * 255.0).round() as u8).min(255);
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
    about = "Make the background transparent: flood-fill background from the borders (color mode: within tolerance of the border color; dark mode: luminance < 60), prune detached noise, and save RGBA PNG(s) as {stem}.clean.png — multiple input images are supported."
)]
struct ExtractForeground {
    /// 输入图像文件（可多个，各输出 {stem}.clean.png）
    #[arg(about = "Input image files (one or more, each written to {stem}.clean.png)")]
    images: Vec<String>,

    /// 背景判定模式
    #[arg(default = "color", about = "Background mode: \"color\" or \"dark\"")]
    mode: String,

    /// 手动搜索区域 "X1,Y1,X2,Y2"（缺省：整图边界洪水填充）
    #[arg(about = "Manual search region X1,Y1,X2,Y2 (default: whole image border-flood)")]
    region: Option<String>,

    /// 排除色 "#RRGGBB"（color 模式：距离 ≤ exclude_tol 视为背景）
    #[arg(about = "Explicit background color #RRGGBB (color mode)")]
    exclude_color: Option<String>,

    /// 排除色容差（0..=255，通道最大差，缺省 35 对齐上游）
    #[arg(default = 35, range = 0..=255)]
    exclude_tol: u8,

    /// 输出路径（缺省：输入同目录 {stem}.clean.png；多图时忽略）
    #[arg(about = "Output RGBA PNG path (single-image mode only)")]
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
/// 业务逻辑：对每个输入：选模式 → 洪水填充 → 透明化 → 剔除离群小分量 → 保存 {stem}.clean.png
///
/// 确定性说明（与上游的差异）：上游 "auto" 模式还会用居中圆盘分析裁掉圆底/环形分量、
/// 只保留图标字形；这里用边界洪水填充保留整个连接前景，仅剔除与主分量不相交的
/// 离群小分量（噪声）。测试断言的是本实现契约，而非上游字形契约。
fn run_extract_foreground(
    app: &ExtractForeground,
    ctx: &Context,
) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: Some(app.images.len() as u64),
        message: Some(format!("extract-foreground {} image(s)", app.images.len())),
    });

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

    let exclude_tol = if app.exclude_tol == 0 {
        35
    } else {
        app.exclude_tol
    };
    let mut outputs = Vec::new();

    for image in &app.images {
        let src = PathBuf::from(image);
        let img = load_rgba(&src)?;
        let (w, h) = img.dimensions();

        // 参考色 + 容差：显式 exclude_color 用 exclude_tol；缺省用边界平均色 + BG_TOLERANCE
        let (ref_color, tol): (Option<(u8, u8, u8)>, u8) = if mode == "color" {
            match &app.exclude_color {
                Some(s) => (Some(parse_hex_color(s)?), exclude_tol),
                None => {
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
                    (
                        Some(((sr / n) as u8, (sg / n) as u8, (sb / n) as u8)),
                        BG_TOLERANCE,
                    )
                }
            }
        } else {
            (None, 0)
        };
        let is_bg = |x: u32, y: u32| -> bool {
            let px = img.get_pixel(x, y);
            match ref_color {
                Some((wr, wg, wb)) => {
                    px[0].abs_diff(wr) <= tol
                        && px[1].abs_diff(wg) <= tol
                        && px[2].abs_diff(wb) <= tol
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
        // 去噪：剔除不与主连通分量相交的离群小分量（噪声），保留内部细节
        prune_detached_foreground(&mut out_img, w, h);
        let kept_pixels = (w as u64 * h as u64).saturating_sub(background_pixels);

        let out = if app.out.is_some() && app.images.len() == 1 {
            PathBuf::from(app.out.as_ref().unwrap())
        } else {
            // 默认命名对齐上游：输入同目录 {stem}.clean.png
            default_output(&src, ".clean.png")
        };
        save_png(&out_img, &out)?;

        outputs.push(serde_json::json!({
            "out": out.display().to_string(),
            "background_pixels": background_pixels,
            "kept_pixels": kept_pixels,
        }));
    }

    let result = serde_json::json!({
        "outputs": outputs,
        "count": outputs.len(),
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// 8 邻域连通分量（按大小降序）
fn connected_components(w: u32, h: u32, mask: &[bool]) -> Vec<Vec<usize>> {
    let mut seen = vec![false; mask.len()];
    let mut comps: Vec<Vec<usize>> = Vec::new();
    for i in 0..mask.len() {
        if !mask[i] || seen[i] {
            continue;
        }
        let mut stack = vec![i];
        seen[i] = true;
        let mut comp = Vec::new();
        while let Some(idx) = stack.pop() {
            comp.push(idx);
            let x = (idx as u32) % w;
            let y = (idx as u32) / w;
            for dx in 0u32..3 {
                for dy in 0u32..3 {
                    if dx == 1 && dy == 1 {
                        continue;
                    }
                    let nx = x as i64 + dx as i64 - 1;
                    let ny = y as i64 + dy as i64 - 1;
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let ni = (ny as u32 * w + nx as u32) as usize;
                    if mask[ni] && !seen[ni] {
                        seen[ni] = true;
                        stack.push(ni);
                    }
                }
            }
        }
        comps.push(comp);
    }
    comps.sort_by_key(|c| std::cmp::Reverse(c.len()));
    comps
}

/// 连通分量包围盒：(x0, y0, x1, y1)
fn comp_bbox(comp: &[usize], w: u32) -> (u32, u32, u32, u32) {
    let mut x0 = u32::MAX;
    let mut y0 = u32::MAX;
    let mut x1 = 0u32;
    let mut y1 = 0u32;
    for &idx in comp {
        let x = (idx as u32) % w;
        let y = (idx as u32) / w;
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    (x0, y0, x1, y1)
}

/// 剔除离群前景噪声：alpha>0 做连通分量，剔除
/// size < max(主分量 2%, 8) 且 bbox 不与主分量 bbox 相交的分量。
fn prune_detached_foreground(img: &mut RgbaImage, w: u32, h: u32) {
    let mask: Vec<bool> = img.enumerate_pixels().map(|(_, _, p)| p[3] > 0).collect();
    let comps = connected_components(w, h, &mask);
    let Some(main) = comps.first() else { return };
    let threshold = ((main.len() as f64 * 0.02).ceil() as usize).max(8);
    let (mix, miy, maxx, maxy) = comp_bbox(main, w);
    for (i, comp) in comps.iter().enumerate() {
        if i == 0 {
            continue;
        }
        let (bx0, by0, bx1, by1) = comp_bbox(comp, w);
        let overlaps = bx0 <= maxx && bx1 >= mix && by0 <= maxy && by1 >= miy;
        if comp.len() < threshold && !overlaps {
            for &idx in comp {
                let x = (idx as u32) % w;
                let y = (idx as u32) / w;
                img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
    }
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

    /// 预处理放大倍率（0 = 自动：短边 <256px 时放大到 256，上限 16x）
    #[arg(default = 0, range = 0..=16)]
    scale: u8,

    /// 输出路径（缺省：输入同目录 {stem}.svg）
    #[arg(about = "Output SVG path")]
    out: Option<String>,
}

/// 剥掉 vtracer 输出的首个白色整底 path（自闭合 "<path ... />"）。
/// 对齐上游 strip_background：白色整底不进 SVG，省 token。
fn strip_background(svg: &str) -> String {
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
fn truncate_decimals(svg: &str) -> String {
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
fn write_svg(p: &Path, svg: &str) -> Result<usize, AppError> {
    let payload = svg.as_bytes();
    std::fs::write(p, payload)
        .map_err(|e| AppError::Runtime(format!("write {}: {e}", p.display())))?;
    Ok(payload.len())
}

/// 业务逻辑：解码 →（自动/显式放大）→ vtracer pipeline → 后处理 → 落盘
fn run_trace(app: &Trace, ctx: &Context) -> Result<serde_json::Value, AppError> {
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

    /// 输出路径（缺省：输入同目录 {stem}.png，对齐上游 default_output）
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
        .unwrap_or_else(|| default_output(&app.source, ".png"));

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
            candidates: None,
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
            images: vec![src.display().to_string()],
            mode: "color".into(),
            region: None,
            exclude_color: None,
            exclude_tol: 35,
            out: Some(out.display().to_string()),
        };
        let r = run_extract_foreground(&app, &ctx()).unwrap();
        let first = &r["outputs"][0];
        assert!(first["kept_pixels"].as_u64().unwrap() > 0);
        assert!(first["background_pixels"].as_u64().unwrap() > 0);

        let result = load_rgba(Path::new(first["out"].as_str().unwrap())).unwrap();
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
            images: vec![src.display().to_string()],
            mode: "neon".into(),
            region: None,
            exclude_color: None,
            exclude_tol: 35,
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

    // ── 对齐上游 test_crop.py ─────────────────────────────

    #[test]
    fn crop_parse_rejects_malformed_regions() {
        for bad in ["1,2,3", "1,2,3,x", "1,2"] {
            assert!(
                parse_region(bad, 100, 100).is_err(),
                "region {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn crop_clamps_negative_and_reversed() {
        // 负坐标夹到 0，越界夹到图像边缘（上游 clamp_box 契约）
        assert_eq!(
            parse_region("-20,5,300,95", 200, 100).unwrap(),
            [0, 5, 200, 95]
        );
        // 颠倒角点归一化
        assert_eq!(
            parse_region("30,40,10,20", 100, 100).unwrap(),
            [10, 20, 30, 40]
        );
    }

    #[test]
    fn crop_empty_region_is_error() {
        // 全出界 → 收敛为空盒 → 报错（上游 "empty" 契约）
        let err = parse_region("500,500,600,600", 200, 100).unwrap_err();
        assert!(err.to_string().contains("empty"), "got: {err}");
    }

    #[test]
    fn crop_scale_naming_and_content() {
        let dir = tempfile::tempdir().unwrap();
        let src = save_png_in(&red_on_white(200, 100), dir.path(), "shot.png");

        // 默认命名：scale>1 → {stem}.crop@{scale}x.png（输入同目录）
        let app = Crop {
            image: src.clone(),
            region: "50,30,150,70".into(),
            scale: 4,
            out: None,
        };
        let r = run_crop(&app, &ctx()).unwrap();
        let out = PathBuf::from(r["out"].as_str().unwrap());
        assert!(out.is_file(), "crop@4x 产物应存在: {out:?}");
        assert!(
            out.to_string_lossy().ends_with("shot.crop@4x.png"),
            "命名应对齐 {{stem}}.crop@{{scale}}x.png: {out:?}"
        );
        let crop = load_rgba(&out).unwrap();
        assert_eq!(crop.dimensions(), (400, 160));
        assert_eq!(
            crop.get_pixel(20, 20),
            &Rgba([255, 0, 0, 255]),
            "放大后红块仍在"
        );

        // 显式 -o 优先
        let custom = dir.path().join("scaled.png").display().to_string();
        let app2 = Crop {
            image: src.clone(),
            region: "50,30,150,70".into(),
            scale: 2,
            out: Some(custom.clone()),
        };
        let r2 = run_crop(&app2, &ctx()).unwrap();
        assert_eq!(r2["out"], custom);
        assert_eq!(
            load_rgba(Path::new(&custom)).unwrap().dimensions(),
            (200, 80)
        );

        // scale=0 拒绝
        let app3 = Crop {
            image: src,
            region: "50,30,150,70".into(),
            scale: 0,
            out: None,
        };
        assert!(run_crop(&app3, &ctx()).is_err());
    }

    #[test]
    fn crop_missing_file_is_error() {
        let app = Crop {
            image: PathBuf::from("C:/definitely/not/here.png"),
            region: "0,0,10,10".into(),
            scale: 1,
            out: None,
        };
        assert!(run_crop(&app, &ctx()).is_err());
    }

    // ── 对齐上游 test_dominant_colors.py ───────────────────

    #[test]
    fn dominant_colors_merges_near_duplicates() {
        // #F5F5F5 与 #F3F3F3 差 2，默认容差 8 应合并为单簇
        let dir = tempfile::tempdir().unwrap();
        let img = ImageBuffer::from_fn(200, 100, |x, _y| {
            if x < 100 {
                Rgba([245, 245, 245, 255])
            } else {
                Rgba([243, 243, 243, 255])
            }
        });
        let src = save_png_in(&img, dir.path(), "near.png");
        let app = DominantColors {
            image: src,
            top: 3,
            tolerance: 0, // 0 → 默认 8
            region: None,
            candidates: None,
        };
        let r = run_dominant_colors(&app, &ctx()).unwrap();
        let colors = r["colors"].as_array().unwrap();
        assert!(
            colors[0]["percent"].as_f64().unwrap() > 90.0,
            "近色应合并成主簇: {colors:?}"
        );
    }

    #[test]
    fn dominant_colors_pick_exact_candidate_wins() {
        let dir = tempfile::tempdir().unwrap();
        let img = ImageBuffer::from_fn(60, 30, |_, _| Rgba([245, 245, 245, 255]));
        let src = save_png_in(&img, dir.path(), "gray.png");
        let app = DominantColors {
            image: src,
            top: 5,
            tolerance: 16,
            region: None,
            candidates: Some("#F9FAFA,#F5F5F5,#F3F3F3,#EDEDED".into()),
        };
        let r = run_dominant_colors(&app, &ctx()).unwrap();
        assert_eq!(r["winner"]["text"], "#F5F5F5");
        assert_eq!(r["winner"]["share"], 100.0);
    }

    #[test]
    fn dominant_colors_pick_no_match_reports_closest() {
        let dir = tempfile::tempdir().unwrap();
        let img = ImageBuffer::from_fn(40, 40, |_, _| Rgba([0, 0, 255, 255])); // blue
        let src = save_png_in(&img, dir.path(), "blue.png");
        let app = DominantColors {
            image: src,
            top: 5,
            tolerance: 16,
            region: None,
            candidates: Some("#F9FAFA,#F5F5F5".into()),
        };
        let r = run_dominant_colors(&app, &ctx()).unwrap();
        assert!(r["winner"].is_null(), "容差内无命中应无赢家: {r}");
        assert_eq!(r["closest"]["text"], "#F5F5F5", "蓝更接近 #F5F5F5: {r}");
    }

    #[test]
    fn dominant_colors_region_clamp_and_empty_reject() {
        let dir = tempfile::tempdir().unwrap();
        let src = save_png_in(&red_on_white(100, 80), dir.path(), "in.png");
        // 负坐标夹紧 → 正常执行
        let ok = DominantColors {
            image: src.clone(),
            top: 3,
            tolerance: 8,
            region: Some("-10,-10,120,90".into()),
            candidates: None,
        };
        assert!(run_dominant_colors(&ok, &ctx()).is_ok());
        // 空盒 → 报错
        let empty = DominantColors {
            image: src,
            top: 3,
            tolerance: 8,
            region: Some("50,50,50,60".into()),
            candidates: None,
        };
        let err = run_dominant_colors(&empty, &ctx()).unwrap_err();
        assert!(err.to_string().contains("empty"), "got: {err}");
    }

    // ── 对齐上游 test_pixel_diff.py ────────────────────────

    #[test]
    fn pixel_diff_composites_transparent_on_white() {
        let dir = tempfile::tempdir().unwrap();
        let white = ImageBuffer::from_fn(240, 120, |_, _| Rgba([255, 255, 255, 255]));
        let transparent = ImageBuffer::from_fn(240, 120, |_, _| Rgba([0, 0, 0, 0]));
        let orig = save_png_in(&white, dir.path(), "a.png");
        let rebuilt = save_png_in(&transparent, dir.path(), "t.png");
        let app = PixelDiff {
            original: orig,
            rebuilt,
            grid: 4,
            top: 1,
            out_heatmap: None,
        };
        let r = run_pixel_diff(&app, &ctx()).unwrap();
        assert_eq!(r["overall_diff_pct"], 0.0, "透明重建应合成白底而非黑底");
    }

    #[test]
    fn pixel_diff_ranks_corrupted_cell() {
        let dir = tempfile::tempdir().unwrap();
        let base = ImageBuffer::from_fn(240, 120, |_, _| Rgba([255, 255, 255, 255]));
        let broken = ImageBuffer::from_fn(240, 120, |x, y| {
            if x >= 180 && y < 60 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        let orig = save_png_in(&base, dir.path(), "a.png");
        let rebuilt = save_png_in(&broken, dir.path(), "b.png");
        let app = PixelDiff {
            original: orig,
            rebuilt,
            grid: 4,
            top: 1,
            out_heatmap: None,
        };
        let r = run_pixel_diff(&app, &ctx()).unwrap();
        let worst = r["worst"][0].clone();
        assert_eq!(worst["x1"], 180, "最差格应指向被破坏的格子: {worst}");
        let score = worst["score"].as_f64().unwrap();
        assert!((score - 1.0).abs() < 0.01, "全黑对全白应 ≈100%: {score}");
        assert!(r["overall_diff_pct"].as_f64().unwrap() > 0.0);
    }

    // ── 对齐上游 test_extract_fg.py（本实现契约）────────────

    /// 徽章图：浅蓝圆盘 + 白环 + 深蓝字形 + 右下角深灰噪声（上游 _make_badge）
    fn badge_image(w: u32, h: u32) -> RgbaImage {
        let cx = w / 2;
        let cy = h / 2;
        ImageBuffer::from_fn(w, h, |x, y| {
            // 右下角深灰噪声（连接背景，应被洪水填充 + 去噪剔除）
            if x >= w - 30 && x < w - 15 && y >= h - 20 && y < h - 8 {
                return Rgba([85, 85, 85, 255]);
            }
            let dx = (x as i64 - cx as i64) as f64;
            let dy = (y as i64 - cy as i64) as f64;
            let d = (dx * dx + dy * dy).sqrt();
            if d <= 20.0 {
                Rgba([47, 95, 191, 255]) // glyph 深蓝
            } else if d <= 60.0 {
                Rgba([180, 211, 240, 255]) // disc 浅蓝
            } else if d <= 64.0 {
                Rgba([255, 255, 255, 255]) // ring 白环
            } else {
                Rgba([234, 241, 249, 255]) // background
            }
        })
    }

    #[test]
    fn extract_fg_multi_image_and_noise() {
        let dir = tempfile::tempdir().unwrap();
        let a = save_png_in(&badge_image(200, 200), dir.path(), "a.png");
        let b = save_png_in(&badge_image(200, 200), dir.path(), "b.png");
        let app = ExtractForeground {
            images: vec![a.display().to_string(), b.display().to_string()],
            mode: "color".into(),
            region: None,
            exclude_color: None,
            exclude_tol: 35,
            out: None,
        };
        let r = run_extract_foreground(&app, &ctx()).unwrap();
        assert_eq!(r["count"], 2);
        let outs = r["outputs"].as_array().unwrap();
        for (i, name) in ["a.clean.png", "b.clean.png"].iter().enumerate() {
            let out = outs[i]["out"].as_str().unwrap();
            assert!(out.ends_with(name), "命名应对齐 {{stem}}.clean.png: {out}");
            let img = load_rgba(Path::new(out)).unwrap();
            // 前景数量足够
            let fg: Vec<&Rgba<u8>> = img.pixels().filter(|p| p[3] > 128).collect();
            assert!(fg.len() > 200, "前景太少: {}", fg.len());
            // 深蓝字形保留
            let deep_blue = fg
                .iter()
                .filter(|p| {
                    p[2] > p[0] + 40 && (p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])) > 60
                })
                .count();
            assert!(deep_blue > 200, "深蓝字形应保留: {deep_blue}");
            // 灰噪声被剔除
            let noise = fg
                .iter()
                .filter(|p| p[0].abs_diff(85) < 25 && p[1].abs_diff(85) < 25)
                .count();
            assert_eq!(noise, 0, "灰噪声不应泄漏进前景");
        }
    }

    #[test]
    fn extract_fg_manual_region_exclude_tol() {
        let dir = tempfile::tempdir().unwrap();
        let src = save_png_in(&badge_image(200, 200), dir.path(), "icon.png");
        let app = ExtractForeground {
            images: vec![src.display().to_string()],
            mode: "color".into(),
            region: Some("40,40,160,160".into()),
            exclude_color: Some("#EAF1F9".into()), // 背景色，容差 35
            exclude_tol: 35,
            out: None,
        };
        let r = run_extract_foreground(&app, &ctx()).unwrap();
        let first = &r["outputs"][0];
        assert!(first["kept_pixels"].as_u64().unwrap() > 0);
        assert!(first["background_pixels"].as_u64().unwrap() > 0);
        let img = load_rgba(Path::new(first["out"].as_str().unwrap())).unwrap();
        let deep_blue = img
            .pixels()
            .filter(|p| {
                p[3] > 128
                    && p[2] > p[0] + 40
                    && (p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])) > 60
            })
            .count();
        assert!(deep_blue > 200, "手动区域应保留深蓝字形: {deep_blue}");
    }

    // ── 对齐上游 test_trace.py ─────────────────────────────

    #[test]
    fn trace_post_processors() {
        let svg = "<svg><path d=\"M0,0 L9,0 Z\" fill=\"#FFFFFF\" transform=\"x\"/><path d=\"M1.23456,7.891011 L2,3\" fill=\"#000000\"/></svg>";
        let stripped = strip_background(svg);
        assert!(
            !stripped.contains("fill=\"#FFFFFF\""),
            "白色整底 path 应被剥掉"
        );
        assert!(stripped.contains("fill=\"#000000\""));

        let kept = strip_background("<svg><path d=\"M0,0\" fill=\"#000000\"/></svg>");
        assert!(kept.contains("fill=\"#000000\""), "非白首 path 应保留");

        let truncated = truncate_decimals(&stripped);
        assert!(!truncated.contains("1.23456") && truncated.contains("1.23"));
        assert!(!truncated.contains("7.891011") && truncated.contains("7.89"));
    }

    #[test]
    fn trace_auto_upscales_small_icon() {
        let dir = tempfile::tempdir().unwrap();
        // 24×24 白底 + 2×2 黑块（默认自动放大到短边 256）
        let img = ImageBuffer::from_fn(24, 24, |x, y| {
            if (10..12).contains(&x) && (10..12).contains(&y) {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        let src = save_png_in(&img, dir.path(), "icon.png");
        let out = dir.path().join("icon.svg");

        let app = Trace {
            image: src.clone(),
            color: false,
            polygon: true,
            scale: 0, // 自动
            out: Some(out.display().to_string()),
        };
        let r = run_trace(&app, &ctx()).unwrap();
        assert!(
            r["traced_at"].as_u64().unwrap() > 1,
            "小图标应自动放大: {}",
            r["traced_at"]
        );
        let svg = std::fs::read_to_string(&out).unwrap();
        assert!(svg.contains("<path"), "自动放大后小图标应可矢量化");

        // 显式 scale=1：2×2 黑块 < speckle 8 → 0 paths → 报错并指明 --scale
        let out2 = dir.path().join("icon2.svg");
        let app1x = Trace {
            image: src,
            color: false,
            polygon: true,
            scale: 1,
            out: Some(out2.display().to_string()),
        };
        let err = run_trace(&app1x, &ctx()).unwrap_err();
        assert!(
            err.to_string().contains("--scale"),
            "空 trace 应点名恢复手段: {err}"
        );
    }

    #[test]
    fn trace_writes_exact_utf8_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("multiline.svg");
        let payload = "<svg>\n<title>几何</title>\n<path/>\n</svg>\n";
        let reported = write_svg(&out, payload).unwrap();
        let written = std::fs::read(&out).unwrap();
        assert_eq!(written, payload.as_bytes(), "SVG 应保留精确 UTF-8 字节");
        assert_eq!(reported, written.len(), "报告字节数应等于磁盘字节数");
        assert!(
            reported > payload.chars().count(),
            "字节契约不能按字符数计算"
        );
    }

    // ── 对齐上游 test_html_shot.py（命名契约，路径差异已记录）─

    #[test]
    fn html_shot_default_naming() {
        // 无父目录 → 裸 stem.png；有父目录 → 输入同目录（我方约定，上游是 cwd）
        assert_eq!(
            default_output(Path::new("page.html"), ".png"),
            PathBuf::from("page.png")
        );
        let with_dir = default_output(Path::new("/tmp/a/b/page.html"), ".png");
        assert!(with_dir.to_string_lossy().ends_with("page.png"));
        assert!(with_dir.to_string_lossy().starts_with("/tmp/a/b/"));
    }
}
