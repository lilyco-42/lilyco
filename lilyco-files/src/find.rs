//! `lfiles find` — 递归查找文件（T0 只读）
//!
//! 支持 glob 文件名匹配 + 扩展名过滤 + 体积区间 + 深度限制。

use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

use crate::util::{ext_of, glob_match, human_size, norm, walk};

/// 递归查找文件
#[derive(App)]
#[app(
    name = "find",
    run = "run_find",
    about = "Recursively find files under a root directory. Filters: `pattern` is a glob matched against the FILE NAME only (`*` matches any run of characters, `?` one character, e.g. `*.jpg`, `img_???.png`); `ext` restricts by extension (comma-separated, e.g. 'jpg,png', case-insensitive); `min-size`/`max-size` are bytes; `max-depth` 0 means only the root level, omitted means unlimited. Dot-directories and build/VCS dirs (.git, node_modules, target, __pycache__, dist, build, .venv, ...) are skipped; symbolic links are not followed. Returns { root, count, total_size, total_size_human, by_ext: {ext: count}, files: [{ path, size, size_human, ext }] } sorted by path. Read-only (safety T0)."
)]
pub struct Find {
    /// 起始目录或文件
    #[arg(
        about = "Root directory (or a single file) to search",
        must_exist = true
    )]
    root: PathBuf,

    /// 文件名 glob（如 *.jpg）
    #[arg(about = "Glob matched against the file NAME, e.g. '*.jpg' (omit for all files)")]
    pattern: Option<String>,

    /// 扩展名过滤（逗号分隔）
    #[arg(about = "Comma-separated extension filter, e.g. 'jpg,png' (case-insensitive)")]
    ext: Option<String>,

    /// 最小字节数
    #[arg(about = "Minimum file size in bytes (inclusive)")]
    min_size: Option<u64>,

    /// 最大字节数
    #[arg(about = "Maximum file size in bytes (inclusive)")]
    max_size: Option<u64>,

    /// 最大深度（0 = 仅根目录）
    #[arg(about = "Maximum recursion depth; 0 = root level only (omit for unlimited)")]
    max_depth: Option<u32>,

    /// 忽略大小写
    #[arg(about = "Case-insensitive pattern and extension matching")]
    ignore_case: bool,

    /// 最多返回多少条（0 = 不限）
    #[arg(
        about = "Cap the number of returned files (0 = unlimited)",
        default = 0
    )]
    limit: u64,
}

/// 一条命中的文件
#[derive(serde::Serialize)]
struct FoundFile {
    path: String,
    size: u64,
    size_human: String,
    ext: String,
}

