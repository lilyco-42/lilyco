//! lgrep — 一个简单的递归 grep，天然四端 + AI 可调。
//!
//! 用作 DSH 生态接入的测试载体：
//! ```bash
//! lgrep --pattern hello --path src                  # CLI
//! lgrep --pattern TODO --path . --ignore-case --count
//! lgrep --pattern hello --path src --json-stream   # AI/脚本消费（JSONL 进度事件）
//! lgrep --mcp                                     # MCP stdio 服务器（dsh-mcp-client 直连）
//! lgrep --gui                                     # Web GUI
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use lilyco::prelude::*;

/// 一个简单的递归 grep（子串匹配，未实现正则）
#[derive(App)]
#[app(run = "run_grep")]
struct Grep {
    /// 要搜索的模式（纯子串匹配）
    #[arg(about = "Pattern to search (substring match)")]
    pattern: String,

    /// 文件或目录（目录递归搜索）
    #[arg(must_exist = true)]
    path: PathBuf,

    /// 忽略大小写
    #[arg(about = "Ignore case")]
    ignore_case: bool,

    /// 只输出统计（文件数 + 匹配数），不输出具体行
    #[arg(about = "Print counts only")]
    count: bool,
}

/// 一条匹配记录
#[derive(serde::Serialize)]
struct MatchLine {
    file: String,
    line: usize,
    text: String,
}

/// 业务逻辑：收集文件 → 逐文件搜索 → 汇总
///
/// 与框架的约定：全程通过 ctx 上报进度，最终 ctx.done 提交结果；
/// 任何渲染端（CLI/TUI/GUI/MCP/DSH）都消费同一事件流。
fn run_grep(app: &Grep, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let needle = if app.ignore_case {
        app.pattern.to_lowercase()
    } else {
        app.pattern.clone()
    };
    if needle.is_empty() {
        return Err(AppError::InvalidArg("pattern must not be empty".into()));
    }

    // 1. 收集文件（目录递归 / 单文件直用）
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&app.path, &mut files)?;

    ctx.emit(Progress::Started {
        total: Some(files.len() as u64),
        message: Some(format!("scanning {} files", files.len())),
    });

    // 2. 逐文件搜索
    let mut matches: Vec<MatchLine> = Vec::new();
    for (i, file) in files.iter().enumerate() {
        ctx.tick(
            i as u64 + 1,
            Some(files.len() as u64),
            file.display().to_string(),
        );
        search_file(file, &needle, app.ignore_case, &mut matches);
    }

    // 3. 汇总
    ctx.log(
        LogLevel::Info,
        format!("{} matches in {} files", matches.len(), files.len()),
    );
    let result = if app.count {
        serde_json::json!({
            "files": files.len(),
            "match_count": matches.len(),
        })
    } else {
        serde_json::json!({
            "files": files.len(),
            "match_count": matches.len(),
            "matches": matches,
        })
    };
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// 递归收集普通文件；目录条目排序，保证输出可复现
fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), AppError> {
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    if path.is_dir() {
        let entries = std::fs::read_dir(path)?;
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                collect_files(&p, out)?;
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
    Ok(())
}

/// 在单个文件中搜索子串；非 UTF-8 / 读取失败的文件静默跳过
fn search_file(file: &Path, needle: &str, ignore_case: bool, out: &mut Vec<MatchLine>) {
    let Ok(content) = std::fs::read_to_string(file) else {
        return;
    };
    for (idx, line) in content.lines().enumerate() {
        let hay = if ignore_case {
            line.to_lowercase()
        } else {
            line.to_string()
        };
        if hay.contains(needle) {
            out.push(MatchLine {
                file: file.display().to_string(),
                line: idx + 1,
                text: line.to_string(),
            });
        }
    }
}

// ── main: one line ────────────────────────────────────────

fn main() {
    lilyco::run::<Grep>();
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_files_recurses_directory_sorted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b.txt"), "hello").unwrap();

        let mut files = Vec::new();
        collect_files(dir.path(), &mut files).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.ends_with("a.rs")));
        assert!(files.iter().any(|f| f.ends_with("b.txt")));
    }

    #[test]
    fn collect_files_accepts_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.txt");
        std::fs::write(&f, "hi").unwrap();

        let mut files = Vec::new();
        collect_files(&f, &mut files).unwrap();
        assert_eq!(files, vec![f]);
    }

    #[test]
    fn collect_files_ignores_missing_path() {
        let mut files = Vec::new();
        collect_files(Path::new("/nonexistent/xyz"), &mut files).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn search_file_finds_substring() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("t.txt");
        std::fs::write(&f, "hello world\nno match\nHello again\n").unwrap();

        let mut m = Vec::new();
        search_file(&f, "hello", false, &mut m);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].line, 1);

        let mut m2 = Vec::new();
        search_file(&f, "hello", true, &mut m2);
        assert_eq!(m2.len(), 2, "ignore_case 应命中 hello + Hello");
        assert_eq!(m2[1].line, 3);
    }

    #[test]
    fn search_file_skips_binary() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("bin.dat");
        std::fs::write(&f, [0u8, 159, 146, 150, 0, 1]).unwrap();

        let mut m = Vec::new();
        search_file(&f, "x", false, &mut m);
        assert!(m.is_empty());
    }

    #[test]
    fn empty_pattern_rejected() {
        let app = Grep {
            pattern: String::new(),
            path: PathBuf::from("."),
            ignore_case: false,
            count: false,
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        let ctx = Context::new_test(tx);
        let err = run_grep(&app, &ctx).unwrap_err();
        assert!(err.to_string().contains("pattern"), "got: {err}");
    }

    #[test]
    fn run_grep_reports_progress_and_result() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "nothing here\nhello again\n").unwrap();

        let app = Grep {
            pattern: "hello".into(),
            path: dir.path().to_path_buf(),
            ignore_case: false,
            count: false,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = Context::new_test(tx);
        let result = run_grep(&app, &ctx).unwrap();
        // 先释放 ctx（持有 tx），否则 rx.iter() 会等 channel 关闭而死锁
        drop(ctx);

        assert_eq!(result["files"], 2);
        assert_eq!(result["match_count"], 2);
        assert_eq!(result["matches"].as_array().unwrap().len(), 2);

        // 进度事件流：Started → Tick×2 → Done
        let events: Vec<Progress> = rx.iter().collect();
        assert!(matches!(events[0], Progress::Started { .. }));
        assert!(
            events
                .iter()
                .filter(|e| matches!(e, Progress::Tick { .. }))
                .count()
                >= 2
        );
        assert!(matches!(events.last(), Some(Progress::Done { .. })));
    }
}
