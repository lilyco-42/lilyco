//! `lfiles rename` — 批量重命名（**T1 需确认**）
//!
//! 默认 dry-run（只预览不改），确认后加 `--apply` 才真正落盘。
//! 这是本域唯一的写操作，安全分级刻意标高，让 MCP 自动化面默认拒绝。

use std::path::{Path, PathBuf};
use std::time::Instant;

use lilyco::prelude::*;

use crate::util::{glob_match, norm, walk};

/// 批量重命名文件
#[derive(App)]
#[app(
    name = "rename",
    run = "run_rename",
    safety = "t1",
    about = "Batch-rename files. Two modes: `prefix` + optional `suffix` (e.g. prefix='IMG_' renames a.jpg -> IMG_a.jpg; suffix='_bak' -> a.jpg -> a_bak.jpg), or `find-replace` substring substitution (`replace-from`/`replace-to`, replaces ALL occurrences, respects `ignore-case`). `pattern` restricts which FILES are touched (glob on file name, e.g. '*.jpg'); directories are never renamed. Collisions abort the whole run BEFORE any rename happens (nothing is half-renamed): two files mapping to the same target, or a target that already exists, both abort unless `overwrite` is true. Idempotent by default: files that ALREADY carry the given prefix/suffix are skipped (reported in `skipped`), so running the same command twice does not produce 'P_P_name' — pass `allow-reapply` to disable that guard. DRY RUN BY DEFAULT — passing `apply: true` actually renames (safety tier T1: the automated/MCP surface denies it, so a human must confirm). Returns { root, dry_run, matched, count, renames: [{ from, to }], skipped: [{ from, reason }] }."
)]
pub struct Rename {
    /// 根目录
    #[arg(about = "Root directory to scan (recursive)", must_exist = true)]
    root: PathBuf,

    /// 只改匹配的文件名 glob
    #[arg(about = "Only rename files whose NAME matches this glob, e.g. '*.jpg' (omit = all files)")]
    pattern: Option<String>,

    /// 新文件名前缀
    #[arg(about = "Prefix to prepend to the file stem, e.g. 'IMG_'")]
    prefix: Option<String>,

    /// 新文件名后缀（加在扩展名之前）
    #[arg(about = "Suffix appended to the file stem, e.g. '_bak'")]
    suffix: Option<String>,

    /// 要替换的子串
    #[arg(about = "Substring to replace (requires replace-to); replaces ALL occurrences")]
    replace_from: Option<String>,

    /// 替换成的子串
    #[arg(about = "Replacement for replace-from ('') deletes the substring")]
    replace_to: Option<String>,

    /// 替换时忽略大小写
    #[arg(about = "Case-insensitive substring replacement")]
    ignore_case: bool,

    /// 真正执行（默认只预览）
    #[arg(about = "Actually rename. Without this flag the command is a dry run")]
    apply: bool,

    /// 允许覆盖已存在的目标
    #[arg(about = "Allow overwriting an existing target file (default: abort on collision)")]
    overwrite: bool,

    /// 允许重复叠加前缀/后缀
    #[arg(about = "Allow re-applying a prefix/suffix that is already present (default: skip such files, so running twice is idempotent)")]
    allow_reapply: bool,
}

/// 一条重命名计划
#[derive(serde::Serialize)]
struct RenamePlan {
    from: String,
    to: String,
}

/// 被跳过的文件
#[derive(serde::Serialize)]
struct Skipped {
    from: String,
    reason: String,
}

