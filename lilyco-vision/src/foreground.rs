//! `extract-foreground` —— 从边界连通域洪水填充标背景，输出透明 RGBA PNG。
//!
//! 确定性近似（比上游算法简单但可复现）：`color` 模式按与边界参考色的通道距离、
//! `dark` 模式按亮度阈值判背景，都要求与边界连通才算背景。

use image::{Rgba, RgbaImage};
use lilyco::prelude::*;
use std::path::PathBuf;
use std::time::Instant;

use crate::common::{
    default_output, load_rgba, parse_hex_color, save_png, BG_TOLERANCE, DARK_LUMINANCE_THRESHOLD,
};

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
pub(crate) struct ExtractForeground {
    /// 输入图像文件（可多个，各输出 {stem}.clean.png）
    #[arg(about = "Input image files (one or more, each written to {stem}.clean.png)")]
    pub(crate) images: Vec<String>,

    /// 背景判定模式
    #[arg(default = "color", about = "Background mode: \"color\" or \"dark\"")]
    pub(crate) mode: String,

    /// 手动搜索区域 "X1,Y1,X2,Y2"（缺省：整图边界洪水填充）
    #[arg(about = "Manual search region X1,Y1,X2,Y2 (default: whole image border-flood)")]
    pub(crate) region: Option<String>,

    /// 排除色 "#RRGGBB"（color 模式：距离 ≤ exclude_tol 视为背景）
    #[arg(about = "Explicit background color #RRGGBB (color mode)")]
    pub(crate) exclude_color: Option<String>,

    /// 排除色容差（0..=255，通道最大差，缺省 35 对齐上游）
    #[arg(default = 35, range = 0..=255)]
    pub(crate) exclude_tol: u8,

    /// 输出路径（缺省：输入同目录 {stem}.clean.png；多图时忽略）
    #[arg(about = "Output RGBA PNG path (single-image mode only)")]
    pub(crate) out: Option<String>,
}

/// 边界连通域洪水填充：返回背景掩码（true = 背景）
pub(crate) fn flood_background(img: &RgbaImage, is_bg: impl Fn(u32, u32) -> bool) -> Vec<bool> {
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
pub(crate) fn run_extract_foreground(
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
pub(crate) fn connected_components(w: u32, h: u32, mask: &[bool]) -> Vec<Vec<usize>> {
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
pub(crate) fn comp_bbox(comp: &[usize], w: u32) -> (u32, u32, u32, u32) {
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
pub(crate) fn prune_detached_foreground(img: &mut RgbaImage, w: u32, h: u32) {
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
