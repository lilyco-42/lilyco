//! `pixel-diff` —— 原图 vs 重建图的网格级差异排名 + 可选红色热力图。

use image::imageops::FilterType;
use image::{ImageBuffer, Rgba, RgbaImage};
use lilyco::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::common::{default_output, flatten_on_white, load_rgba, save_png};

// ── 5. pixel-diff ──────────────────────────────────────────

/// 重建图网格化对比原图：逐格平均通道差，按得分排序，可选输出红色热力图
#[derive(App)]
#[app(
    run = "run_pixel_diff",
    about = "Diff an original image against a rebuilt one: resize the rebuilt to the original size, split both into a grid x grid, rank cells by mean absolute channel difference, and return { grid, mean_diff, worst: [{x1,y1,x2,y2,score}] } in original-image pixel coordinates; optionally write a red-intensity PNG heatmap."
)]
pub(crate) struct PixelDiff {
    /// 原图
    #[arg(about = "Original image file", must_exist = true)]
    pub(crate) original: PathBuf,

    /// 重建图
    #[arg(about = "Rebuilt/regenerated image file", must_exist = true)]
    pub(crate) rebuilt: PathBuf,

    /// 网格数（每边）
    #[arg(default = 6, range = 1..=32)]
    pub(crate) grid: u8,

    /// 返回的“最差格子”数量
    #[arg(default = 5, range = 1..=16)]
    pub(crate) top: u8,

    /// 可选热力图输出路径（默认：输入同目录 {stem}-heatmap.png）
    #[arg(about = "Optional red-intensity heatmap PNG path")]
    pub(crate) out_heatmap: Option<String>,
}

/// 业务逻辑：对齐尺寸 → 逐格平均差排名 → 可选热力图
pub(crate) fn run_pixel_diff(
    app: &PixelDiff,
    ctx: &Context,
) -> Result<serde_json::Value, AppError> {
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
pub(crate) fn write_heatmap(
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
