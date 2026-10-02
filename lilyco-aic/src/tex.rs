//! `laic tex` — Texture2D 清单与贴图导出（T0 只读）
//!
//! 遍历 `root` 下的 `*.texture_0.dat`（UnityFS 包着 v22 SerializedFile），
//! 列出每个包内的 Texture2D（名字/尺寸/格式/存储方式），`--out` 时解码出 PNG：
//! - fmt 4 (RGBA32)：内联 `image data` 或 `.resS` 资源流
//! - fmt 12 (DXT1/BC1)、fmt 13 (DXT5/BC3)：texture2ddecoder 解码
//!
//! 姿势贴图调色的数据底座——pxls 层的 img id ↔ 贴图块映射要靠它落地。

use std::path::{Path, PathBuf};
use std::time::Instant;

use lilyco::prelude::*;

use crate::serialized::SerializedFile;
use crate::unityfs;
use crate::util::collect_tex_targets;

/// 列出 / 导出 Texture2D
#[derive(App)]
#[app(
    name = "tex",
    run = "run_tex",
    about = "List Texture2D assets inside all `*.texture_0.dat` UnityFS bundles under `root` (file or directory, scanned recursively), or decode them to PNG files with `--out`. Each texture reports name, size, format (4=RGBA32, 12=DXT1/BC1, 13=DXT5/BC3), storage (inline / .resS stream) and bundle path. PNG export decodes RGBA32 directly and BC1/BC3 via texture2ddecoder, writing `<name>.png` into `--out`. Read-only (safety T0)."
)]
pub struct Tex {
    /// 搜索根目录（或单文件）
    #[arg(about = "A .texture_0.dat file, or a directory scanned recursively for texture bundles", must_exist = true)]
    root: PathBuf,

    /// 贴图名 glob 过滤（匹配 Texture2D 名，不区分大小写）
    #[arg(about = "Texture2D name glob filter, e.g. 'noel*'", default = "")]
    name: String,

    /// 导出 PNG 的目标目录（缺省只列清单不导出）
    #[arg(about = "Directory to write decoded PNG files into (optional)", default = "")]
    out: String,

    /// 最多处理的包数（0 = 不限）
    #[arg(about = "Cap the number of bundles processed (0 = unlimited)", default = 0)]
    limit: u64,
}

fn run_tex(app: &Tex, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let targets = collect_tex_targets(&app.root).map_err(AppError::InvalidArg)?;
    let targets = if app.limit > 0 {
        targets.into_iter().take(app.limit as usize).collect()
    } else {
        targets
    };
    ctx.emit(Progress::Started {
        total: Some(targets.len() as u64),
        message: Some(format!("scanning {} texture bundles", targets.len())),
    });

    let out_dir = if app.out.is_empty() {
        None
    } else {
        let p = PathBuf::from(&app.out);
        std::fs::create_dir_all(&p).map_err(|e| AppError::InvalidArg(format!("--out mkdir: {e}")))?;
        Some(p)
    };

    let mut bundles: Vec<serde_json::Value> = Vec::new();
    let mut textures: Vec<serde_json::Value> = Vec::new();
    let mut exported = 0usize;
    let mut errors: Vec<serde_json::Value> = Vec::new();

    for (bi, path) in targets.iter().enumerate() {
        if bi % 8 == 0 {
            ctx.tick(bi as u64, Some(targets.len() as u64), "");
        }
        let file_str = path.display().to_string();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": format!("read: {e}") }));
                continue;
            }
        };
        let (data, nodes) = match unityfs::inflate_bundle_ex(&bytes) {
            Ok(v) => v,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": format!("UnityFS: {e}") }));
                continue;
            }
        };
        let sf = match SerializedFile::parse(&data) {
            Ok(sf) => sf,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": format!("SerializedFile: {e}") }));
                continue;
            }
        };

        // .resS 资源流节点（streamData.path 形如 "archive:/CAB-…/CAB-….resS"）
        let res_s: Option<&[u8]> = nodes
            .iter()
            .find(|n| n.path.ends_with(".resS"))
            .and_then(|n| data.get(n.offset..n.offset + n.size));

        let tex_objects: Vec<_> = sf
            .objects
            .iter()
            .filter(|o| o.class_name == "Texture2D")
            .collect();
        let mut tex_list: Vec<serde_json::Value> = Vec::new();

        for o in &tex_objects {
            let fields = match sf.read_object(o, &data) {
                Ok(f) => f,
                Err(e) => {
                    errors.push(serde_json::json!({ "file": file_str, "path_id": o.path_id, "error": e }));
                    continue;
                }
            };
            let name = fields
                .get("m_Name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !app.name.is_empty() && !crate::util::glob_match(&app.name, &name) {
                continue;
            }
            let w = fields.get("m_Width").and_then(|v| v.as_int()).unwrap_or(0) as u32;
            let h = fields.get("m_Height").and_then(|v| v.as_int()).unwrap_or(0) as u32;
            let fmt = fields.get("m_TextureFormat").and_then(|v| v.as_int()).unwrap_or(0) as u32;
            let cis = fields.get("m_CompleteImageSize").and_then(|v| v.as_int()).unwrap_or(0) as u64;

            let (stream_off, stream_size, stream_path) = match fields.get("m_StreamData").and_then(|v| v.as_obj()) {
                Some(sd) => (
                    sd.get("offset").and_then(|v| v.as_int()).unwrap_or(0) as u64,
                    sd.get("size").and_then(|v| v.as_int()).unwrap_or(0) as u64,
                    sd.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ),
                None => (0, 0, String::new()),
            };
            let inline = fields.get("image data").and_then(|v| v.as_bytes()).unwrap_or(&[]);
            let storage = if stream_size > 0 { "resS" } else { "inline" };

            let mut entry = serde_json::json!({
                "name": name,
                "width": w,
                "height": h,
                "format": fmt,
                "format_name": format_name(fmt),
                "complete_image_size": cis,
                "storage": storage,
                "path_id": o.path_id,
            });
            if storage == "resS" {
                entry["resS"] = serde_json::json!({
                    "path": stream_path,
                    "offset": stream_off,
                    "size": stream_size,
                    "found": res_s.is_some(),
                });
            }

            if let Some(out_dir) = &out_dir {
                let pixels = if w > 0 && h > 0 {
                    decode_texture(fmt, w, h, inline, res_s, stream_off as usize, stream_size as usize)
                } else {
                    Err("zero size".into())
                };
                match pixels {
                    Ok(rgba) => {
                        let file_name = sanitize_name(&name);
                        let out_path = out_dir.join(format!("{file_name}.png"));
                        match write_png(&out_path, w, h, &rgba) {
                            Ok(()) => {
                                exported += 1;
                                entry["exported"] = serde_json::json!(out_path.display().to_string());
                            }
                            Err(e) => {
                                errors.push(serde_json::json!({ "file": file_str, "texture": name, "error": format!("png: {e}") }));
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(serde_json::json!({ "file": file_str, "texture": name, "error": e }));
                    }
                }
            }
            tex_list.push(entry);
        }

        textures.extend(tex_list.iter().cloned());
        bundles.push(serde_json::json!({
            "file": file_str,
            "textures": tex_list.len(),
        }));
    }

    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "name_filter": app.name,
        "out": app.out,
        "bundles_scanned": targets.len(),
        "textures_found": textures.len(),
        "exported": exported,
        "textures": textures,
        "errors": errors,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

fn format_name(fmt: u32) -> &'static str {
    match fmt {
        1 => "Alpha8",
        4 => "RGBA32",
        5 => "BGRA32",
        7 => "RGB565",
        10 => "RGBA4444",
        12 => "DXT1/BC1",
        13 => "DXT5/BC3",
        14 => "RGBA1010102",
        15 => "R16",
        16 => "DXT1-Crunched",
        17 => "DXT5-Crunched",
        _ => "unknown",
    }
}