fn run_rename(app: &Rename, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();

    // 1. 模式校验：必须给出 prefix / suffix / replace 之一
    let has_affix = app.prefix.is_some() || app.suffix.is_some();
    let replace_mode = app.replace_from.is_some();
    if app.replace_to.is_some() && !replace_mode {
        return Err(AppError::InvalidArg(
            "replace-to 必须与 replace-from 一起使用".into(),
        ));
    }
    if !has_affix && !replace_mode {
        return Err(AppError::InvalidArg(
            "至少给出一种改名方式：prefix / suffix / replace-from+replace-to".into(),
        ));
    }
    if replace_mode && app.replace_from.as_deref() == Some("") {
        return Err(AppError::InvalidArg("replace-from 不能为空串".into()));
    }

    // 2. 收集候选
    let entries = walk(&app.root, None).map_err(AppError::InvalidArg)?;
    ctx.emit(Progress::Started {
        total: Some(entries.len() as u64),
        message: Some(format!("planning {} files", entries.len())),
    });

    let pat = app.pattern.as_ref().map(|p| norm(p, app.ignore_case));
    let mut plans: Vec<RenamePlan> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();

    for (i, e) in entries.iter().enumerate() {
        if i % 256 == 0 {
            ctx.tick(i as u64, Some(entries.len() as u64), "");
        }
        let Some(fname) = e.path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        if let Some(p) = &pat {
            if !glob_match(p, &norm(&fname, app.ignore_case)) {
                continue;
            }
        }

        let Some(new_name) = build_new_name(&fname, app) else {
            continue; // 改名结果与原名相同 → 不算命中
        };
        if new_name == fname {
            continue;
        }
        // 幂等保护：前缀/后缀已存在时默认跳过，避免 `P_` 跑两次变成 `P_P_`
        if !app.allow_reapply && already_applied(&fname, app) {
            skipped.push(Skipped {
                from: e.path.display().to_string(),
                reason: "已带该前缀/后缀，跳过（如需重复叠加请加 --allow-reapply）".into(),
            });
            continue;
        }
        // 新名字不能含路径分隔符（防止越出当前目录）
        if new_name.contains('/') || new_name.contains('\\') {
            skipped.push(Skipped {
                from: e.path.display().to_string(),
                reason: "改名结果包含路径分隔符".into(),
            });
            continue;
        }
        let to = e.path.with_file_name(&new_name);
        plans.push(RenamePlan {
            from: e.path.display().to_string(),
            to: to.display().to_string(),
        });
    }

    // 3. 冲突检测（在**任何**改名之前完成，保证不会半改）
    //
    // 两条规则，都比"看目标是否在源集合里"更保守 —— 后者看似能放行交换类改名，
    // 实则掩盖真实覆盖：`a.txt -> P_a.txt` 与 `P_a.txt -> P_P_a.txt` 互为链时，
    // 先执行第一条就覆盖掉了还没被移走的 P_a.txt。既然顺序无法保证，
    // 就一律中止，把决定权交回给人（`--overwrite` 显式自担风险）。
    let mut conflicts: Vec<String> = Vec::new();
    {
        // 规则 1：多个文件改到同一目标
        let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for p in &plans {
            let key = norm(&p.to, app.ignore_case);
            if let Some(prev) = seen.insert(key, p.from.clone()) {
                conflicts.push(format!("多个文件改到同一目标: {} 与 {}", prev, p.to));
            }
        }
        // 规则 2：目标路径已存在（且不在本次 apply 的允许覆盖之下）
        if !app.overwrite {
            for p in &plans {
                if Path::new(&p.to).exists() {
                    conflicts.push(format!("目标已存在: {}（如确认要覆盖请加 --overwrite）", p.to));
                }
            }
        }
    }
    if !conflicts.is_empty() {
        return Err(AppError::InvalidArg(format!(
            "检测到 {} 处冲突，已中止（未做任何改动）：{}",
            conflicts.len(),
            conflicts.join("; ")
        )));
    }

    // 4. dry-run 或落盘
    let mut applied: Vec<RenamePlan> = Vec::new();
    if app.apply {
        for (i, p) in plans.iter().enumerate() {
            ctx.tick(
                i as u64 + 1,
                Some(plans.len() as u64),
                format!("{} -> {}", p.from, p.to),
            );
            std::fs::rename(&p.from, &p.to)
                .map_err(|e| AppError::Runtime(format!("重命名失败 {} -> {}: {e}", p.from, p.to)))?;
            applied.push(RenamePlan {
                from: p.from.clone(),
                to: p.to.clone(),
            });
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "dry_run": !app.apply,
        "matched": plans.len(),
        "count": if app.apply { applied.len() } else { plans.len() },
        "renames": plans,
        "skipped": skipped,
        "duration_ms": duration_ms,
        "hint": if app.apply { "已实际重命名" } else { "这是预览（dry run）；确认无误后加 --apply 才真正改名" },
    });
    ctx.done(result.clone(), duration_ms);
    Ok(result)
}