fn run_find(app: &Find, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();

    if let (Some(lo), Some(hi)) = (app.min_size, app.max_size) {
        if lo > hi {
            return Err(AppError::InvalidArg(format!(
                "min-size ({lo}) 不能大于 max-size ({hi})"
            )));
        }
    }

    let ext_filter: Option<Vec<String>> = app.ext.as_ref().map(|s| {
        s.split(',')
            .map(|x| x.trim().trim_start_matches('.').to_lowercase())
            .filter(|x| !x.is_empty())
            .collect()
    });

    // 1. 遍历
    let entries =
        walk(&app.root, app.max_depth.map(|d| d as usize)).map_err(AppError::InvalidArg)?;

    ctx.emit(Progress::Started {
        total: Some(entries.len() as u64),
        message: Some(format!("scanning {} entries", entries.len())),
    });

    // 2. 过滤
    let pat = app.pattern.as_ref().map(|p| norm(p, app.ignore_case));
    let mut hits: Vec<FoundFile> = Vec::new();
    let mut total_size: u64 = 0;
    let mut by_ext: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();

    for (i, e) in entries.iter().enumerate() {
        if i % 256 == 0 {
            ctx.tick(i as u64, Some(entries.len() as u64), "");
        }
        let file_name = e
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if let Some(p) = &pat {
            if !glob_match(p, &norm(&file_name, app.ignore_case)) {
                continue;
            }
        }
        let ext = ext_of(&e.path);
        if let Some(f) = &ext_filter {
            if !f.iter().any(|x| x == &ext) {
                continue;
            }
        }
        if let Some(lo) = app.min_size {
            if e.size < lo {
                continue;
            }
        }
        if let Some(hi) = app.max_size {
            if e.size > hi {
                continue;
            }
        }

        total_size += e.size;
        *by_ext.entry(ext.clone()).or_insert(0) += 1;
        hits.push(FoundFile {
            path: e.path.display().to_string(),
            size: e.size,
            size_human: human_size(e.size),
            ext,
        });
        if app.limit > 0 && hits.len() as u64 >= app.limit {
            break;
        }
    }

    hits.sort_by(|a, b| a.path.cmp(&b.path));

    let duration_ms = start.elapsed().as_millis() as u64;
    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "count": hits.len(),
        "total_size": total_size,
        "total_size_human": human_size(total_size),
        "by_ext": by_ext,
        "files": hits,
        "duration_ms": duration_ms,
    });
    ctx.done(result.clone(), duration_ms);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn touch(dir: &std::path::Path, name: &str, content: &[u8]) {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(content).unwrap();
    }

    fn run(args: &[(&str, serde_json::Value)]) -> Result<serde_json::Value, AppError> {
        let mut app = Find {
            root: PathBuf::from("."),
            pattern: None,
            ext: None,
            min_size: None,
            max_size: None,
            max_depth: None,
            ignore_case: false,
            limit: 0,
        };
        for (k, v) in args {
            match *k {
                "root" => app.root = PathBuf::from(v.as_str().unwrap()),
                "pattern" => app.pattern = v.as_str().map(str::to_string),
                "ext" => app.ext = v.as_str().map(str::to_string),
                "min_size" => app.min_size = v.as_u64(),
                "max_size" => app.max_size = v.as_u64(),
                "max_depth" => app.max_depth = v.as_u64().map(|d| d as u32),
                "ignore_case" => app.ignore_case = v.as_bool().unwrap_or(false),
                "limit" => app.limit = v.as_u64().unwrap_or(0),
                _ => {}
            }
        }
        let (tx, _rx) = std::sync::mpsc::channel();
        run_find(&app, &Context::new_test(tx))
    }

    #[test]
    fn finds_by_glob() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.jpg", b"12345");
        touch(tmp.path(), "b.png", b"12");
        touch(tmp.path(), "sub/c.jpg", b"1234567");
        let r = run(&[
            ("root", serde_json::json!(tmp.path().to_str().unwrap())),
            ("pattern", serde_json::json!("*.jpg")),
        ])
        .unwrap();
        assert_eq!(r["count"], 2);
        assert_eq!(r["total_size"], 12);
        assert_eq!(r["by_ext"]["jpg"], 2);
    }

    #[test]
    fn filters_by_ext_list() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.JPG", b"1");
        touch(tmp.path(), "b.png", b"1");
        touch(tmp.path(), "c.txt", b"1");
        let r = run(&[
            ("root", serde_json::json!(tmp.path().to_str().unwrap())),
            ("ext", serde_json::json!("jpg,png")),
        ])
        .unwrap();
        assert_eq!(r["count"], 2, "JPG should match jpg case-insensitively");
    }

    #[test]
    fn filters_by_size_range() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "small.bin", b"1"); // 1 字节 → 低于 min
        touch(tmp.path(), "mid.bin", b"12345678"); // 8 字节 → 命中
        touch(tmp.path(), "big.bin", b"12345678901234567890"); // 20 字节 → 也在范围内
        let r = run(&[
            ("root", serde_json::json!(tmp.path().to_str().unwrap())),
            ("min_size", serde_json::json!(5)),
            ("max_size", serde_json::json!(100)),
        ])
        .unwrap();
        // min=5 排除 small(1)，mid(8) 与 big(20) 都在 [5,100] 内。
        // 结果按路径排序 → big.bin(20) 在 mid.bin(8) 之前。
        assert_eq!(r["count"], 2, "{r}");
        assert_eq!(r["files"][0]["size"], 20);
        assert_eq!(r["files"][1]["size"], 8);
    }

    #[test]
    fn max_size_upper_bound_excludes_larger() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "ok.bin", b"12345678"); // 8 字节
        touch(tmp.path(), "toobig.bin", b"123456789012345678901"); // 21 字节
        let r = run(&[
            ("root", serde_json::json!(tmp.path().to_str().unwrap())),
            ("max_size", serde_json::json!(10)),
        ])
        .unwrap();
        assert_eq!(r["count"], 1, "{r}");
        assert_eq!(r["files"][0]["size"], 8);
    }

    #[test]
    fn inverted_size_range_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let e = run(&[
            ("root", serde_json::json!(tmp.path().to_str().unwrap())),
            ("min_size", serde_json::json!(100)),
            ("max_size", serde_json::json!(10)),
        ])
        .unwrap_err();
        assert!(e.to_string().contains("不能大于"), "{e}");
    }

    #[test]
    fn limit_caps_results() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..10 {
            touch(tmp.path(), &format!("f{i}.txt"), b"x");
        }
        let r = run(&[
            ("root", serde_json::json!(tmp.path().to_str().unwrap())),
            ("limit", serde_json::json!(3)),
        ])
        .unwrap();
        assert_eq!(r["count"], 3);
    }

    #[test]
    fn max_depth_zero_is_root_only() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "top.txt", b"x");
        touch(tmp.path(), "deep/a.txt", b"x");
        let r = run(&[
            ("root", serde_json::json!(tmp.path().to_str().unwrap())),
            ("max_depth", serde_json::json!(0)),
        ])
        .unwrap();
        assert_eq!(r["count"], 1);
    }

    #[test]
    fn ignore_case_matches_uppercase_name() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "PHOTO.JPG", b"x");
        let r = run(&[
            ("root", serde_json::json!(tmp.path().to_str().unwrap())),
            ("pattern", serde_json::json!("*.jpg")),
            ("ignore_case", serde_json::json!(true)),
        ])
        .unwrap();
        assert_eq!(r["count"], 1);
    }

    #[test]
    fn missing_root_is_invalid_arg() {
        let e = run(&[("root", serde_json::json!("nope/xyz/qq"))]).unwrap_err();
        assert!(matches!(e, AppError::InvalidArg(_)), "{e:?}");
    }

    #[test]
    fn result_sorted_by_path() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "z.txt", b"x");
        touch(tmp.path(), "a.txt", b"x");
        touch(tmp.path(), "m.txt", b"x");
        let r = run(&[("root", serde_json::json!(tmp.path().to_str().unwrap()))]).unwrap();
        let paths: Vec<String> = r["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap().to_string())
            .collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "results must be path-sorted");
    }
}
