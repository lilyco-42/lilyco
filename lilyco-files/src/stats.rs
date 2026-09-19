//! `lfiles stats` — 目录体积统计（T0 只读）
//!
//! 回答"我的磁盘被什么占满了"：按扩展名/目录聚合体积，列出最大的文件。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

use crate::util::{ext_of, human_size, walk};

/// 目录体积统计
#[derive(App)]
#[app(
    name = "stats",
    run = "run_stats",
    about = "Summarize disk usage under a root directory: total size and file count, a breakdown by file extension, a breakdown by immediate subdirectory, and the N largest files (largest: how many to list, default 10). Skips VCS/build directories (.git, node_modules, target, __pycache__, dist, build, .venv, ...) and does not follow symlinks. Read-only (safety T0). Returns { root, file_count, total_size, total_size_human, by_ext: [{ ext, count, size, size_human, percent }], by_dir: [{ dir, count, size, size_human }], largest: [{ path, size, size_human }] } — all breakdowns sorted by size descending."
)]
pub struct Stats {
    /// 根目录
    #[arg(about = "Root directory to analyze", must_exist = true)]
    root: PathBuf,

    /// 最大的 N 个文件
    #[arg(about = "How many of the largest files to list", default = 10)]
    largest: u64,
}

/// 按扩展名的聚合
#[derive(serde::Serialize)]
struct ExtStat {
    ext: String,
    count: u64,
    size: u64,
    size_human: String,
    percent: f64,
}

/// 按子目录的聚合
#[derive(serde::Serialize)]
struct DirStat {
    dir: String,
    count: u64,
    size: u64,
    size_human: String,
}

/// 一个大文件
#[derive(serde::Serialize)]
struct BigFile {
    path: String,
    size: u64,
    size_human: String,
}

