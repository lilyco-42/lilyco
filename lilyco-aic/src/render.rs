//! `laic render` — 把姿势帧合成为 PNG / GIF / 精灵表（T0 只读）
//!
//! 变换语义照抄 `PxlMeshDrawer.makeMesh` → `RotaGraph`（游戏真正用来建网格的那段）：
//!
//! ```text
//! RotaGraph(uv, x = layer.x, y = -layer.y, sx = zmx, sy = zmy, rot = -rotR, w, h)
//!   num = round(srcW * uv.w);  num2 = round(srcH * uv.h)     ← 即 sprite 像素尺寸
//!   matrix = Translate(x/ppu, y/ppu) * Rotate(-rotR) * Scale(zmx, zmy)
//!   四边形落在 (-num/2, -num2/2, num, num2)，UV 与顶点一一对应（底左 ↔ 底左）
//! Col.a = (byte)(layer.alpha * 255f / 100f)
//! ```
//!
//! 落到**位图**（行 0 = 顶）时要把 Unity 的 Y 上翻过来：
//! `pngX = poseW/2 + q.x`、`pngY = poseH/2 - q.y`（q = 变换后的像素坐标）。
//! 采样用最近邻（`PxlImage.createFromPngRawData` 里 `filterMode = Point`，像素画必须点采样）。
//! 另外 `PxlLayer.readFromBytes` 有一条只在 `rotR == 0` 生效的奇偶修正
//! （sprite 宽/高为奇数时 x/y 各 +0.5），这里同样复刻。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lilyco::prelude::*;

use crate::atlas::{read_png, write_png, Img};
use crate::pxlslib::{Frame, Pxls};
use crate::util::{collect_targets, glob_match, load_pxls};

/// 渲染姿势帧
#[derive(App)]
#[app(
    name = "render",
    run = "run_render",
    about = "Composite PixelLiner pose frames into PNG images (and optionally an animated GIF per direction and/or a sprite sheet per pose) — i.e. actually DRAW the character instead of dumping numbers. Semantics are taken from the game's own mesh builder (`PxlMeshDrawer.makeMesh` -> `RotaGraph`): per layer the sprite is placed with its centre at (layer.x, layer.y) in a `pose.width x pose.height` canvas, scaled by (zmx, zmy), rotated by -rot_r, blended with alpha = layer.alpha/100, sampled nearest-neighbour (Point filter, required for pixel art). `root` is a pxls file or a directory scanned recursively; `pose` is a title glob (default `*`); `dir`/`frame` narrow to one direction / frame index. Writes `<out>/<table>.<pose>.d<dir>.f<frame>.png`; `anim` writes `<table>.<pose>.d<dir>.gif`; `sheet` writes `<table>.<pose>.sheet.png` (rows = directions, columns = frames). `scale` is an integer upscale. The atlas is paired automatically (`<pxls>.pxls.bytes.texture_<i>.dat`, or bare `.texture_0.png`, or an embedded PNG); `texture` overrides the file for atlas 0. Read-only (safety T0)."
)]
pub struct Render {
    /// pxls 文件或目录
    #[arg(about = "A .pxls file, or a directory scanned recursively for pxls tables", must_exist = true)]
    root: PathBuf,

    /// 输出目录
    #[arg(about = "Directory to write the rendered PNG/GIF/sheet files into", must_exist = false)]
    out: PathBuf,

    /// 姿势标题 glob（不区分大小写）
    #[arg(about = "Pose title glob filter (case-insensitive)", default = "*")]
    pose: String,

    /// 只渲染指定方向（0..=7）
    #[arg(about = "Only render direction N (0..=7; omit for all)", default = 99)]
    dir: u32,

    /// 只渲染指定帧序号（每个方向内 0 起）
    #[arg(about = "Only render frame index N within each direction (omit for all)", default = 99)]
    frame: u32,

    /// 整数倍放大输出
    #[arg(about = "Integer upscale factor for the output image", default = 1)]
    scale: u32,

    /// 每个方向额外输出一张 GIF
    #[arg(about = "Also write one animated GIF per direction", default = false)]
    anim: bool,

    /// 每个姿势额外输出一张精灵表
    #[arg(about = "Also write a sprite sheet per pose (rows = directions, columns = frames)", default = false)]
    sheet: bool,

