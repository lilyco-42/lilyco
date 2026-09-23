//! `@BIN@ show` —— 一条 T0 只读命令的完整样板：参数、进度、自证、四端一致。

use lilyco::prelude::*;
use serde_json::{json, Value};
use std::path::PathBuf;

/// 省略数字参数时四端必须一致：`#[arg(default = N)]` **只对 CLI 生效**，
/// Web/MCP 对省略的数字参数送 0，把它当「返回 0 行」用就会四端结果长度不同。
/// 所以 0 一律解释成「用这个缺省」（`lbin` 的四端逐字探针抓出过这条）。
const DEFAULT_LINES: u64 = 20;

/// 数一个文本文件的行数与字节数，并返回开头若干行
#[derive(App)]
#[app(
    name = "show",
    run = "run_show",
    about = "Count lines and bytes of a UTF-8 text file and return its first lines: gives { path, bytes, lines, returned_lines, truncated, head } where lines is the whole file's line count and head holds at most `lines` entries (0 means the default 20). Read-only (safety T0); nothing is written and the file is never parsed beyond line breaks."
)]
pub struct Show {
    /// 要看的文件
    #[arg(about = "File to show", must_exist = true)]
    path: PathBuf,

    /// 最多返回几行（0 = 用缺省）
    #[arg(about = "Max lines to return", default = 20)]
    lines: u64,
}

fn run_show(app: &Show, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let limit = if app.lines == 0 {
        DEFAULT_LINES
    } else {
        app.lines
    };
    let bytes = std::fs::read(&app.path)
        .map_err(|e| AppError::InvalidInput(format!("读 {} 失败: {e}", app.path.display())))?;
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some("counting".to_string()),
    });
    let text = String::from_utf8_lossy(&bytes);
    let mut rest = text.lines();
    let head: Vec<&str> = rest.by_ref().take(limit as usize).collect();
    let total = head.len() + rest.count();
    ctx.tick(1, Some(1), "");
    let result = json!({
        "path": app.path.to_string_lossy(),
        "bytes": bytes.len(),
        "lines": total,
        "returned_lines": head.len(),
        // 截断要说：只读了 20 / 500 行不是「这个文件只有 20 行」
        "truncated": total > head.len(),
        "head": head,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(path: PathBuf, lines: u64) -> Result<Value, AppError> {
        let app = Show { path, lines };
        let (tx, _rx) = mpsc::channel();
        run_show(&app, &Context::new_test(tx))
    }

    /// 建一个 n 行的小文件，返回路径
    fn sample(dir: &tempfile::TempDir, name: &str, n: usize) -> PathBuf {
        let path = dir.path().join(name);
        let body: String = (1..=n).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, body).expect("写样本");
        path
    }

    #[test]
    fn counts_every_line_and_returns_the_head() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = run(sample(&dir, "a.txt", 5), 3).expect("show 应成功");
        assert_eq!(out["lines"], 5);
        assert_eq!(out["returned_lines"], 3);
        assert_eq!(out["truncated"], true);
        assert_eq!(out["head"][0], "line 1");
        assert_eq!(out["head"][2], "line 3");
    }

    /// 0 是「没填」，不是「返回 0 行」：Web/MCP 省略数字参数时送的就是 0
    #[test]
    fn a_zero_line_count_uses_the_documented_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zero = run(sample(&dir, "a.txt", 30), 0).expect("0 要当缺省");
        let dflt = run(sample(&dir, "a.txt", 30), DEFAULT_LINES).expect("显式缺省");
        assert_eq!(zero["returned_lines"], dflt["returned_lines"]);
        assert_eq!(zero["returned_lines"], DEFAULT_LINES);
    }

    #[test]
    fn a_missing_file_is_an_error_not_an_empty_answer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = run(dir.path().join("nope.txt"), 5).expect_err("不存在的文件要报错");
        assert!(matches!(err, AppError::InvalidInput(_)), "{err:?}");
    }
}
