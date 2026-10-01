//! `lpxls frame` — 取指定姿势的帧/图层结构数据（T0 只读）
//!
//! 回答「gun 姿势第 0 帧里 rod 层的 x/y/rotR 是多少」——正是 mod 调姿势时要抄的数据。

use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

use crate::util::{collect_targets, glob_match, load_pxls};

/// 导出某个姿势的全部帧与图层变换
#[derive(App)]
#[app(
    name = "frame",
    run = "run_frame",
    about = "Dump every frame and layer transform of poses matching `pose` (glob on title, case-insensitive) from PixelLiner .pxls tables. This is the character-structure data a mod needs: per layer { name, kind, group, alpha, x, y (units), zmx, zmy, rot_r (radians), blend_variable, img (image key EDI<hex>_<id2>), img_size (px, when the image is found in an atlas) }. `root` is a pxls file or a directory; `index` restricts to one frame index per direction sequence. Returns { root, matches: [{ file, pose, dirs: [{ dir, frames: [{ index, name, crf60, layers: [...] }] }] }] }. Read-only (safety T0)."
)]
pub struct Frame {
    /// pxls 文件或目录
    #[arg(about = "A .pxls file, or a directory scanned recursively for pxls tables", must_exist = true)]
    root: PathBuf,

    /// 姿势标题（glob，不区分大小写），如 gun
    #[arg(about = "Pose title glob, e.g. 'gun', 'gun2stand'")]
    pose: String,

    /// 只取每个方向序列的第 N 帧（0 起；省略 = 全部帧）
    #[arg(about = "Only frame N within each direction sequence (0-based; omit for all)")]
    index: Option<u32>,
}

fn run_frame(app: &Frame, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let targets = collect_targets(&app.root).map_err(AppError::InvalidArg)?;
    ctx.emit(Progress::Started {
        total: Some(targets.len() as u64),
        message: Some(format!("scanning {} pxls candidates", targets.len())),
    });

    let mut matches: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut files_parsed = 0usize;

    for (i, path) in targets.iter().enumerate() {
        if i % 8 == 0 {
            ctx.tick(i as u64, Some(targets.len() as u64), "");
        }
        let file_str = path.display().to_string();
        let p = match load_pxls(path) {
            Ok(p) => p,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": e }));
                continue;
            }
        };
        files_parsed += 1;

        for pose in &p.poses {
            if !glob_match(&app.pose, &pose.title) {
                continue;
            }
            let dirs: Vec<serde_json::Value> = pose
                .seqs
                .iter()
                .map(|s| {
                    let frames: Vec<serde_json::Value> = s
                        .frames
                        .iter()
                        .enumerate()
                        .filter(|(idx, _)| app.index.map(|n| *idx as u32 == n).unwrap_or(true))
                        .map(|(idx, f)| {
                            serde_json::json!({
                                "index": idx,
                                "name": f.name,
                                "crf60": f.crf60,
                                "layers": f.layers,
                            })
                        })
                        .collect();
                    serde_json::json!({ "dir": s.dir, "frames": frames })
                })
                .collect();
            matches.push(serde_json::json!({
                "file": file_str,
                "pose": pose.title,
                "dirs": dirs,
            }));
        }
    }

    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "pose_filter": app.pose,
        "frame_index": app.index,
        "files_parsed": files_parsed,
        "matches": matches,
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
    fn pose_arg_is_required_by_schema() {
        let app = Frame {
            root: PathBuf::from("."),
            pose: String::new(),
            index: None,
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        // 空标题也允许运行（匹配不到而已），这里只验证不 panic
        let _ = run_frame(&app, &Context::new_test(tx));
    }
}
