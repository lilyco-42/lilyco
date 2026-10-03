//! `laic sprites` — 按图集 UV 裁切导出每个 sprite 的 PNG（T0 只读）
//!
//! 对标 AICPxlsUnpacker 与 pixelliner4j 的 `getImageByKey`，但**更进一步**：
//! - 裁切矩形取自 `%PACK_SECTION%` 的 UV 表（结构化解析），不是字节模式硬找；
//! - 文件按**姿势层名**命名（`body` / `head_s` / `rod` …），而不是 `EDI29_2` 这种 key
//!   （AICPxlsUnpacker 要靠回扫 POSE 节猜名字，pixelliner4j 只在内存里查）；
//! - 外部贴图走 `.texture_0.dat` 的 UnityFS+Texture2D 全解码（BC1/BC3/BC7/ASTC/…），
//!   旧版裸 `.texture_0.png` 与内嵌 PNG 同样支持。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lilyco::prelude::*;

use crate::atlas::{read_png, write_png, Img};
use crate::pxlslib::Pxls;
use crate::util::{collect_targets, load_pxls};

/// 导出 sprite 裁切图
#[derive(App)]
#[app(
    name = "sprites",
    run = "run_sprites",
    about = "Export every sprite of PixelLiner .pxls character tables as an individual PNG by cropping the packed atlas with the `%PACK_SECTION%` UV table (the data a sprite/mod pipeline needs). For each table under `root` (file or directory, scanned recursively) it pairs every atlas entry with its pixels the way the game does: atlas index i takes the Texture2D whose asset name ends with `_i` inside `<stem>.pxls.bytes.texture_0.dat` (the game packs both `texture_0` and the parts `texture_1` in that one bundle — matched by name, never by object order), falling back to a separate `<stem>.pxls.bytes.texture_<i>` file or an embedded PNG. Decoding covers DXT5/BC3, DXT1/BC1, BC7, BC4/BC5, DXT1/5Crunched, RGBA32, RGB24, RGB565, ARGB4444, RGBA4444, BGRA32 and ASTC. Output is `<out>/<table>/<layer-name>.png`. Sprites are named after the POSE LAYER that uses the image (falling back to the raw EDI<hex>_<id2> key when no layer references it); sprites taken from a secondary atlas get an `.a<i>` suffix. A PARTS atlas whose UV count is 0 inherits the previous atlas's UV table (same rects, different texture — the torn/clothing-overlay variant), exactly like `PxlsImgAtlas.readFromBytes` does. `margin` from the atlas entry is honoured (`x+margin, y+margin, w-2m, h-2m`) and the crop uses PNG top-left origin (no Y flip). Flags: `atlas` also dumps the whole decoded atlas, `embedded` also writes PNGs embedded in the `%IMGS_SECTION%`. Read-only (safety T0)."
)]
pub struct Sprites {
    /// pxls 文件或目录
    #[arg(about = "A .pxls file, or a directory scanned recursively for pxls tables", must_exist = true)]
    root: PathBuf,

    /// 导出目录（必填）
    #[arg(about = "Directory to write the cropped sprite PNG files into", must_exist = false)]
    out: PathBuf,

    /// 姿势标题 glob 过滤（只裁该姿势用到的 sprite）
    #[arg(about = "Only export sprites used by poses whose title matches this glob", default = "")]
    pose: String,

    /// 额外导出整张解码后的图集
    #[arg(about = "Also write the whole decoded atlas as <table>.atlas_<i>.png", default = false)]
    atlas: bool,

    /// 额外导出 IMGS 节内嵌的 PNG
    #[arg(about = "Also write PNGs embedded in the %IMGS_SECTION%", default = false)]
    embedded: bool,

    /// 最多处理的文件数（0 = 不限）
    #[arg(about = "Cap the number of tables processed (0 = unlimited)", default = 0)]
    limit: u64,
}

const IMGS_PREFIX: &str = "imgs";