    /// 显式指定 atlas 0 的贴图文件
    #[arg(about = "Explicit texture file for atlas 0 (overrides auto pairing)", default = "")]
    texture: String,

    /// 最多处理的文件数（0 = 不限）
    #[arg(about = "Cap the number of tables processed (0 = unlimited)", default = 0)]
    limit: u64,
}

/// 已解码图集 + UV 索引
struct AtlasSet {
    /// atlas index → 解码后的整图
    images: Vec<Option<Img>>,
    /// img key → (atlas index, 裁切矩形)
    uv: BTreeMap<String, (usize, [u32; 4])>,
}

fn run_render(app: &Render, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let targets = collect_targets(&app.root).map_err(AppError::InvalidArg)?;
    let targets = if app.limit > 0 {
        targets.into_iter().take(app.limit as usize).collect()
    } else {
        targets
    };
    std::fs::create_dir_all(&app.out)
        .map_err(|e| AppError::InvalidArg(format!("--out mkdir: {e}")))?;
    ctx.emit(Progress::Started {
        total: Some(targets.len() as u64),
        message: Some(format!("rendering {} tables", targets.len())),
    });

    let mut tables: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut frames_written = 0usize;
    let mut gifs_written = 0usize;
    let mut sheets_written = 0usize;

    for (ti, path) in targets.iter().enumerate() {
        ctx.tick(ti as u64, Some(targets.len() as u64), "");
        let file_str = path.display().to_string();
        let p = match load_pxls(path) {
            Ok(p) => p,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": e }));
                continue;
            }
        };
        if p.poses.is_empty() {
            continue;
        }
        let table = table_name(path);

        // 必须配对贴图才能画
        let mut set = match build_atlas_set(&p, path, app) {
            Ok(s) => s,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": e }));
                continue;
            }
        };

        let mut renders: Vec<serde_json::Value> = Vec::new();
        for pose in &p.poses {
            if !glob_match(&app.pose, &pose.title) {
                continue;
            }
            let s = app.scale.max(1) as f64;
            let cw = (pose.width as f64 * s).round().max(1.0) as u32;
            let ch = (pose.height as f64 * s).round().max(1.0) as u32;
            if cw == 0 || ch == 0 || cw > 4096 || ch > 4096 {
                continue;
            }
            let pose_tag = sanitize(&pose.title);

            // 精灵表缓冲：rows = 方向顺序，cols = 帧
            let mut sheet_frames: Vec<(i32, u32, Img)> = Vec::new();

            for seq in &pose.seqs {
                if app.dir != 99 && seq.dir as u32 != app.dir {
                    continue;
                }
                let mut gif_frames: Vec<(u16, Img)> = Vec::new();

                for (fi, frame) in seq.frames.iter().enumerate() {
                    if app.frame != 99 && fi as u32 != app.frame {
                        continue;
                    }
                    let canvas = render_frame(&mut set, frame, cw, ch, s, pose.width, pose.height)
                        .map_err(|e| AppError::InvalidArg(format!("{file_str}: {e}")))?;
                    let fname = format!("{table}.{pose_tag}.d{}.f{fi}.png", seq.dir);
                    let fpath = app.out.join(&fname);
                    if let Err(e) = write_png(&fpath, cw, ch, &canvas.rgba) {
                        errors.push(serde_json::json!({ "file": file_str, "error": e }));
                        continue;
                    }
                    frames_written += 1;
                    renders.push(serde_json::json!({
                        "pose": pose.title, "dir": seq.dir, "frame": fi,
                        "file": fpath.display().to_string(), "width": cw, "height": ch,
                    }));
                    if app.anim {
                        gif_frames.push((frame.crf60.max(1) as u16, canvas.clone()));
                    }
                    if app.sheet {
                        sheet_frames.push((seq.dir, fi as u32, canvas));
                    }
                }

                if app.anim && !gif_frames.is_empty() {
                    let gp = app.out.join(format!("{table}.{pose_tag}.d{}.gif", seq.dir));
                    match write_gif(&gp, &gif_frames) {
                        Ok(()) => gifs_written += 1,
                        Err(e) => errors.push(serde_json::json!({ "file": file_str, "error": e })),
                    }
                }
            }

            if app.sheet && !sheet_frames.is_empty() {
                match write_sheet(&app.out.join(format!("{table}.{pose_tag}.sheet.png")), &sheet_frames, cw, ch) {
                    Ok(()) => sheets_written += 1,
                    Err(e) => errors.push(serde_json::json!({ "file": file_str, "error": e })),
                }
            }
        }

        tables.push(serde_json::json!({
            "file": file_str,
            "table": table,
            "poses_rendered": renders.len(),
            "frames": renders,
        }));
    }

    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "out": app.out.display().to_string(),
        "pose_filter": app.pose,
        "scale": app.scale.max(1),
        "tables_scanned": targets.len(),
        "frames_written": frames_written,
        "gifs_written": gifs_written,
        "sheets_written": sheets_written,
        "tables": tables,
        "errors": errors,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// 解码图集并建立 img key → 裁切矩形索引