fn run_stats(app: &Stats, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();

    let entries = walk(&app.root, None).map_err(AppError::InvalidArg)?;
    ctx.emit(Progress::Started {
        total: Some(entries.len() as u64),
        message: Some(format!("analyzing {} files", entries.len())),
    });

    let mut total_size: u64 = 0;
    let mut ext_map: BTreeMap<String, (u64, u64)> = BTreeMap::new(); // ext -> (count, size)
    let mut dir_map: BTreeMap<String, (u64, u64)> = BTreeMap::new(); // dir -> (count, size)

    for (i, e) in entries.iter().enumerate() {
        if i % 512 == 0 {
            ctx.tick(i as u64, Some(entries.len() as u64), "");
        }
        total_size += e.size;

        let ext = ext_of(&e.path);
        let slot = ext_map.entry(ext).or_insert((0, 0));
        slot.0 += 1;
        slot.1 += e.size;

        // 归属到根下的第一层子目录；根下的文件归到 `(root)`
        let rel = e.path.strip_prefix(&app.root).unwrap_or(&e.path);
        let top = match rel.components().next() {
            Some(c) if rel.components().count() > 1 => c.as_os_str().to_string_lossy().to_string(),
            _ => "(root)".to_string(),
        };
        let slot = dir_map.entry(top).or_insert((0, 0));
        slot.0 += 1;
        slot.1 += e.size;
    }

    // 扩展名：按体积降序
    let mut by_ext: Vec<ExtStat> = ext_map
        .into_iter()
        .map(|(ext, (count, size))| ExtStat {
            ext,
            count,
            size,
            size_human: human_size(size),
            percent: if total_size == 0 {
                0.0
            } else {
                (size as f64 / total_size as f64 * 1000.0).round() / 10.0
            },
        })
        .collect();
    by_ext.sort_by(|a, b| b.size.cmp(&a.size).then(a.ext.cmp(&b.ext)));

    // 子目录：按体积降序
    let mut by_dir: Vec<DirStat> = dir_map
        .into_iter()
        .map(|(dir, (count, size))| DirStat {
            dir,
            count,
            size,
            size_human: human_size(size),
        })
        .collect();
    by_dir.sort_by(|a, b| b.size.cmp(&a.size).then(a.dir.cmp(&b.dir)));

    // 最大文件
    let mut sorted: Vec<&crate::util::Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| b.size.cmp(&a.size).then(a.path.cmp(&b.path)));
    let largest: Vec<BigFile> = sorted
        .into_iter()
        .take(app.largest as usize)
        .map(|e| BigFile {
            path: e.path.display().to_string(),
            size: e.size,
            size_human: human_size(e.size),
        })
        .collect();

    let duration_ms = start.elapsed().as_millis() as u64;
    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "file_count": entries.len(),
        "total_size": total_size,
        "total_size_human": human_size(total_size),
        "by_ext": by_ext,
        "by_dir": by_dir,
        "largest": largest,
        "duration_ms": duration_ms,
    });
    ctx.done(result.clone(), duration_ms);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn run(args: &[(&str, serde_json::Value)]) -> Result<serde_json::Value, AppError> {
        let mut app = Stats {
            root: PathBuf::from("."),
            largest: 10,
        };
        for (k, v) in args {
            match *k {
                "root" => app.root = PathBuf::from(v.as_str().unwrap()),
                "largest" => app.largest = v.as_u64().unwrap_or(10),
                _ => {}
            }
        }
        let (tx, _rx) = std::sync::mpsc::channel();
        run_stats(&app, &Context::new_test(tx))
    }

    #[test]
    fn aggregates_size_and_count() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), vec![0u8; 100]).unwrap();
        fs::write(tmp.path().join("b.txt"), vec![0u8; 200]).unwrap();
        fs::write(tmp.path().join("c.png"), vec![0u8; 50]).unwrap();
        let r = run(&[("root", serde_json::json!(tmp.path().to_str().unwrap()))]).unwrap();
        assert_eq!(r["file_count"], 3);
        assert_eq!(r["total_size"], 350);
        // txt 250 字节占多数 → 排第一
        assert_eq!(r["by_ext"][0]["ext"], "txt");
        assert_eq!(r["by_ext"][0]["size"], 300);
    }

    #[test]
    fn percent_is_share_of_total() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.bin"), vec![0u8; 75]).unwrap();
        fs::write(tmp.path().join("b.txt"), vec![0u8; 25]).unwrap();
        let r = run(&[("root", serde_json::json!(tmp.path().to_str().unwrap()))]).unwrap();
        assert_eq!(r["by_ext"][0]["percent"], 75.0, "{r}");
    }

    #[test]
    fn by_dir_groups_top_level() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("photos")).unwrap();
        fs::create_dir_all(tmp.path().join("docs")).unwrap();
        fs::write(tmp.path().join("photos/a.jpg"), vec![0u8; 500]).unwrap();
        fs::write(tmp.path().join("docs/b.txt"), vec![0u8; 100]).unwrap();
        fs::write(tmp.path().join("top.txt"), vec![0u8; 10]).unwrap();
        let r = run(&[("root", serde_json::json!(tmp.path().to_str().unwrap()))]).unwrap();
        assert_eq!(r["by_dir"][0]["dir"], "photos");
        assert_eq!(r["by_dir"][0]["size"], 500);
        // 根下散文件归到 (root)
        let dirs: Vec<String> = r["by_dir"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["dir"].as_str().unwrap().to_string())
            .collect();
        assert!(dirs.contains(&"(root)".to_string()), "{dirs:?}");
    }

    #[test]
    fn largest_respects_limit_and_sorts() {
        let tmp = tempfile::tempdir().unwrap();
        for (n, sz) in [("s", 10), ("m", 1000), ("l", 5000)] {
            fs::write(tmp.path().join(format!("{n}.bin")), vec![0u8; sz]).unwrap();
        }
        let r = run(&[
            ("root", serde_json::json!(tmp.path().to_str().unwrap())),
            ("largest", serde_json::json!(2)),
        ])
        .unwrap();
        let l = r["largest"].as_array().unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0]["size"], 5000);
        assert_eq!(l[1]["size"], 1000);
    }

    #[test]
    fn empty_dir_is_zero_not_error() {
        let tmp = tempfile::tempdir().unwrap();
        let r = run(&[("root", serde_json::json!(tmp.path().to_str().unwrap()))]).unwrap();
        assert_eq!(r["file_count"], 0);
        assert_eq!(r["total_size"], 0);
        assert_eq!(r["total_size_human"], "0 B");
    }

    #[test]
    fn skips_build_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), vec![0u8; 10]).unwrap();
        fs::create_dir_all(tmp.path().join("target")).unwrap();
        fs::write(tmp.path().join("target/big.bin"), vec![0u8; 99999]).unwrap();
        let r = run(&[("root", serde_json::json!(tmp.path().to_str().unwrap()))]).unwrap();
        assert_eq!(r["total_size"], 10, "target/ must be skipped: {r}");
    }

    #[test]
    fn missing_root_errors() {
        let e = run(&[("root", serde_json::json!("nope/nope/qq"))]).unwrap_err();
        assert!(matches!(e, AppError::InvalidArg(_)));
    }
}
