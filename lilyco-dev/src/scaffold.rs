//! `lilyco new <name>` — 脚手架：把 `scripts/domain-template/`（单一事实源，
//! `include_str!` 内嵌进二进制）实例化为 `lilyco-<name>` crate（二进制 `l<name>`）。
//!
//! 为什么内嵌而不是读磁盘路径：`cargo install` 出来的元 CLI 手上没有仓库
//! 检出，模板必须跟着二进制走（cargo new 同理）。代价是改模板要重编元 CLI
//! —— 由 CI 统一验证，可接受。
//!
//! 发布注意：`include_str!` 引用了包目录之外的文件，crates.io 打包不含
//! `scripts/` —— 发布 lilyco-dev 前需把模板挪进包内（当前仓库优先，未发布）。

use lilyco::prelude::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const TEMPLATE_CARGO_TOML: &str = include_str!("../../scripts/domain-template/Cargo.toml");
pub const TEMPLATE_MAIN_RS: &str = include_str!("../../scripts/domain-template/src/main.rs");
pub const TEMPLATE_SHOW_RS: &str = include_str!("../../scripts/domain-template/src/show.rs");

/// 域名规则：小写字母开头，只含小写字母/数字/连字符，不以连字符结尾、
/// 不含连续连字符，≤32 字符。返回 Err(原因)。
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("域名不能为空".to_string());
    }
    if name.len() > 32 {
        return Err(format!("域名过长（{}/32 字符）", name.len()));
    }
    let first = name.chars().next().unwrap_or('?');
    if !first.is_ascii_lowercase() {
        return Err(format!("域名必须以小写字母开头（得到 `{first}`）"));
    }
    for c in name.chars().skip(1) {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(format!("域名只允许小写字母/数字/连字符（得到 `{c}`）"));
        }
    }
    if name.ends_with('-') {
        return Err("域名不能以连字符结尾".to_string());
    }
    if name.contains("--") {
        return Err("域名不能含连续连字符".to_string());
    }
    Ok(())
}

/// 占位符替换：`@CRATE@` → crate 名（`lilyco-<name>`），`@BIN@` → 二进制名（`l<name>`）
pub fn substitute(template: &str, crate_name: &str, bin_name: &str) -> String {
    template
        .replace("@CRATE@", crate_name)
        .replace("@BIN@", bin_name)
}

/// 在 `parent` 下实例化模板；目标目录已存在时拒绝（与 cargo new 一致，绝不覆盖）
pub fn scaffold_files(name: &str, parent: &Path) -> Result<(PathBuf, Vec<PathBuf>), String> {
    validate_name(name)?;
    let crate_name = format!("lilyco-{name}");
    let bin_name = format!("l{name}");
    let dir = parent.join(&crate_name);
    if dir.exists() {
        return Err(format!("{} 已存在 —— 拒绝覆盖", dir.display()));
    }
    let src = dir.join("src");
    std::fs::create_dir_all(&src).map_err(|e| format!("建目录 {} 失败: {e}", src.display()))?;
    let files = [
        (
            dir.join("Cargo.toml"),
            substitute(TEMPLATE_CARGO_TOML, &crate_name, &bin_name),
        ),
        (
            dir.join("src").join("main.rs"),
            substitute(TEMPLATE_MAIN_RS, &crate_name, &bin_name),
        ),
        (
            dir.join("src").join("show.rs"),
            substitute(TEMPLATE_SHOW_RS, &crate_name, &bin_name),
        ),
    ];
    let mut written = Vec::new();
    for (path, body) in files {
        std::fs::write(&path, &body).map_err(|e| format!("写 {} 失败: {e}", path.display()))?;
        written.push(path);
    }
    Ok((dir, written))
}