///
/// 同一 img key 可能出现在多个图集里（PARTS 图集继承前一个图集的 UV 表 ⇒ 同一套 rect、
/// 另一张贴图）。这里**取第一个**：图集 0 是正常图，后面的部件变体默认不参与合成。
fn build_atlas_set(p: &Pxls, path: &Path, app: &Render) -> Result<AtlasSet, String> {
    if p.atlas.is_empty() {
        return Err("no %PACK_SECTION% (nothing to crop)".into());
    }
    // 贴图来源一次解析完：第 i 个图集 ↔ `<stem>.pxls.bytes.texture_0.dat` 里的第 i 张 Texture2D
    let sources = crate::atlas::resolve_atlas_textures(path, p.atlas.len());
    let mut set = AtlasSet { images: vec![None; p.atlas.len()], uv: BTreeMap::new() };

    for (ai, at) in p.atlas.iter().enumerate() {
        let m = at.margin as u32;
        for uv in p.effective_uvs(ai) {
            let (w, h) = (uv.w.saturating_sub(m * 2), uv.h.saturating_sub(m * 2));
            if w > 0 && h > 0 {
                set.uv
                    .entry(uv.img.clone())
                    .or_insert((ai, [uv.x + m, uv.y + m, w, h]));
            }
        }
    }

    // 只解码姿势实际用到的图集（4096×4096 的 BC7 解码很贵）
    let mut wanted: Vec<usize> = p
        .poses
        .iter()
        .filter(|po| glob_match(&app.pose, &po.title))
        .flat_map(|po| po.seqs.iter())
        .flat_map(|s| s.frames.iter())
        .flat_map(|f| f.layers.iter())
        .filter_map(|l| set.uv.get(&l.img).map(|(ai, _)| *ai))
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    if wanted.is_empty() {
        wanted.push(0);
    }

    for ai in wanted {
        let at = &p.atlas[ai];
        let img = if let Some((ew, eh)) = at.external_wh {
            let src = if ai == 0 && !app.texture.is_empty() {
                crate::atlas::TextureSource::File { path: PathBuf::from(&app.texture) }
            } else {
                sources.get(ai).and_then(|s| s.clone()).ok_or_else(|| {
                    format!("external atlas {ai} claims {ew}x{eh} but no texture is available for it")
                })?
            };
            src.read()?
        } else if let Some(png) = &at.embedded_png {
            read_png(png, "embedded atlas")?
        } else {
            continue;
        };
        set.images[ai] = Some(img);
    }
    Ok(set)
}