fn run_sprites(app: &Sprites, ctx: &Context) -> Result<serde_json::Value, AppError> {
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
        message: Some(format!("cropping sprites from {} tables", targets.len())),
    });

    let mut tables: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut warnings: Vec<serde_json::Value> = Vec::new();
    let mut total_sprites = 0usize;

    for (i, path) in targets.iter().enumerate() {
        ctx.tick(i as u64, Some(targets.len() as u64), "");
        let file_str = path.display().to_string();
        let p = match load_pxls(path) {
            Ok(p) => p,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": e }));
                continue;
            }
        };
        let table = table_name(path);
        let out_dir = app.out.join(&table);

        // 图层名表：img key → 层名（供命名）
        let names = layer_names(&p, &app.pose);

        let mut sprites: Vec<serde_json::Value> = Vec::new();
        let mut atlas_used: Vec<serde_json::Value> = Vec::new();
        let mut used: BTreeMap<String, usize> = BTreeMap::new();

        // 贴图来源一次解析完（bundle 只解一次）：第 i 个图集 ↔ 主包里的第 i 张 Texture2D
        let sources = crate::atlas::resolve_atlas_textures(path, p.atlas.len());

        for (ai, at) in p.atlas.iter().enumerate() {
            // 每个图集条目都报一次（含没有贴图的），不然「为什么少了 sprite」无从查起
            let uvs = p.effective_uvs(ai);
            let mut meta = serde_json::json!({
                "index": ai,
                "img_type": crate::pxlslib::atlas_img_type(at),
                "img_type_name": crate::pxlslib::img_type_name(crate::pxlslib::atlas_img_type(at)),
                "flags": at.flags,
                "external": at.flags & 1 == 1,
                "margin": at.margin,
                "uvs": at.uvs.len(),
                "uvs_effective": uvs.len(),
                "uv_inherited_from": p.uv_inherited_from(ai),
                "declared": at.external_wh,
                "embedded_png_bytes": at.embedded_png.as_ref().map(|v| v.len()).unwrap_or(0),
                "decoded": false,
            });
            let img: Img = if let Some((ew, eh)) = at.external_wh {
                match sources.get(ai).and_then(|s| s.clone()) {
                    Some(src) => match src.read() {
                        Ok(im) => {
                            meta["source"] = serde_json::json!(src.describe());
                            // 取的是包里哪个 asset —— 对象表顺序可能与名字编号不一致
                            // （`noel_t` 的 `texture_1` 就排在 `texture_0` 前面），排查时很有用
                            meta["asset"] = serde_json::json!(src.asset_name());
                            meta["width"] = serde_json::json!(im.w);
                            meta["height"] = serde_json::json!(im.h);
                            meta["decoded"] = serde_json::json!(true);
                            atlas_used.push(meta);
                            im
                        }
                        Err(e) => {
                            meta["error"] = serde_json::json!(e.clone());
                            atlas_used.push(meta);
                            errors.push(serde_json::json!({ "file": file_str, "atlas": ai, "error": e }));
                            continue;
                        }
                    },
                    None => {
                        let note = format!(
                            "external atlas {ai} claims {ew}x{eh} but this dump has no texture for it \
                             (no #{ai} Texture2D in <stem>.pxls.bytes.texture_0.dat and no texture_{ai} file)"
                        );
                        meta["note"] = serde_json::json!(note.clone());
                        atlas_used.push(meta);
                        // 数据缺口（本作的 `_icons` / `noel_bassrobe` / `house` 三张表整份没有配套贴图），
                        // 不是解析失败 —— 记警告，不污染 errors。
                        warnings.push(serde_json::json!({
                            "file": file_str, "atlas": ai,
                            "uvs": uvs.len(), "warning": note,
                        }));
                        continue;
                    }
                }
            } else if let Some(png) = &at.embedded_png {
                match read_png(png, "embedded atlas") {
                    Ok(im) => {
                        meta["source"] = serde_json::json!("embedded");
                        meta["width"] = serde_json::json!(im.w);
                        meta["height"] = serde_json::json!(im.h);
                        meta["decoded"] = serde_json::json!(true);
                        atlas_used.push(meta);
                        im
                    }
                    Err(e) => {
                        meta["error"] = serde_json::json!(e.clone());
                        atlas_used.push(meta);
                        errors.push(serde_json::json!({ "file": file_str, "atlas": ai, "error": e }));
                        continue;
                    }
                }
            } else {
                atlas_used.push(meta);
                continue;
            };

            if app.atlas {
                let ap = out_dir.join(format!("{table}.atlas_{ai}.png"));
                if let Err(e) = write_png(&ap, img.w, img.h, &img.rgba) {
                    errors.push(serde_json::json!({ "file": file_str, "error": e }));
                }
            }

            let m = at.margin as u32;
            for uv in uvs {
                // 只导姿势实际用到的 sprite（给了 --pose 过滤时）
                if !app.pose.is_empty() && !names.contains_key(&uv.img) {
                    continue;
                }
                let (w, h) = (uv.w.saturating_sub(m * 2), uv.h.saturating_sub(m * 2));
                if w == 0 || h == 0 {
                    continue;
                }
                let base = names.get(&uv.img).cloned().unwrap_or_else(|| uv.img.clone());
                let crop = img.crop(uv.x + m, uv.y + m, w, h);
                // 继承 UV 的部件图集：命名带上 atlas 序号，免得和主图集的同名 sprite 打架
                let base = if ai > 0 { format!("{base}.a{ai}") } else { base };
                let out_path = unique_path(&out_dir, &base, &uv.img, &mut used);
                match write_png(&out_path, crop.w, crop.h, &crop.rgba) {
                    Ok(()) => {
                        total_sprites += 1;
                        sprites.push(serde_json::json!({
                            "img": uv.img,
                            "name": base,
                            "atlas": ai,
                            "file": out_path.display().to_string(),
                            "source_rect": [uv.x + m, uv.y + m, w, h],
                            "width": w,
                            "height": h,
                        }));
                    }
                    Err(e) => errors
                        .push(serde_json::json!({ "file": file_str, "img": uv.img, "error": e })),
                }
            }
        }

        if app.embedded {
            for e in &p.embedded {
                for (slot, png) in [("i", &e.i_png), ("p", &e.p_png)] {
                    let Some(png) = png else { continue };
                    if png.is_empty() {
                        continue;
                    }
                    let base = format!("{IMGS_PREFIX}.{}", names.get(&e.img).cloned().unwrap_or_else(|| e.img.clone()));
                    let out_path = unique_path(&out_dir, &format!("{base}.{slot}"), &e.img, &mut used);
                    match read_png(png, "embedded image") {
                        Ok(im) => match write_png(&out_path, im.w, im.h, &im.rgba) {
                            Ok(()) => {
                                total_sprites += 1;
                                sprites.push(serde_json::json!({
                                    "img": e.img, "name": base, "slot": slot,
                                    "file": out_path.display().to_string(),
                                    "width": im.w, "height": im.h,
                                }));
                            }
                            Err(er) => errors
                                .push(serde_json::json!({ "file": file_str, "error": er })),
                        },
                        Err(er) => errors
                            .push(serde_json::json!({ "file": file_str, "error": er })),
                    }
                }
            }
        }

        tables.push(serde_json::json!({
            "file": file_str,
            "table": table,
            "out_dir": out_dir.display().to_string(),
            "atlases": atlas_used,
            "sprites": sprites.len(),
            "entries": sprites,
        }));
    }

    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "out": app.out.display().to_string(),
        "tables_scanned": targets.len(),
        "tables_exported": tables.len(),
        "sprites_written": total_sprites,
        "tables": tables,
        "warnings": warnings,
        "errors": errors,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// `noel.pxls.dat` → `noel`；`boss_nusi.pxls.bytes` → `boss_nusi`