fn sanitize_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    // 防路径穿越
    s.trim_start_matches(['.', '/']).to_string()
}

/// 按格式解码 → RGBA8
fn decode_texture(
    fmt: u32,
    w: u32,
    h: u32,
    inline: &[u8],
    res_s: Option<&[u8]>,
    stream_off: usize,
    stream_size: usize,
) -> Result<Vec<u8>, String> {
    let src: &[u8] = if stream_size > 0 {
        let rs = res_s.ok_or_else(|| {
            format!(".resS stream referenced but not found in bundle (need {stream_size}B @ {stream_off})")
        })?;
        rs.get(stream_off..stream_off + stream_size)
            .ok_or_else(|| format!("resS range {stream_off}+{stream_size} out of stream {}", rs.len()))?
    } else {
        inline
    };

    match fmt {
        4 => {
            // RGBA32
            let need = w as usize * h as usize * 4;
            if src.len() < need {
                return Err(format!("RGBA32 data short: {}/{}", src.len(), need));
            }
            Ok(src[..need].to_vec())
        }
        12 => {
            // DXT1 / BC1：texture2ddecoder 输出 BGRA u32（color() = LE [b,g,r,a]）
            let mut out = vec![0u32; w as usize * h as usize];
            texture2ddecoder::decode_bc1(src, w as usize, h as usize, &mut out)
                .map_err(|e| format!("bc1: {e}"))?;
            Ok(bgra_to_rgba(&out))
        }
        13 => {
            // DXT5 / BC3
            let mut out = vec![0u32; w as usize * h as usize];
            texture2ddecoder::decode_bc3(src, w as usize, h as usize, &mut out)
                .map_err(|e| format!("bc3: {e}"))?;
            Ok(bgra_to_rgba(&out))
        }
        other => Err(format!(
            "format {other} ({}) not supported for PNG export (list-only)",
            format_name(other)
        )),
    }
}

fn bgra_to_rgba(px: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() * 4);
    for &p in px {
        let r = (p & 0xFF) as u8;
        let g = ((p >> 8) & 0xFF) as u8;
        let b = ((p >> 16) & 0xFF) as u8;
        let a = ((p >> 24) & 0xFF) as u8;
        out.extend_from_slice(&[r, g, b, a]);
    }
    out
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| "rgba buffer size mismatch".to_string())?;
    img.save(path).map_err(|e| format!("save {}: {e}", path.display()))
}