/// 合成一帧。`cw`/`ch` 是画布像素尺寸（已含 scale），`pw`/`ph` 是 pose 的逻辑宽高。
fn render_frame(
    set: &mut AtlasSet,
    frame: &Frame,
    cw: u32,
    ch: u32,
    s: f64,
    pw: u16,
    ph: u16,
) -> Result<Img, String> {
    let mut canvas = Img::blank(cw, ch);
    let cx = cw as f64 / 2.0;
    let cy = ch as f64 / 2.0;
    let _ = (pw, ph);

    for l in &frame.layers {
        if l.group || l.alpha <= 0 {
            continue;
        }
        let Some((ai, rect)) = set.uv.get(&l.img).copied() else { continue };
        let Some(atlas) = set.images.get(ai).and_then(|o| o.as_ref()) else { continue };

        let (sw, sh, sx0, sy0) = (rect[2], rect[3], rect[0], rect[1]);
        let sprite = atlas.crop(sx0, sy0, sw, sh);

        // PxlLayer.readFromBytes 的奇偶修正（仅 rotR == 0）
        let mut x0 = l.x;
        let mut y0 = l.y;
        if l.rot_r == 0.0 {
            if sw % 2 == 1 {
                x0 += 0.5;
            }
            if sh % 2 == 1 {
                y0 += 0.5;
            }
        }

        // 画布像素坐标（含 scale）
        let x0p = x0 * s;
        let y0p = y0 * s;
        let num = sw as f64 * s;
        let num2 = sh as f64 * s;
        let theta = -l.rot_r; // Unity Y-up，逆时针为正
        let (ct, st) = (theta.cos(), theta.sin());
        let zmx = l.zmx;
        let zmy = l.zmy;
        if zmx == 0.0 || zmy == 0.0 {
            continue;
        }
        let a_layer = (l.alpha as f64 * 255.0 / 100.0).floor().clamp(0.0, 255.0) as u32;
        if a_layer == 0 {
            continue;
        }
        let inv_zx = 1.0 / zmx;
        let inv_zy = 1.0 / zmy;

        // 包围盒（把 sprite 四角投到画布上）
        let mut minx = f64::INFINITY;
        let mut maxx = f64::NEG_INFINITY;
        let mut miny = f64::INFINITY;
        let mut maxy = f64::NEG_INFINITY;
        for (ux, uy) in [
            (-num / 2.0, -num2 / 2.0),
            (-num / 2.0, num2 / 2.0),
            (num / 2.0, num2 / 2.0),
            (num / 2.0, -num2 / 2.0),
        ] {
            let vx = ux * zmx;
            let vy = uy * zmy;
            let rx = vx * ct - vy * st;
            let ry = vx * st + vy * ct;
            let png_x = cx + x0p + rx;
            let png_y = cy - (y0p + ry);
            minx = minx.min(png_x);
            maxx = maxx.max(png_x);
            miny = miny.min(png_y);
            maxy = maxy.max(png_y);
        }
        let bx0 = (minx.floor().max(0.0)) as u32;
        let bx1 = (maxx.ceil().min(cw as f64)) as u32;
        let by0 = (miny.floor().max(0.0)) as u32;
        let by1 = (maxy.ceil().min(ch as f64)) as u32;

        for py in by0..by1 {
            for px in bx0..bx1 {
                // 画布像素中心 → Unity Y-up 的相对坐标
                let qx = px as f64 + 0.5 - cx - x0p;
                let qy = cy - (py as f64 + 0.5) - y0p;
                // 逆旋转（R(-θ)）
                let vx = qx * ct + qy * st;
                let vy = -qx * st + qy * ct;
                // 逆缩放 → sprite 局部像素
                let ux = vx * inv_zx;
                let uy = vy * inv_zy;
                let col = ux + num / 2.0;
                let row = num2 / 2.0 - uy;
                if col < 0.0 || row < 0.0 || col >= num || row >= num2 {
                    continue;
                }
                // 最近邻回落到原图素
                let sc = (col / s).floor() as i64;
                let sr = (row / s).floor() as i64;
                if sc < 0 || sr < 0 || sc >= sw as i64 || sr >= sh as i64 {
                    continue;
                }
                let si = ((sr as usize) * sw as usize + sc as usize) * 4;
                let sa = sprite.rgba[si + 3] as u32 * a_layer / 255;
                if sa == 0 {
                    continue;
                }
                let di = ((py as usize) * cw as usize + px as usize) * 4;
                blend_src_over(&mut canvas.rgba[di..di + 4], &sprite.rgba[si..si + 4], sa);
            }
        }
    }
    Ok(canvas)
}

/// 非预乘 SRC_OVER：src 用 sa 缩放后与 dst 合成
#[inline]
fn blend_src_over(dst: &mut [u8], src: &[u8], a: u32) {
    let da = dst[3] as u32;
    let out_a = a + da * (255 - a) / 255;
    if out_a == 0 {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }
    for c in 0..3 {
        let s = src[c] as u32 * a;
        let d = dst[c] as u32 * da * (255 - a) / 255;
        dst[c] = ((s + d) / out_a).min(255) as u8;
    }
    dst[3] = out_a.min(255) as u8;
}