/// 该文件是否**已经**带上配置的前缀/后缀（用于幂等保护）
///
/// 只看 affix 模式：`replace` 模式天然幂等（替换完就找不到了），不需要这层。
fn already_applied(fname: &str, app: &Rename) -> bool {
    if app.replace_from.is_some() {
        return false;
    }
    let path = Path::new(fname);
    let stem = match path.extension() {
        Some(_) => match path.file_stem() {
            Some(s) => s.to_string_lossy().to_string(),
            None => return false,
        },
        None => fname.to_string(),
    };
    if let Some(p) = &app.prefix {
        if !p.is_empty() && stem.starts_with(p.as_str()) {
            return true;
        }
    }
    if let Some(sfx) = &app.suffix {
        if !sfx.is_empty() && stem.ends_with(sfx.as_str()) {
            return true;
        }
    }
    false
}

/// 按 prefix / suffix / replace 计算新文件名；无变化返回 None
fn build_new_name(fname: &str, app: &Rename) -> Option<String> {
    let path = Path::new(fname);
    if let Some(from) = &app.replace_from {
        let to = app.replace_to.as_deref().unwrap_or("");
        if app.ignore_case {
            // 大小写不敏感替换：逐段扫描（避免 to_lowercase 改变长度导致索引错位）
            let lower_hay = fname.to_lowercase();
            let lower_from = from.to_lowercase();
            if lower_from.is_empty() {
                return None;
            }
            let mut out = String::new();
            let mut cursor = 0usize;
            while let Some(pos) = lower_hay[cursor..].find(&lower_from) {
                let s = cursor + pos;
                out.push_str(&fname[cursor..s]);
                out.push_str(to);
                cursor = s + lower_from.len();
                if cursor > fname.len() {
                    break;
                }
            }
            out.push_str(&fname[cursor..]);
            return Some(out);
        }
        return Some(fname.replace(from, to));
    }

    // prefix / suffix：插在扩展名之前，保留扩展名
    let (stem, ext) = match path.extension() {
        Some(e) => (
            path.file_stem().map(|s| s.to_string_lossy().to_string())?,
            format!(".{}", e.to_string_lossy()),
        ),
        None => (fname.to_string(), String::new()),
    };
    let new_stem = format!(
        "{}{}{}",
        app.prefix.as_deref().unwrap_or(""),
        stem,
        app.suffix.as_deref().unwrap_or("")
    );
    Some(format!("{new_stem}{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"x").unwrap();
    }

    fn base(root: &Path) -> Rename {
        Rename {
            root: root.to_path_buf(),
            pattern: None,
            prefix: None,
            suffix: None,
            replace_from: None,
            replace_to: None,
            ignore_case: false,
            apply: false,
            overwrite: false,
            allow_reapply: false,
        }
    }

    fn run(app: &Rename) -> Result<serde_json::Value, AppError> {
        let (tx, _rx) = std::sync::mpsc::channel();
        run_rename(app, &Context::new_test(tx))
    }

    #[test]
    fn prefix_keeps_extension() {
        assert_eq!(
            build_new_name(
                "a.jpg",
                &Rename {
                    prefix: Some("IMG_".into()),
                    ..base(Path::new("."))
                }
            ),
            Some("IMG_a.jpg".into())
        );
    }

    #[test]
    fn suffix_goes_before_extension() {
        assert_eq!(
            build_new_name(
                "a.jpg",
                &Rename {
                    suffix: Some("_bak".into()),
                    ..base(Path::new("."))
                }
            ),
            Some("a_bak.jpg".into())
        );
    }

    #[test]
    fn prefix_and_suffix_combine() {
        assert_eq!(
            build_new_name(
                "a.txt",
                &Rename {
                    prefix: Some("P_".into()),
                    suffix: Some("_S".into()),
                    ..base(Path::new("."))
                }
            ),
            Some("P_a_S.txt".into())
        );
    }

    #[test]
    fn no_extension_file_gets_affix() {
        assert_eq!(
            build_new_name(
                "README",
                &Rename {
                    prefix: Some("X_".into()),
                    ..base(Path::new("."))
                }
            ),
            Some("X_README".into())
        );
    }

    #[test]
    fn replace_all_occurrences() {
        assert_eq!(
            build_new_name(
                "a-b-c.txt",
                &Rename {
                    replace_from: Some("-".into()),
                    replace_to: Some("_".into()),
                    ..base(Path::new("."))
                }
            ),
            Some("a_b_c.txt".into())
        );
    }

    #[test]
    fn replace_ignore_case_preserves_original_segments() {
        assert_eq!(
            build_new_name(
                "Photo.JPG",
                &Rename {
                    replace_from: Some("photo".into()),
                    replace_to: Some("IMG".into()),
                    ignore_case: true,
                    ..base(Path::new("."))
                }
            ),
            Some("IMG.JPG".into())
        );
    }

    #[test]
    fn replace_to_empty_deletes_substring() {
        assert_eq!(
            build_new_name(
                "a_TMP_b.txt",
                &Rename {
                    replace_from: Some("_TMP".into()),
                    replace_to: Some("".into()),
                    ..base(Path::new("."))
                }
            ),
            Some("a_b.txt".into())
        );
    }

    #[test]
    fn dry_run_does_not_touch_disk() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.jpg");
        let app = Rename {
            prefix: Some("IMG_".into()),
            ..base(tmp.path())
        };
        let r = run(&app).unwrap();
        assert_eq!(r["dry_run"], true);
        assert_eq!(r["count"], 1);
        assert!(tmp.path().join("a.jpg").exists(), "dry run must not rename");
        assert!(!tmp.path().join("IMG_a.jpg").exists());
    }

    #[test]
    fn apply_actually_renames() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.jpg");
        touch(tmp.path(), "b.jpg");
        let app = Rename {
            prefix: Some("IMG_".into()),
            apply: true,
            ..base(tmp.path())
        };
        let r = run(&app).unwrap();
        assert_eq!(r["dry_run"], false);
        assert_eq!(r["count"], 2);
        assert!(tmp.path().join("IMG_a.jpg").exists());
        assert!(tmp.path().join("IMG_b.jpg").exists());
        assert!(!tmp.path().join("a.jpg").exists());
    }

    #[test]
    fn pattern_restricts_scope() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.jpg");
        touch(tmp.path(), "b.png");
        let app = Rename {
            pattern: Some("*.jpg".into()),
            prefix: Some("X_".into()),
            apply: true,
            ..base(tmp.path())
        };
        let r = run(&app).unwrap();
        assert_eq!(r["count"], 1, "{r}");
        assert!(tmp.path().join("X_a.jpg").exists());
        assert!(tmp.path().join("b.png").exists(), "png must be untouched");
    }

    #[test]
    fn collision_aborts_before_any_change() {
        let tmp = tempfile::tempdir().unwrap();
        // a_1.txt 与 a1.txt 都要变成 a1.txt（去掉下划线）
        touch(tmp.path(), "a_1.txt");
        touch(tmp.path(), "a1.txt");
        let app = Rename {
            replace_from: Some("_".into()),
            replace_to: Some("".into()),
            apply: true,
            ..base(tmp.path())
        };
        let e = run(&app).unwrap_err();
        assert!(e.to_string().contains("冲突"), "{e}");
        // 两个原文件都还在 → 没有半改
        assert!(tmp.path().join("a_1.txt").exists());
        assert!(tmp.path().join("a1.txt").exists());
    }

    #[test]
    fn target_exists_aborts() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.txt");
        touch(tmp.path(), "P_a.txt");
        let app = Rename {
            prefix: Some("P_".into()),
            apply: true,
            ..base(tmp.path())
        };
        let e = run(&app).unwrap_err();
        assert!(e.to_string().contains("目标已存在"), "{e}");
        assert!(tmp.path().join("a.txt").exists(), "abort must not change anything");
    }

    #[test]
    fn overwrite_flag_allows_existing_target() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), b"original").unwrap();
        fs::write(tmp.path().join("P_a.txt"), b"will-be-replaced").unwrap();
        let app = Rename {
            pattern: Some("a.txt".into()),
            prefix: Some("P_".into()),
            apply: true,
            overwrite: true,
            ..base(tmp.path())
        };
        let r = run(&app).unwrap();
        assert_eq!(r["count"], 1, "{r}");
        // 覆盖后内容来自 a.txt
        assert_eq!(fs::read_to_string(tmp.path().join("P_a.txt")).unwrap(), "original");
    }

    /// 链式改名（a -> P_a，而 P_a 本身也是候选源）必须整体中止。
    ///
    /// 这是一个**真实踩过的缺陷**：早期实现用"目标在源集合里就放行"来判断，
    /// 结果 a.txt -> P_a.txt 会先覆盖掉尚未被移走的 P_a.txt。现在一律中止。
    #[test]
    fn chained_rename_aborts_instead_of_clobbering() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), b"AAA").unwrap();
        fs::write(tmp.path().join("P_a.txt"), b"BBB").unwrap();
        let app = Rename {
            prefix: Some("P_".into()),
            apply: true,
            ..base(tmp.path())
        };
        let e = run(&app).unwrap_err();
        assert!(e.to_string().contains("冲突"), "{e}");
        // 两个文件都必须原样保留
        assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "AAA");
        assert_eq!(fs::read_to_string(tmp.path().join("P_a.txt")).unwrap(), "BBB");
    }

    #[test]
    fn no_mode_is_invalid_arg() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.txt");
        let e = run(&base(tmp.path())).unwrap_err();
        assert!(e.to_string().contains("至少给出一种改名方式"), "{e}");
    }

    #[test]
    fn replace_to_without_from_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let app = Rename {
            replace_to: Some("x".into()),
            ..base(tmp.path())
        };
        let e = run(&app).unwrap_err();
        assert!(e.to_string().contains("必须与 replace-from"), "{e}");
    }

    #[test]
    fn unchanged_names_are_not_counted() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.txt");
        // 替换一个不存在的子串 → 无变化 → 不计入
        let app = Rename {
            replace_from: Some("zzz".into()),
            replace_to: Some("q".into()),
            apply: true,
            ..base(tmp.path())
        };
        let r = run(&app).unwrap();
        assert_eq!(r["count"], 0);
        assert!(tmp.path().join("a.txt").exists());
    }

    /// **幂等保护**：跑第二遍不能把 IMG_a.jpg 变成 IMG_IMG_a.jpg
    ///
    /// 这是真实踩过的 UX 事故：无脑叠加前缀，用户跑两次文件名就废了。
    #[test]
    fn running_twice_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.jpg");
        let first = Rename {
            prefix: Some("IMG_".into()),
            apply: true,
            ..base(tmp.path())
        };
        let r1 = run(&first).unwrap();
        assert_eq!(r1["count"], 1);
        assert!(tmp.path().join("IMG_a.jpg").exists());

        // 第二遍：同一个命令，应跳过而不是叠成 IMG_IMG_a.jpg
        let r2 = run(&first).unwrap();
        assert_eq!(r2["count"], 0, "second run must be a no-op: {r2}");
        assert_eq!(r2["skipped"].as_array().unwrap().len(), 1);
        assert!(
            tmp.path().join("IMG_a.jpg").exists(),
            "must not double-prefix"
        );
        assert!(!tmp.path().join("IMG_IMG_a.jpg").exists());
    }

    /// 后缀同样幂等
    #[test]
    fn suffix_is_also_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.jpg");
        let app = Rename {
            suffix: Some("_bak".into()),
            apply: true,
            ..base(tmp.path())
        };
        run(&app).unwrap();
        assert!(tmp.path().join("a_bak.jpg").exists());
        let r2 = run(&app).unwrap();
        assert_eq!(r2["count"], 0, "{r2}");
        assert!(!tmp.path().join("a_bak_bak.jpg").exists());
    }

    /// `--allow-reapply` 显式关掉幂等保护
    #[test]
    fn allow_reapply_permits_double_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.jpg");
        let app = Rename {
            prefix: Some("IMG_".into()),
            apply: true,
            allow_reapply: true,
            ..base(tmp.path())
        };
        run(&app).unwrap();
        let r2 = run(&app).unwrap();
        assert_eq!(r2["count"], 1, "{r2}");
        assert!(tmp.path().join("IMG_IMG_a.jpg").exists());
    }

    /// replace 模式天然幂等（替换掉就找不到了），不受该保护影响
    #[test]
    fn replace_mode_is_naturally_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a-b.txt");
        let app = Rename {
            replace_from: Some("-".into()),
            replace_to: Some("_".into()),
            apply: true,
            ..base(tmp.path())
        };
        run(&app).unwrap();
        assert!(tmp.path().join("a_b.txt").exists());
        let r2 = run(&app).unwrap();
        assert_eq!(r2["count"], 0, "no more '-' left to replace: {r2}");
    }

    #[test]
    fn already_applied_detects_prefix_and_suffix() {
        let mk = |p: Option<&str>, s: Option<&str>| Rename {
            prefix: p.map(str::to_string),
            suffix: s.map(str::to_string),
            ..base(Path::new("."))
        };
        assert!(already_applied("IMG_a.jpg", &mk(Some("IMG_"), None)));
        assert!(!already_applied("a.jpg", &mk(Some("IMG_"), None)));
        assert!(already_applied("a_bak.jpg", &mk(None, Some("_bak"))));
        assert!(!already_applied("a.jpg", &mk(None, Some("_bak"))));
        // 空串不算已带
        assert!(!already_applied("a.jpg", &mk(Some(""), Some(""))));
    }
}
