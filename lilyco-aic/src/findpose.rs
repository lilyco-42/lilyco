//! `laic find-pose` — 跨角色表搜姿势存在性（T0 只读）
//!
//! 回答「哪个角色的表里有 gun 姿势」——例如确认小兵表 honeycomb 有 gun、
//! 玩家表 noel 没有（这正是姿势移植需求的直接证据）。

use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

use crate::util::{collect_targets, glob_match, load_pxls};

/// 跨所有 pxls 表搜索姿势名
#[derive(App)]
#[app(
    name = "find-pose",
    run = "run_find_pose",
    about = "Search ALL PixelLiner .pxls character tables under `root` (file or directory, scanned recursively) for poses whose title matches `name` (glob, case-insensitive) and report which files contain them — e.g. find which enemy table has a `gun` pose that the player table lacks. Returns { root, files_searched, files_parsed, count, matches: [{ file, poses: [{ title, dirs: [{ dir, frames }], frames_total, aliases }] }] } sorted by file path, plus per-file parse `errors`. Read-only (safety T0)."
)]
pub struct FindPose {
    /// 搜索根目录（或单文件）
    #[arg(about = "A .pxls file, or a directory scanned recursively for pxls tables", must_exist = true)]
    root: PathBuf,

    /// 姿势标题（glob，不区分大小写），如 gun
    #[arg(about = "Pose title glob, e.g. 'gun', '*stand*'", default = "")]
    name: String,

    /// 最多处理的文件数（0 = 不限）
    #[arg(about = "Cap the number of files processed (0 = unlimited)", default = 0)]
    limit: u64,
}

fn run_find_pose(app: &FindPose, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let targets = collect_targets(&app.root).map_err(AppError::InvalidArg)?;
    let targets = if app.limit > 0 {
        targets.into_iter().take(app.limit as usize).collect()
    } else {
        targets
    };
    ctx.emit(Progress::Started {
        total: Some(targets.len() as u64),
        message: Some(format!("searching {} pxls tables", targets.len())),
    });

    let mut matches: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut files_parsed = 0usize;
    let mut hit_count = 0usize;

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

        let hits: Vec<serde_json::Value> = p
            .poses
            .iter()
            .filter(|pose| {
                // 姿势名 + 别名都参与匹配（alias_to 机制：一个姿势可以有多个可调用名）
                glob_match(&app.name, &pose.title)
                    || pose.aliases.iter().any(|a| glob_match(&app.name, a))
            })
            .map(|pose| {
                hit_count += 1;
                serde_json::json!({
                    "title": pose.title,
                    "aliases": pose.aliases,
                    "dirs": pose.seqs.iter().map(|s| serde_json::json!({
                        "dir": s.dir,
                        "frames": s.frames.len(),
                        "loop_to": s.loop_to,
                    })).collect::<Vec<_>>(),
                    "frames_total": pose.frames_total(),
                })
            })
            .collect();

        if !hits.is_empty() {
            matches.push(serde_json::json!({ "file": file_str, "poses": hits }));
        }
    }

    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "name": app.name,
        "files_searched": targets.len(),
        "files_parsed": files_parsed,
        "count": matches.len(),
        "pose_hits": hit_count,
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
    fn glob_matches_alias_and_title() {
        assert!(glob_match("gun", "GUN"));
        assert!(glob_match("gun*", "gun2stand"));
        assert!(glob_match("*stand", "gun2stand"));
        assert!(!glob_match("gun", "gun2stand"));
    }
}
