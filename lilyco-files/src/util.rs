//! 域内共享的遍历 / 匹配 / 体积工具。
//!
//! 三条命令（find / rename / dedup）都建在这层之上 —— 精读一遍就够了。
//! 这一层不依赖 `lilyco`，纯 std，便于单测。

use std::fs;
use std::path::{Path, PathBuf};

/// 遍历时跳过的目录名（跨平台一致的"不该翻"清单）
pub const SKIP_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
    ".cache",
    "$RECYCLE.BIN",
    "System Volume Information",
];

/// 一个被发现的文件
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub size: u64,
}

/// 递归收集文件（跳过 [`SKIP_DIRS`]、不跟随符号链接）
///
/// `max_depth` = None 表示不限深度；Some(0) 只收根目录这一层。
/// 返回的错误只在**根路径**本身不可读时产生（子目录不可读则跳过，
/// 避免一个权限问题让整次扫描失败）。
pub fn walk(root: &Path, max_depth: Option<usize>) -> Result<Vec<Entry>, String> {
    if !root.exists() {
        return Err(format!("路径不存在: {}", root.display()));
    }
    let mut out = Vec::new();
    if root.is_file() {
        let size = fs::metadata(root).map(|m| m.len()).unwrap_or(0);
        out.push(Entry {
            path: root.to_path_buf(),
            size,
        });
        return Ok(out);
    }
    walk_into(root, 0, max_depth, &mut out);
    Ok(out)
}

fn walk_into(dir: &Path, depth: usize, max_depth: Option<usize>, out: &mut Vec<Entry>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let path = e.path();
        // symlink_metadata：不跟随符号链接，避免环
        let Ok(md) = fs::symlink_metadata(&path) else {
            continue;
        };
        if md.file_type().is_symlink() {
            continue;
        }
        if md.is_dir() {
            if depth >= max_depth.unwrap_or(usize::MAX) {
                continue;
            }
            let name = e.file_name();
            let name = name.to_string_lossy();
            if SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                continue;
            }
            walk_into(&path, depth + 1, max_depth, out);
        } else if md.is_file() {
            out.push(Entry {
                path,
                size: md.len(),
            });
        }
    }
}

/// 极简 glob 匹配：支持 `*`（不含分隔符）、`?`（单字符）、`**`（任意层级）
///
/// 刻意不引入 glob crate：域内只需要文件名匹配，`**` 仅在整段模式里出现一次
/// 时才有特殊含义，够用且零依赖。
pub fn glob_match(pattern: &str, text: &str) -> bool {
    glob_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_inner(p: &[u8], t: &[u8]) -> bool {
    // 逐字节回溯匹配（模式短，无需 DP 表）
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            // `**` 与 `*` 在此实现里语义相同（都匹配任意字符序列）；
            // 需要"不跨目录"的严格语义请调用方先按 `/` 分段。
            if pi + 1 < p.len() && p[pi + 1] == b'*' {
                pi += 1;
            }
            star_p = pi;
            star_t = ti;
            pi += 1;
        } else if star_p != usize::MAX {
            pi = star_p + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// 大小写处理（`ignore_case` 为真时统一小写）
pub fn norm(s: &str, ignore_case: bool) -> String {
    if ignore_case {
        s.to_lowercase()
    } else {
        s.to_string()
    }
}

/// 人类可读体积
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// 按扩展名分组（无扩展名归到 `(none)`）
pub fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "(none)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    fn touch(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = File::create(&p).unwrap();
        f.write_all(content).unwrap();
        p
    }

    #[test]
    fn walk_finds_nested_files() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.txt", b"hello");
        touch(tmp.path(), "sub/b.txt", b"world!!");
        let mut e = walk(tmp.path(), None).unwrap();
        e.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(e.len(), 2, "{e:?}");
        assert_eq!(e[0].size, 5);
        assert_eq!(e[1].size, 7);
    }

    #[test]
    fn walk_skips_skip_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "keep.txt", b"x");
        touch(tmp.path(), "node_modules/junk.js", b"xxxx");
        touch(tmp.path(), ".git/config", b"xx");
        let e = walk(tmp.path(), None).unwrap();
        assert_eq!(e.len(), 1, "should skip node_modules/.git: {e:?}");
    }

    #[test]
    fn walk_depth_zero_stops_at_root() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "top.txt", b"a");
        touch(tmp.path(), "sub/deep.txt", b"b");
        let e = walk(tmp.path(), Some(0)).unwrap();
        assert_eq!(e.len(), 1);
        assert!(e[0].path.ends_with("top.txt"));
    }

    #[test]
    fn walk_missing_root_errors() {
        let e = walk(Path::new("definitely/not/here/xyz"), None);
        assert!(e.is_err());
    }

    #[test]
    fn walk_single_file_root() {
        let tmp = tempfile::tempdir().unwrap();
        let p = touch(tmp.path(), "one.txt", b"abc");
        let e = walk(&p, None).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].size, 3);
    }

    #[test]
    fn glob_star_and_question() {
        assert!(glob_match("*.txt", "a.txt"));
        assert!(glob_match("*.txt", "hello world.txt"));
        assert!(!glob_match("*.txt", "a.md"));
        assert!(glob_match("a?c.png", "abc.png"));
        assert!(!glob_match("a?c.png", "abbc.png"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn glob_double_star_matches_any() {
        assert!(glob_match("**/*.jpg", "a/b/c.jpg"));
        assert!(glob_match("img_*.png", "img_001.png"));
        assert!(!glob_match("img_*.png", "photo_001.png"));
    }

    #[test]
    fn glob_is_backtracking_safe() {
        // 经典回溯陷阱：不该指数爆炸
        assert!(!glob_match("*a*a*a*a*a*b", "aaaaaaaaaaaaaaaaaaaaaaaaaaac"));
    }

    #[test]
    fn human_size_formats() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn ext_of_handles_none_and_case() {
        assert_eq!(ext_of(Path::new("a.TXT")), "txt");
        assert_eq!(ext_of(Path::new("README")), "(none)");
    }
}