/// `lilyco new` — 生成一个四端域应用骨架
#[derive(App)]
#[app(
    name = "new",
    run = "run_new",
    safety = "t1",
    about = "Scaffold a new four-surface (CLI / TUI / Web / MCP) domain app from the embedded domain template: creates directory `lilyco-<name>` with Cargo.toml, src/main.rs and src/show.rs, substituting @CRATE@ -> lilyco-<name> and @BIN@ -> l<name>. Never overwrites an existing directory. Writes files (safety T1); returns { crate_name, bin_name, dir, files }. The generated crate expects to live inside the lilyco workspace (path dependency on ../lilyco)."
)]
pub struct New {
    /// 域名（小写字母开头；生成 crate `lilyco-<name>`、二进制 `l<name>`）
    pub name: String,

    /// 目标父目录（缺省 = 当前目录）
    pub dir: Option<PathBuf>,
}

fn run_new(app: &New, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    validate_name(&app.name).map_err(AppError::InvalidInput)?;
    let parent = app.dir.clone().unwrap_or_else(|| PathBuf::from("."));
    ctx.emit(Progress::Started {
        total: Some(3),
        message: Some(format!("scaffolding lilyco-{}", app.name)),
    });
    let (dir, files) = scaffold_files(&app.name, &parent).map_err(AppError::Runtime)?;
    ctx.tick(3, Some(3), "");
    let result = json!({
        "crate_name": format!("lilyco-{}", app.name),
        "bin_name": format!("l{}", app.name),
        "dir": dir.to_string_lossy(),
        "files": files.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模板里必须还有占位符 —— 若有人把 scripts/domain-template 的占位符
    /// 「填实」了，new 的替换契约就断了，第一时间在这里炸
    #[test]
    fn embedded_templates_carry_the_placeholders() {
        assert!(
            TEMPLATE_CARGO_TOML.contains("@CRATE@"),
            "Cargo.toml 模板丢了 @CRATE@"
        );
        assert!(
            TEMPLATE_CARGO_TOML.contains("@BIN@"),
            "Cargo.toml 模板丢了 @BIN@"
        );
        assert!(TEMPLATE_MAIN_RS.contains("@BIN@"), "main.rs 模板丢了 @BIN@");
        assert!(TEMPLATE_SHOW_RS.contains("@BIN@"), "show.rs 模板丢了 @BIN@");
    }

    #[test]
    fn substitution_replaces_every_placeholder() {
        let out = substitute(TEMPLATE_CARGO_TOML, "lilyco-todo", "ltodo");
        assert!(!out.contains("@CRATE@"), "@CRATE@ 没换干净");
        assert!(!out.contains("@BIN@"), "@BIN@ 没换干净");
        assert!(
            out.contains("name = \"lilyco-todo\""),
            "crate 名没落到 [package] name"
        );
        assert!(
            out.contains("name = \"ltodo\""),
            "bin 名没落到 [[bin]] name"
        );
    }

    #[test]
    fn name_validation_table() {
        for ok in ["todo", "a", "my-tool", "tool2", "x-1-y"] {
            assert!(validate_name(ok).is_ok(), "`{ok}` 应合法");
        }
        for bad in [
            "",
            "Todo",
            "1tool",
            "-tool",
            "tool-",
            "to--ol",
            "我的",
            "tool name",
            "t.o",
        ] {
            assert!(validate_name(bad).is_err(), "`{bad}` 应被拒");
        }
    }

    #[test]
    fn scaffolds_three_files_and_never_overwrites() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (dir, files) = scaffold_files("todo", tmp.path()).expect("脚手架应成功");
        assert_eq!(files.len(), 3, "模板 = Cargo.toml + main.rs + show.rs");
        let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).expect("读 Cargo.toml");
        assert!(cargo.contains("name = \"lilyco-todo\""));
        let main_rs = std::fs::read_to_string(dir.join("src").join("main.rs")).expect("读 main.rs");
        assert!(main_rs.contains("ltodo"), "main.rs 里应有 bin 名");
        assert!(!main_rs.contains("@BIN@"), "main.rs 占位符没换干净");
        // 绝不覆盖：同目录再来一次必须报错
        let err = scaffold_files("todo", tmp.path()).expect_err("二次脚手架应被拒");
        assert!(err.contains("拒绝覆盖"), "错误要说明拒绝覆盖: {err}");
    }
}
