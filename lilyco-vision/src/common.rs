//! 公共辅助：读图 / 存图 / 默认输出名 / `--region` 解析 / 抠白底 / 十六进制色解析。
//! 这些是各命令共用的「与图像域无关、与 lilyco 无关」的小工具。

use image::{ImageBuffer, Rgba, RgbaImage};
use lilyco::prelude::*;
use std::path::{Path, PathBuf};

/// color 模式判定“背景”的颜色容差（通道差上限，确定性近似）
pub(crate) const BG_TOLERANCE: u8 = 48;
/// dark 模式的亮度阈值（低于该值视为背景）
pub(crate) const DARK_LUMINANCE_THRESHOLD: u8 = 60;

/// 输出文件默认路径：输入同目录 + stem + 后缀
pub(crate) fn default_output(input: &Path, suffix: &str) -> PathBuf {
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
pub(crate) fn parse_region(s: &str, w: u32, h: u32) -> Result<[u32; 4], AppError> {
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
pub(crate) fn flatten_on_white(img: &RgbaImage) -> RgbaImage {
    ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        let a = p[3] as u32;
        let mix = |c: u8| -> u8 { ((c as u32 * a + 255 * (255 - a)) / 255) as u8 };
        Rgba([mix(p[0]), mix(p[1]), mix(p[2]), 255])
    })
}

/// 解码为 RGBA（统一后续处理的数据形态）
pub(crate) fn load_rgba(p: &Path) -> Result<RgbaImage, AppError> {
    let d =
        image::open(p).map_err(|e| AppError::Runtime(format!("decode {}: {e}", p.display())))?;
    Ok(d.to_rgba8())
}

/// 保存 PNG（格式由文件扩展名推导）
pub(crate) fn save_png(img: &RgbaImage, p: &Path) -> Result<(), AppError> {
    img.save(p)
        .map_err(|e| AppError::Runtime(format!("write {}: {e}", p.display())))
}

/// 解析 "#RRGGBB" 颜色
pub(crate) fn parse_hex_color(s: &str) -> Result<(u8, u8, u8), AppError> {
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
