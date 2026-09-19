//! `lfiles dedup` — 查找重复文件（**T0 只读**，只报告不删）
//!
//! 三段式：体积分桶 → 头尾 4KB 快速指纹 → 全量哈希。绝大多数文件在第二段就被排除。
//! **刻意不提供删除**：删文件属于 T2/T3，应由人显式确认；本命令只产出清单。

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

use crate::util::{human_size, walk};

/// 快速指纹取样字节数（文件小于 2×此值时直接全量）
const SAMPLE: usize = 4096;

/// 查找重复文件
#[derive(App)]
#[app(
    name = "dedup",
    run = "run_dedup",
    about = "Find duplicate files under a root directory and report the wasted space. Three-stage detection: group by exact size, then compare a cheap head+tail fingerprint, then verify with a full content hash — so only files that are already size-identical and prefix-identical get fully hashed. `min-size` skips tiny files (default 1 byte, raise it to ignore trivial files); `ext` restricts to given extensions. REPORT ONLY — this command never deletes or modifies anything (safety T0). Returns { root, groups: [{ size, size_human, wasted, hash, files: [path...] }], group_count, duplicate_files, wasted_bytes, wasted_human, scanned, duration_ms }. To actually reclaim space, review the report and delete manually."
)]
pub struct Dedup {
    /// 根目录
    #[arg(about = "Root directory to scan (recursive)", must_exist = true)]
    root: PathBuf,

    /// 最小体积（低于此值不参与查重）
    #[arg(about = "Ignore files smaller than this many bytes", default = 1)]
    min_size: u64,

    /// 只查这些扩展名（逗号分隔）
    #[arg(about = "Comma-separated extension filter, e.g. 'jpg,png' (omit = all files)")]
    ext: Option<String>,
}

/// 一组重复文件
#[derive(serde::Serialize)]
struct DupGroup {
    size: u64,
    size_human: String,
    /// 这一组里除了保留 1 份之外浪费掉的字节
    wasted: u64,
    hash: String,
    files: Vec<String>,
}

fn run_dedup(app: &Dedup, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();

    let ext_filter: Option<Vec<String>> = app.ext.as_ref().map(|s| {
        s.split(',')
            .map(|x| x.trim().trim_start_matches('.').to_lowercase())
            .filter(|x| !x.is_empty())
            .collect()
    });

    let all = walk(&app.root, None).map_err(AppError::InvalidArg)?;
    ctx.emit(Progress::Started {
        total: Some(all.len() as u64),
        message: Some(format!("hashing {} files", all.len())),
    });

    // ── 阶段 1：按体积分桶（唯一体积直接排除） ────────────
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for e in &all {
        if e.size < app.min_size {
            continue;
        }
        if let Some(f) = &ext_filter {
            if !f.iter().any(|x| x == &crate::util::ext_of(&e.path)) {
                continue;
            }
        }
        by_size.entry(e.size).or_default().push(e.path.clone());
    }
    let candidates: usize = by_size.values().map(|v| v.len()).sum();
    let size_groups: Vec<(u64, Vec<PathBuf>)> =
        by_size.into_iter().filter(|(_, v)| v.len() > 1).collect();

    // ── 阶段 2 + 3：指纹 → 全量哈希 ───────────────────────
    let mut groups: Vec<DupGroup> = Vec::new();
    let mut done: u64 = 0;
    let scanned = candidates as u64;

    for (size, paths) in size_groups {
        // 阶段 2：头尾取样指纹
        let mut by_fp: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for p in paths {
            done += 1;
            if done % 64 == 0 {
                ctx.tick(done, Some(scanned.max(1)), "");
            }
            match fingerprint(&p, size) {
                Ok(fp) => by_fp.entry(fp).or_default().push(p),
                Err(_) => continue, // 读不了就跳过，不中断整次扫描
            }
        }

        // 阶段 3：同指纹组才做全量哈希
        for (_, group) in by_fp.into_iter().filter(|(_, v)| v.len() > 1) {
            let mut by_hash: HashMap<String, Vec<String>> = HashMap::new();
            for p in group {
                let h = match full_hash(&p) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                by_hash.entry(h).or_default().push(p.display().to_string());
            }
            for (hash, mut files) in by_hash.into_iter().filter(|(_, v)| v.len() > 1) {
                files.sort();
                let wasted = size * (files.len() as u64 - 1);
                groups.push(DupGroup {
                    size,
                    size_human: human_size(size),
                    wasted,
                    hash,
                    files,
                });
            }
        }
    }

    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted).then(a.files[0].cmp(&b.files[0])));

    let duplicate_files: usize = groups.iter().map(|g| g.files.len() - 1).sum();
    let wasted_bytes: u64 = groups.iter().map(|g| g.wasted).sum();

    let duration_ms = start.elapsed().as_millis() as u64;
    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "groups": groups,
        "group_count": groups.len(),
        "duplicate_files": duplicate_files,
        "wasted_bytes": wasted_bytes,
        "wasted_human": human_size(wasted_bytes),
        "scanned": scanned,
        "duration_ms": duration_ms,
        "note": "仅报告，未删除任何文件（T0 只读）",
    });
    ctx.done(result.clone(), duration_ms);
    Ok(result)
}

