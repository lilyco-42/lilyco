//! `dominant-colors` —— 缩图采样 + 贪心聚类，取主色。

use image::imageops::FilterType;
use image::RgbaImage;
use lilyco::prelude::*;
use std::path::PathBuf;
use std::time::Instant;

use crate::common::{load_rgba, parse_hex_color, parse_region};

// ── 4. dominant-colors ─────────────────────────────────────

/// 单色聚类状态（运行和，合并时求平均色）
#[derive(Clone, Copy)]
pub(crate) struct Cluster {
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
pub(crate) struct DominantColors {
    /// 输入图像文件
    #[arg(about = "Input image file", must_exist = true)]
    pub(crate) image: PathBuf,

    /// 返回的颜色数
    #[arg(default = 5, range = 1..=32)]
    pub(crate) top: u8,

    /// 聚类容差（0..=255，通道最大差，缺省 8 对齐上游 merge_tol）
    #[arg(default = 8, range = 0..=255)]
    pub(crate) tolerance: u8,

    /// 可选分析区域 "X1,Y1,X2,Y2"
    #[arg(about = "Optional pixel box X1,Y1,X2,Y2 to analyze")]
    pub(crate) region: Option<String>,

    /// pick 模式：逗号分隔的候选色（如 "#F9FAFA,#F5F5F5,#EDEDED"）
    #[arg(about = "Pick mode: comma-separated candidate palette #RRGGBB (e.g. #F9FAFA,#F5F5F5)")]
    pub(crate) candidates: Option<String>,
}

/// 业务逻辑：可选区域裁剪 → 缩略图 → 采样 → 贪心聚类 → 排序
pub(crate) fn run_dominant_colors(
    app: &DominantColors,
    ctx: &Context,
) -> Result<serde_json::Value, AppError> {
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
pub(crate) fn pick_colors(
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
            // 三通道差先提升到 u16，避免 debug 模式下 u8 相加溢出
            let d = (p[0].abs_diff(*cr) as u16
                + p[1].abs_diff(*cg) as u16
                + p[2].abs_diff(*cb) as u16) as f64
                / 3.0;
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