fn table_name(path: &Path) -> String {
    let n = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = n.split(".pxls").next().unwrap_or(&n).to_string();
    if stem.is_empty() {
        sanitize(&n)
    } else {
        sanitize(&stem)
    }
}

/// img key → 层名（同 key 多解时取第一个非空名）
fn layer_names(p: &Pxls, pose_glob: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for pose in &p.poses {
        if !pose_glob.is_empty() && !crate::util::glob_match(pose_glob, &pose.title) {
            continue;
        }
        for s in &pose.seqs {
            for f in &s.frames {
                for l in &f.layers {
                    if l.name.is_empty() {
                        continue;
                    }
                    out.entry(l.img.clone()).or_insert_with(|| l.name.clone());
                }
            }
        }
    }
    out
}

/// 同名去重：首次用层名，重名时追加 img key，再冲突再加序号
fn unique_path(dir: &Path, base: &str, key: &str, used: &mut BTreeMap<String, usize>) -> PathBuf {
    let base = sanitize(base);
    let key = sanitize(key);
    let mut name = if used.contains_key(&base) {
        format!("{base}.{key}")
    } else {
        base.clone()
    };
    let mut i = 2;
    while used.contains_key(&name) {
        name = format!("{base}.{key}.{i}");
        i += 1;
    }
    used.insert(name.clone(), 1);
    dir.join(format!("{name}.png"))
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
    fn table_name_strips_pxls_suffix() {
        assert_eq!(table_name(Path::new("a/b/noel.pxls.dat")), "noel");
        assert_eq!(table_name(Path::new("a/b/boss_nusi.pxls.bytes")), "boss_nusi");
        assert_eq!(table_name(Path::new("a/b/x.pxls")), "x");
    }

    /// 生成的文件名绝不能带路径分隔符或以点开头 —— 这是唯一的防穿越防线
    #[test]
    fn sanitize_blocks_path_traversal() {
        for raw in [
            "../../etc/passwd",
            "..\\..\\windows\\system32",
            "/abs/path",
            "a/b/c",
            "..",
            "",
        ] {
            let s = sanitize(raw);
            assert!(!s.is_empty(), "{raw} → 空名");
            assert!(!s.contains('/') && !s.contains('\\'), "{raw} → {s} 仍带分隔符");
            assert!(!s.starts_with('.'), "{raw} → {s} 以点开头");
            assert!(!s.contains(':'), "{raw} → {s} 带盘符冒号");
        }
        assert_eq!(sanitize("body"), "body");
        assert_eq!(sanitize("head_s.2"), "head_s.2");
    }

    #[test]
    fn missing_root_is_invalid_arg() {
        let app = Sprites {
            root: PathBuf::from("nope/zzz/qq"),
            out: PathBuf::from("x"),
            pose: String::new(),
            atlas: false,
            embedded: false,
            limit: 0,
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        let err = run_sprites(&app, &Context::new_test(tx)).unwrap_err();
        assert!(matches!(err, AppError::InvalidArg(_)));
    }
}
