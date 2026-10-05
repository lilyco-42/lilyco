//! `laic poses` — 列出 pxls 角色表的姿势（T0 只读）
//!
//! 回答「这个角色有哪些姿势、每条姿势几个方向几帧」。
//! `--pose` 按标题过滤（glob，不区分大小写）；`--full` 附带每帧图层变换明细。

use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

use crate::util::{collect_targets, glob_match, load_pxls};

/// 列出 pxls 角色表的姿势清单
#[derive(App)]
#[app(
    name = "poses",
    run = "run_poses",
    about = "List poses defined in PixelLiner .pxls character tables (raw files or UnityFS-wrapped .pxls.dat, e.g. Alice in Cradle StreamingAssets). `root` may be a single file or a directory scanned recursively for *.pxls.dat / *.pxls / *.pxls.bytes. Returns { root, files_searched, files_parsed, poses: [{ file, title, width, height, auto_flip, aliases, dirs: [{ dir, frames, loop_to }], frames_total }] } sorted by file path; `pose` filters by pose title with a glob (`*`/`?`, case-insensitive); `full` additionally embeds every frame's layer table (name, type, group, alpha, x, y, zmx, zmy, rot_r, img, img_size) — this is the full character-structure dump. Read-only (safety T0)."
)]
pub struct Poses {
    /// pxls 文件或目录
    #[arg(about = "A .pxls file, or a directory scanned recursively for pxls tables", must_exist = true)]
    root: PathBuf,

    /// 按姿势标题过滤（glob，不区分大小写）
    #[arg(about = "Glob filter on pose TITLE, e.g. 'gun*' (omit for all poses)")]
    pose: Option<String>,

    /// 附带每帧图层变换明细（完整人物结构数据）
    #[arg(about = "Also embed every frame's layer transform table (name/type/x/y/zmx/zmy/rot_r/img)")]
    full: bool,

    /// 最多处理的文件数（0 = 不限）
    #[arg(about = "Cap the number of files processed (0 = unlimited)", default = 0)]
    limit: u64,
}

fn run_poses(app: &Poses, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let targets = collect_targets(&app.root).map_err(AppError::InvalidArg)?;
    let targets = if app.limit > 0 {
        targets.into_iter().take(app.limit as usize).collect()
    } else {
        targets
    };
    ctx.emit(Progress::Started {
        total: Some(targets.len() as u64),
        message: Some(format!("scanning {} pxls candidates", targets.len())),
    });

    let mut files: Vec<serde_json::Value> = Vec::new();
    let mut files_parsed = 0usize;
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut pose_hits = 0usize;

    for (i, path) in targets.iter().enumerate() {
        if i % 8 == 0 {
            ctx.tick(i as u64, Some(targets.len() as u64), "");
        }
        let file_str = path.display().to_string();
        let parsed = load_pxls(path);
        let p = match parsed {
            Ok(p) => p,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": e }));
                continue;
            }
        };
        files_parsed += 1;

        let mut poses_json = Vec::new();
        for pose in &p.poses {
            if let Some(f) = &app.pose {
                if !glob_match(f, &pose.title) {
                    continue;
                }
            }
            pose_hits += 1;
            let mut pj = serde_json::json!({
                "title": pose.title,
                "width": pose.width,
                "height": pose.height,
                "auto_flip": pose.auto_flip,
                "aliases": pose.aliases,
                "dirs": pose.seqs.iter().map(|s| serde_json::json!({
                    "dir": s.dir,
                    "frames": s.frames.len(),
                    "loop_to": s.loop_to,
                })).collect::<Vec<_>>(),
                "frames_total": pose.frames_total(),
            });
            if app.full {
                pj["frames"] = serde_json::to_value(&pose.seqs)
                    .map_err(|e| AppError::InvalidArg(format!("serialize: {e}")))?;
                // 组层树：每帧附 tree（PxlGroupContainer.fineLinks 语义重建）
                if let Some(fr) = pj["frames"].as_array_mut() {
                    for (si, s) in pose.seqs.iter().enumerate() {
                        for (fi, frame) in s.frames.iter().enumerate() {
                            fr[si]["frames"][fi]["tree"] = crate::pxlslib::layer_tree(frame);
                        }
                    }
                }
            }
            poses_json.push(pj);
        }

        if !poses_json.is_empty() || app.pose.is_none() {
            files.push(serde_json::json!({
                "file": file_str,
                "sections": p.sections,
                "image_count": p.image_count,
                "pose_count": p.poses.len(),
                "vectors": p.vectors,
                "warnings": p.warnings,
                "poses": poses_json,
            }));
        }
    }

    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "files_searched": targets.len(),
        "files_parsed": files_parsed,
        "pose_hits": pose_hits,
        "files": files,
        "errors": errors,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_root_is_invalid_arg() {
        let app = Poses {
            root: PathBuf::from("nope/zzz/qq"),
            pose: None,
            full: false,
            limit: 0,
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        let err = run_poses(&app, &Context::new_test(tx)).unwrap_err();
        assert!(matches!(err, AppError::InvalidArg(_)));
    }
}