fn write_gif(path: &Path, frames: &[(u16, Img)]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let f0 = &frames[0].1;
    let file = std::fs::File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let mut enc = gif::Encoder::new(std::io::BufWriter::new(file), f0.w as u16, f0.h as u16, &[])
        .map_err(|e| format!("gif encoder: {e}"))?;
    enc.set_repeat(gif::Repeat::Infinite).map_err(|e| format!("gif repeat: {e}"))?;
    for (crf60, img) in frames {
        if img.w != f0.w || img.h != f0.h {
            continue;
        }
        let mut rgba = img.rgba.clone();
        let mut f = gif::Frame::from_rgba_speed(img.w as u16, img.h as u16, &mut rgba, 10);
        // crf60 是 1/60 秒 → 百分之一秒
        f.delay = ((*crf60 as u32 * 100) / 60).max(1) as u16;
        enc.write_frame(&f).map_err(|e| format!("gif frame: {e}"))?;
    }
    Ok(())
}

fn write_sheet(path: &Path, frames: &[(i32, u32, Img)], cw: u32, ch: u32) -> Result<(), String> {
    let mut dirs: Vec<i32> = frames.iter().map(|(d, _, _)| *d).collect();
    dirs.sort_unstable();
    dirs.dedup();
    let cols = frames.iter().map(|(_, f, _)| *f).max().unwrap_or(0) + 1;
    if cols == 0 || dirs.is_empty() {
        return Err("empty sheet".into());
    }
    let w = cw * cols;
    let h = ch * dirs.len() as u32;
    if w > 16384 || h > 16384 {
        return Err(format!("sheet too large: {w}x{h}"));
    }
    let mut sheet = Img::blank(w, h);
    for (d, f, img) in frames {
        let di = dirs.iter().position(|x| x == d).unwrap() as u32;
        let ox = f * cw;
        let oy = di * ch;
        for row in 0..ch.min(img.h) {
            let s0 = ((row as usize) * img.w as usize) * 4;
            let s1 = s0 + (cw.min(img.w) as usize) * 4;
            let d0 = (((oy + row) as usize) * w as usize + ox as usize) * 4;
            let d1 = d0 + (cw.min(img.w) as usize) * 4;
            sheet.rgba[d0..d1].copy_from_slice(&img.rgba[s0..s1]);
        }
    }
    write_png(path, w, h, &sheet.rgba)
}

fn table_name(path: &Path) -> String {
    let n = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = n.split(".pxls").next().unwrap_or(&n).to_string();
    sanitize(if stem.is_empty() { &n } else { &stem })
}

fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let s = s.trim_start_matches(['.', '/']).to_string();
    if s.is_empty() { "unnamed".to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn src_over_is_correct_for_opaque_and_transparent() {
        let mut d = [0u8, 0, 0, 0];
        blend_src_over(&mut d, &[255, 0, 0, 255], 255);
        assert_eq!(d, [255, 0, 0, 255]);
        // 半透明红（a=128）压在不透明蓝上：alpha 变满，红略多于蓝（非预乘 SRC_OVER）
        let mut d = [0u8, 0, 255, 255];
        blend_src_over(&mut d, &[255, 0, 0, 255], 128);
        assert_eq!(d[3], 255, "源半透明 + 底不透明 ⇒ 结果仍不透明");
        assert_eq!(d[0], 128);
        assert_eq!(d[2], 127);
        assert!(d[0] > d[2], "混合后应偏红而不是偏蓝");
    }

    #[test]
    fn missing_root_is_invalid_arg() {
        let app = Render {
            root: PathBuf::from("nope/zzz/qq"),
            out: PathBuf::from("x"),
            pose: "*".into(),
            dir: 99,
            frame: 99,
            scale: 1,
            anim: false,
            sheet: false,
            texture: String::new(),
            limit: 0,
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        let err = run_render(&app, &Context::new_test(tx)).unwrap_err();
        assert!(matches!(err, AppError::InvalidArg(_)));
    }
}