/// 头尾取样指纹（大小已相同，故只取内容特征）
fn fingerprint(path: &std::path::Path, size: u64) -> std::io::Result<String> {
    use std::fs::File;
    let mut f = File::open(path)?;
    let mut buf = vec![0u8; SAMPLE.min(size as usize)];
    let n = f.read(&mut buf)?;
    let mut acc: u64 = 0xcbf29ce484222325;
    for b in &buf[..n] {
        acc ^= *b as u64;
        acc = acc.wrapping_mul(0x100000001b3);
    }
    // 尾部样本（小文件会与前段重叠，无妨）
    if size > SAMPLE as u64 {
        use std::io::Seek;
        let seek_to = size - SAMPLE as u64;
        f.seek(std::io::SeekFrom::Start(seek_to))?;
        let mut tail = vec![0u8; SAMPLE];
        let n = f.read(&mut tail)?;
        for b in &tail[..n] {
            acc ^= *b as u64;
            acc = acc.wrapping_mul(0x100000001b3);
        }
    }
    Ok(format!("{acc:016x}"))
}

/// 全量内容哈希（FNV-1a 64 位；用于查重足够，非密码学用途）
fn full_hash(path: &std::path::Path) -> std::io::Result<String> {
    use std::fs::File;
    use std::io::BufReader;
    let f = File::open(path)?;
    let mut r = BufReader::with_capacity(64 * 1024, f);
    let mut acc: u64 = 0xcbf29ce484222325;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for b in &buf[..n] {
            acc ^= *b as u64;
            acc = acc.wrapping_mul(0x100000001b3);
        }
    }
    Ok(format!("{acc:016x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn base(root: &std::path::Path) -> Dedup {
        Dedup {
            root: root.to_path_buf(),
            min_size: 1,
            ext: None,
        }
    }

    fn run(app: &Dedup) -> Result<serde_json::Value, AppError> {
        let (tx, _rx) = std::sync::mpsc::channel();
        run_dedup(app, &Context::new_test(tx))
    }

    #[test]
    fn finds_identical_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), b"same content here").unwrap();
        fs::write(tmp.path().join("b.txt"), b"same content here").unwrap();
        fs::write(tmp.path().join("c.txt"), b"different content!").unwrap();
        let r = run(&base(tmp.path())).unwrap();
        assert_eq!(r["group_count"], 1, "{r}");
        assert_eq!(r["duplicate_files"], 1);
        assert_eq!(r["wasted_bytes"], 17);
        let files = r["groups"][0]["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn same_size_but_different_content_not_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.bin"), b"aaaaaaaaaa").unwrap();
        fs::write(tmp.path().join("b.bin"), b"bbbbbbbbbb").unwrap();
        let r = run(&base(tmp.path())).unwrap();
        assert_eq!(
            r["group_count"], 0,
            "same size + different head must not match: {r}"
        );
    }

    #[test]
    fn min_size_filters_small_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), b"xy").unwrap();
        fs::write(tmp.path().join("b.txt"), b"xy").unwrap();
        let app = Dedup {
            min_size: 100,
            ..base(tmp.path())
        };
        let r = run(&app).unwrap();
        assert_eq!(r["group_count"], 0);
    }

    #[test]
    fn ext_filter_restricts() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.jpg"), b"identical-bytes").unwrap();
        fs::write(tmp.path().join("b.jpg"), b"identical-bytes").unwrap();
        fs::write(tmp.path().join("c.txt"), b"identical-bytes").unwrap();
        let app = Dedup {
            ext: Some("jpg".into()),
            ..base(tmp.path())
        };
        let r = run(&app).unwrap();
        assert_eq!(r["group_count"], 1);
        assert_eq!(r["groups"][0]["files"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn three_copies_report_two_redundant() {
        let tmp = tempfile::tempdir().unwrap();
        for n in ["a", "b", "c"] {
            fs::write(tmp.path().join(format!("{n}.bin")), vec![7u8; 1000]).unwrap();
        }
        let r = run(&base(tmp.path())).unwrap();
        assert_eq!(r["duplicate_files"], 2);
        assert_eq!(r["wasted_bytes"], 2000);
    }

    #[test]
    fn large_file_tail_difference_detected() {
        // 超过 SAMPLE 的文件：仅尾部不同也必须区分开
        let tmp = tempfile::tempdir().unwrap();
        let mut a = vec![0u8; SAMPLE * 3];
        let mut b = a.clone();
        a.push(1);
        b.push(2);
        a.extend_from_slice(&[0u8; 10]);
        b.extend_from_slice(&[0u8; 10]);
        fs::write(tmp.path().join("a.bin"), &a).unwrap();
        fs::write(tmp.path().join("b.bin"), &b).unwrap();
        let r = run(&base(tmp.path())).unwrap();
        assert_eq!(r["group_count"], 0, "tail differs -> not duplicates: {r}");
    }

    #[test]
    fn large_identical_files_grouped() {
        let tmp = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..(SAMPLE * 5)).map(|i| (i % 251) as u8).collect();
        fs::write(tmp.path().join("a.bin"), &data).unwrap();
        fs::write(tmp.path().join("b.bin"), &data).unwrap();
        let r = run(&base(tmp.path())).unwrap();
        assert_eq!(r["group_count"], 1, "{r}");
        assert_eq!(r["wasted_bytes"], (SAMPLE * 5) as u64);
    }

    #[test]
    fn groups_sorted_by_wasted_desc() {
        let tmp = tempfile::tempdir().unwrap();
        // 小组：10 字节 ×2
        fs::write(tmp.path().join("s1"), vec![1u8; 10]).unwrap();
        fs::write(tmp.path().join("s2"), vec![1u8; 10]).unwrap();
        // 大组：5000 字节 ×2
        fs::write(tmp.path().join("b1"), vec![2u8; 5000]).unwrap();
        fs::write(tmp.path().join("b2"), vec![2u8; 5000]).unwrap();
        let r = run(&base(tmp.path())).unwrap();
        assert_eq!(r["group_count"], 2);
        assert_eq!(r["groups"][0]["wasted"], 5000, "biggest waste first: {r}");
    }

    #[test]
    fn skips_build_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), b"dupdupdup").unwrap();
        fs::create_dir_all(tmp.path().join("node_modules")).unwrap();
        fs::write(tmp.path().join("node_modules/b.txt"), b"dupdupdup").unwrap();
        let r = run(&base(tmp.path())).unwrap();
        assert_eq!(r["group_count"], 0, "node_modules must be skipped");
    }

    #[test]
    fn missing_root_errors() {
        let app = base(std::path::Path::new("no/such/dir/qq"));
        assert!(run(&app).is_err());
    }
}
