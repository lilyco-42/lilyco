//! `lilyco build` / `lilyco run` — cargo 的薄包装。
//!
//! 不重新发明构建：参数组装是纯函数（可单测），执行是继承 stdout/stderr 的
//! `std::process::Command`（用户看得到 cargo 原生输出，退出码透传）。
//! 元 CLI 的价值只在把生态的标准 profile 编码进来：
//!
//! - Android headless：`--target aarch64-linux-android --no-default-features
//!   --features android`（模板 Cargo.toml 注释里的既定 profile，crossterm
//!   不支持 Android，应用 crate 靠 `android` feature 退到 CLI + MCP）
//! - 表面选择：`--surface tui|web|mcp` → 透传 `--tui` / `--gui` / `--mcp`
//!   给 app（与 facade `detect_backend` 的显式标志一一对应）

use lilyco::prelude::*;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

/// 运行表面（透传给 app 的后端选择标志）
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// CLI（app 缺省形态，不传标志）
    Cli,
    /// TUI 命令选择页（--tui）
    Tui,
    /// Web 控制台（--gui）
    Web,
    /// MCP stdio 服务器（--mcp）
    Mcp,
}

impl Surface {
    /// 对应 app 的显式后端标志；Cli 不传（app 自动探测即落 CLI/管道）
    pub fn app_flag(self) -> Option<&'static str> {
        match self {
            Surface::Cli => None,
            Surface::Tui => Some("--tui"),
            Surface::Web => Some("--gui"),
            Surface::Mcp => Some("--mcp"),
        }
    }
}

/// `cargo build` 参数（纯函数，可单测）
pub fn build_args(release: bool, android: bool, target: Option<&str>) -> Vec<String> {
    let mut v = vec!["build".to_string()];
    if release {
        v.push("--release".to_string());
    }
    if android {
        v.push("--target".to_string());
        v.push(target.unwrap_or("aarch64-linux-android").to_string());
        v.push("--no-default-features".to_string());
        v.push("--features".to_string());
        v.push("android".to_string());
    }
    v
}

/// `cargo run` 参数（纯函数，可单测）：`--` 之后的全部进 app
pub fn run_args(release: bool, surface: Option<Surface>, app_args: &[String]) -> Vec<String> {
    let mut v = vec!["run".to_string()];
    if release {
        v.push("--release".to_string());
    }
    v.push("--".to_string());
    if let Some(flag) = surface.and_then(|s| s.app_flag()) {
        v.push(flag.to_string());
    }
    v.extend(app_args.iter().cloned());
    v
}

/// 执行 cargo：继承 stdout/stderr（用户看原生输出），退出码透传；
/// cargo 不在 PATH → 打印原因并返回 127
pub fn exec_cargo(project: Option<&std::path::Path>, args: &[String]) -> i32 {
    let mut cmd = Command::new("cargo");
    cmd.args(args);
    if let Some(p) = project {
        cmd.current_dir(p);
    }
    match cmd.status() {
        Ok(st) => st.code().unwrap_or(1),
        Err(e) => {
            eprintln!("Error: 无法启动 cargo（在 PATH 里吗？）: {e}");
            127
        }
    }
}

/// `lilyco build` — 编排 cargo build
#[derive(App)]
#[app(
    name = "build",
    run = "run_build",
    safety = "t1",
    about = "Thin wrapper over `cargo build` for the target project (default: current directory): --release selects the release profile, --android selects the ecosystem's Android headless profile (--target aarch64-linux-android --no-default-features --features android, requires the NDK toolchain) with an optional --target override. External process invocation (safety T1); returns { command, exit }."
)]
pub struct Build {
    /// 项目目录（缺省 = 当前目录）
    pub project: Option<PathBuf>,

    /// release profile
    pub release: bool,

    /// Android headless profile（需 NDK 工具链在位）
    pub android: bool,

    /// 覆盖 Android 目标 triple（仅 --android 时生效）
    pub target: Option<String>,
}

fn run_build(app: &Build, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let args = build_args(app.release, app.android, app.target.as_deref());
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some(format!("cargo {}", args.join(" "))),
    });
    let code = exec_cargo(app.project.as_deref(), &args);
    if code != 0 {
        return Err(AppError::Runtime(format!(
            "cargo build 失败（exit {code}）—— 输出见上方"
        )));
    }
    let result = json!({
        "command": format!("cargo {}", args.join(" ")),
        "exit": code,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// `lilyco run` — cargo run + 表面选择
#[derive(App)]
#[app(
    name = "run",
    run = "run_run",
    safety = "t1",
    about = "Run the app of the target project (default: current directory) via `cargo run -- <flags> <args>`: --surface selects which surface flag is forwarded to the app (cli forwards nothing, tui -> --tui, web -> --gui, mcp -> --mcp) and --app-arg values are forwarded verbatim after the surface flag. External process invocation running arbitrary app code (safety T1); returns { command, exit }."
)]
pub struct Run {
    /// 项目目录（缺省 = 当前目录）
    pub project: Option<PathBuf>,

    /// release profile
    pub release: bool,

    /// 转发给 app 的表面标志（cli 不传 / tui / web / mcp）
    pub surface: Option<Surface>,

    /// 原样转发给 app 的参数（放在表面标志之后）
    pub app_args: Vec<String>,
}

fn run_run(app: &Run, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let args = run_args(app.release, app.surface, &app.app_args);
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some(format!("cargo {}", args.join(" "))),
    });
    let code = exec_cargo(app.project.as_deref(), &args);
    if code != 0 {
        return Err(AppError::Runtime(format!(
            "cargo run 失败（exit {code}）—— 输出见上方"
        )));
    }
    let result = json!({
        "command": format!("cargo {}", args.join(" ")),
        "exit": code,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_profile_matrix() {
        assert_eq!(build_args(false, false, None), vec!["build"]);
        assert_eq!(build_args(true, false, None), vec!["build", "--release"]);
        assert_eq!(
            build_args(false, true, None),
            vec![
                "build",
                "--target",
                "aarch64-linux-android",
                "--no-default-features",
                "--features",
                "android",
            ]
        );
        assert!(
            build_args(false, true, Some("x86_64-linux-android"))
                .contains(&"x86_64-linux-android".to_string()),
            "--target 覆盖应生效"
        );
    }

    #[test]
    fn run_args_forward_surface_then_app_args() {
        assert_eq!(run_args(false, None, &[]), vec!["run", "--"]);
        assert_eq!(run_args(false, Some(Surface::Cli), &[]), vec!["run", "--"]);
        assert_eq!(
            run_args(false, Some(Surface::Tui), &[]),
            vec!["run", "--", "--tui"]
        );
        assert_eq!(
            run_args(
                true,
                Some(Surface::Mcp),
                &["--name".to_string(), "x".to_string()]
            ),
            vec!["run", "--release", "--", "--mcp", "--name", "x"]
        );
    }

    #[test]
    fn surface_flag_mapping_matches_facade_detect_flags() {
        assert_eq!(Surface::Tui.app_flag(), Some("--tui"));
        assert_eq!(
            Surface::Web.app_flag(),
            Some("--gui"),
            "Web 面的显式标志是 --gui（facade detect_backend 认 --gui/--web）"
        );
        assert_eq!(Surface::Mcp.app_flag(), Some("--mcp"));
        assert_eq!(Surface::Cli.app_flag(), None);
    }
}
