//! lbrush — brush（Bo[u]rn[e] RUsty SHell，bash 兼容 shell）作为 lilyco App 提供给 AI。
//!
//! 取代原来的 JS 版 `dsh-tool-brush`：用 lilyco 框架重写，天然四端 + AI 可调。
//! ```bash
//! lbrush --command "ls -la"                     # CLI
//! lbrush --command "x=1; echo $x" --cwd D:/tmp  # 工作目录
//! lbrush --command "sleep 5" --timeout-secs 1   # 超时 kill
//! lbrush --command "ls" --json-stream           # AI/脚本消费（JSONL）
//! lbrush --mcp                                  # MCP stdio 服务器（dsh-mcp-client 直连）
//! ```
//!
//! 语义（与旧版 brush 工具对齐）：
//! - 每次调用全新 shell（brush `--no-config`），无状态泄漏
//! - 非零退出码**不是**工具错误：结果 JSON 携带 `exit_code`，由调用方解释
//! - 超时 kill 并报告 `timed_out: true`；输出按 64KB 截断并标记
//! - Git for Windows coreutils 自动进 PATH（ls/cat/grep/wc/…）

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use lilyco::prelude::*;

/// 输出截断上限（与旧版一致）
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// brush.exe 常见安装位置（scoop / cargo）
const BRUSH_CANDIDATES: &[&str] = &[
    "D:\\app\\scoop\\apps\\rustup\\current\\.cargo\\bin\\brush.exe",
    "D:\\app\\scoop\\shims\\brush.exe",
];

/// Git Bash 回退路径（brush 缺失时用真 bash）
const GIT_BASH_CANDIDATES: &[&str] = &[
    "C:\\Program Files\\Git\\bin\\bash.exe",
    "C:\\Program Files\\Git\\usr\\bin\\bash.exe",
];

/// Git for Windows coreutils 目录（前置到 PATH）
const GIT_USR_BIN_CANDIDATES: &[&str] = &[
    "C:\\Program Files\\Git\\usr\\bin",
    "D:\\app\\scoop\\apps\\git\\current\\usr\\bin",
];

/// 执行 bash 命令的 brush shell
#[derive(App)]
#[app(
    run = "run_brush",
    about = "Execute a bash command with the brush shell (Bo[u]rn[e] RUsty SHell — real bash syntax, no Nushell translation layer) and return structured JSON { exit_code, stdout, stderr, timed_out, duration_ms }. Full bash compatibility: variables (x=1; echo $x), command substitution ($(cmd) and backticks), pipelines, redirection (>, 2>, 2>&1, /dev/null), && / ||, if/for/while/case, functions, arrays, arithmetic $(( )), $?, $1, printf, tilde. Git for Windows coreutils (ls, cat, grep, wc, tr, head, tail, tee, xargs, find, sed, awk, ...) are on PATH when available. Each call runs in a fresh shell: no state (cwd, variables, exports) persists — pass `cwd` instead of using `cd`. Paths accept native Windows form (C:\\...) and POSIX form. Non-zero exits are NOT tool errors: the result carries exit_code for the caller to interpret. Timeouts kill the shell and report timed_out: true. Output is capped at 64KB per stream with a *_truncated flag."
)]
struct Brush {
    /// bash 命令（每次调用全新 shell）
    #[arg(about = "Bash command to execute")]
    command: String,

    /// 工作目录（默认：当前目录）
    #[arg(about = "Working directory (default: current)")]
    cwd: Option<String>,

    /// 超时秒数（超时 kill 并报 timed_out）
    #[arg(default = 120, range = 1..=600)]
    timeout_secs: u64,
}

/// 业务逻辑：解析 shell → spawn → 读输出 → 等待/超时 → 结构化结果
fn run_brush(app: &Brush, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();

    if app.command.trim().is_empty() {
        return Err(AppError::InvalidArg("command must not be empty".into()));
    }
    let cwd = app.cwd.clone().unwrap_or_else(|| ".".to_string());
    if !Path::new(&cwd).is_dir() {
        return Err(AppError::InvalidArg(format!(
            "cwd is not a directory: {cwd}"
        )));
    }

    let (shell, shell_kind) = resolve_shell()?;
    let path = build_path_env();

    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("[{shell_kind}] {}", app.command)),
    });

    // 参数按 shell 区分：brush 支持 --no-config（全新 shell），bash 不支持
    let mut cmd = Command::new(&shell);
    match shell_kind {
        "brush" => {
            cmd.arg("--no-config").arg("-c").arg(&app.command);
        }
        _ => {
            cmd.arg("-c").arg(&app.command);
        }
    }
    cmd.current_dir(&cwd)
        .env("PATH", path)
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Runtime(format!("spawn {shell}: {e}")))?;

    // 双线程读管道，避免输出大时管道写满死锁
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let out_t = std::thread::spawn(move || read_with_cap(stdout));
    let err_t = std::thread::spawn(move || read_with_cap(stderr));

    // 带超时等待；超时则 kill
    let timeout = Duration::from_secs(app.timeout_secs.max(1));
    let (exit_code, timed_out) = match child.wait_timeout(timeout) {
        Ok(Some(status)) => (status.code(), false),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            (None, true)
        }
        Err(e) => return Err(AppError::Runtime(format!("wait: {e}"))),
    };

    let (stdout_text, stdout_truncated) = out_t
        .join()
        .map_err(|_| AppError::Runtime("stdout reader panicked".into()))?;
    let (stderr_text, stderr_truncated) = err_t
        .join()
        .map_err(|_| AppError::Runtime("stderr reader panicked".into()))?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let result = serde_json::json!({
        "shell": shell_kind,
        "command": app.command,
        "cwd": cwd,
        "exit_code": exit_code.map(serde_json::json).unwrap_or(serde_json::Value::Null),
        "timed_out": timed_out,
        "duration_ms": duration_ms,
        "stdout": stdout_text,
        "stderr": stderr_text,
        "stdout_truncated": stdout_truncated,
        "stderr_truncated": stderr_truncated,
    });
    ctx.done(result.clone(), duration_ms);
    Ok(result)
}

/// 读取管道最多 MAX_OUTPUT_BYTES；超出部分继续排空（防子进程写阻塞）并标记截断
fn read_with_cap(mut reader: impl Read) -> (String, bool) {
    let mut kept: Vec<u8> = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if kept.len() + n > MAX_OUTPUT_BYTES {
            let room = MAX_OUTPUT_BYTES - kept.len();
            kept.extend_from_slice(&chunk[..room]);
            truncated = true;
            let mut sink = [0u8; 8192];
            while reader.read(&mut sink).unwrap_or(0) > 0 {}
            break;
        }
        kept.extend_from_slice(&chunk[..n]);
    }
    (String::from_utf8_lossy(&kept).into_owned(), truncated)
}

// ── shell 解析 ────────────────────────────────────────────

/// 解析要用的 shell：BRUSH_PATH > brush 候选路径 > PATH brush > Git Bash > PATH bash
fn resolve_shell() -> Result<(String, &'static str), AppError> {
    if let Ok(p) = std::env::var("BRUSH_PATH") {
        if !p.is_empty() && Path::new(&p).exists() {
            return Ok((p, "brush"));
        }
    }
    for c in BRUSH_CANDIDATES {
        if Path::new(c).exists() {
            return Ok((c.to_string(), "brush"));
        }
    }
    if let Some(p) = find_on_path("brush") {
        return Ok((p, "brush"));
    }
    for c in GIT_BASH_CANDIDATES {
        if Path::new(c).exists() {
            return Ok((c.to_string(), "bash"));
        }
    }
    if let Some(p) = find_on_path("bash") {
        return Ok((p, "bash"));
    }
    Err(AppError::Runtime(
        "no shell found: checked BRUSH_PATH, brush.exe candidates, PATH, Git Bash — \
         install brush (scoop install brush / cargo install brush) or Git for Windows"
            .into(),
    ))
}

/// Git for Windows coreutils 目录（coreutils 进 PATH）
fn resolve_git_usr_bin() -> Option<String> {
    if let Ok(p) = std::env::var("GIT_USR_BIN") {
        if !p.is_empty() && Path::new(&p).is_dir() {
            return Some(p);
        }
    }
    GIT_USR_BIN_CANDIDATES
        .iter()
        .find(|c| Path::new(c).is_dir())
        .map(|c| c.to_string())
}

/// 构建 PATH：Git usr/bin 前置 + 继承现有 PATH
fn build_path_env() -> String {
    let existing = std::env::var("PATH").unwrap_or_default();
    match resolve_git_usr_bin() {
        Some(gub) if !existing.split(';').any(|p| p.eq_ignore_ascii_case(&gub)) => {
            format!("{gub};{existing}")
        }
        _ => existing,
    }
}

/// 在 PATH 上找可执行文件（Windows 带 PATHEXT 扩展）
fn find_on_path(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    let exts: Vec<String> = std::env::var("PATHEXT")
        .map(|e| e.split(';').map(|s| s.to_lowercase()).collect())
        .unwrap_or_else(|_| vec![".exe".into(), ".bat".into(), ".cmd".into()]);
    for dir in path.split(';') {
        if dir.is_empty() {
            continue;
        }
        let base = Path::new(dir).join(name);
        for ext in &exts {
            let candidate = format!("{}{}", base.display(), ext);
            if Path::new(&candidate).is_file() {
                return Some(candidate);
            }
        }
        // 无扩展名的可执行（Unix 风格）
        if Path::new(&base).is_file() {
            return Some(base.display().to_string());
        }
    }
    None
}

// ── main: one line ────────────────────────────────────────

fn main() {
    lilyco::run::<Brush>();
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试辅助：跑一次 brush，返回结果 JSON
    fn run(args: &[(String, serde_json::Value)]) -> Result<serde_json::Value, AppError> {
        let mut app = Brush {
            command: String::new(),
            cwd: None,
            timeout_secs: 30,
        };
        for (k, v) in args {
            match k.as_str() {
                "command" => app.command = v.as_str().unwrap().to_string(),
                "cwd" => app.cwd = Some(v.as_str().unwrap().to_string()),
                "timeout_secs" => app.timeout_secs = v.as_u64().unwrap(),
                _ => {}
            }
        }
        let (tx, _rx) = std::sync::mpsc::channel();
        let ctx = Context::new_test(tx);
        run_brush(&app, &ctx)
    }

    fn shell_available() -> bool {
        resolve_shell().is_ok()
    }

    #[test]
    fn echo_runs_and_captures_stdout() {
        if !shell_available() {
            eprintln!("skip: no brush/bash");
            return;
        }
        let r = run(&[("command".into(), serde_json::json!("echo hello lbrush"))]).unwrap();
        assert_eq!(r["exit_code"], 0);
        assert!(r["stdout"].as_str().unwrap().contains("hello lbrush"));
        assert_eq!(r["timed_out"], false);
    }

    #[test]
    fn exit_code_is_propagated_not_errored() {
        if !shell_available() {
            eprintln!("skip: no brush/bash");
            return;
        }
        let r = run(&[("command".into(), serde_json::json!("exit 3"))]).unwrap();
        assert_eq!(r["exit_code"], 3, "非零退出码应出现在结果里而不是报错");
    }

    #[test]
    fn stderr_is_captured() {
        if !shell_available() {
            eprintln!("skip: no brush/bash");
            return;
        }
        let r = run(&[("command".into(), serde_json::json!("echo boom 1>&2"))]).unwrap();
        assert!(r["stderr"].as_str().unwrap().contains("boom"));
    }

    #[test]
    fn cwd_is_honored() {
        if !shell_available() {
            eprintln!("skip: no brush/bash");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let dir_name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let r = run(&[
            ("command".into(), serde_json::json!("pwd")),
            (
                "cwd".into(),
                serde_json::json!(dir.path().display().to_string()),
            ),
        ])
        .unwrap();
        assert_eq!(r["exit_code"], 0);
        assert!(
            r["stdout"].as_str().unwrap().contains(&dir_name),
            "pwd 应落在 cwd 目录内: {}",
            r["stdout"]
        );
    }

    #[test]
    fn timeout_kills_and_reports() {
        if !shell_available() {
            eprintln!("skip: no brush/bash");
            return;
        }
        let started = Instant::now();
        let r = run(&[
            ("command".into(), serde_json::json!("sleep 5")),
            ("timeout_secs".into(), serde_json::json!(1)),
        ])
        .unwrap();
        assert_eq!(r["timed_out"], true);
        assert!(r["exit_code"].is_null());
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "超时应在 1s 附近触发，实际 {}ms",
            started.elapsed().as_millis()
        );
    }

    #[test]
    fn missing_cwd_is_rejected() {
        let r = run(&[
            ("command".into(), serde_json::json!("echo hi")),
            ("cwd".into(), serde_json::json!("C:/definitely/not/a/dir")),
        ]);
        assert!(r.is_err());
    }

    #[test]
    fn empty_command_is_rejected() {
        let r = run(&[("command".into(), serde_json::json!("  "))]);
        assert!(r.is_err());
    }

    #[test]
    fn bash_syntax_works_through_brush() {
        if !shell_available() {
            eprintln!("skip: no brush/bash");
            return;
        }
        let r = run(&[(
            "command".into(),
            serde_json::json!("x=1; y=2; echo $((x + y)); ls /nonexistent 2>/dev/null; echo $?"),
        )])
        .unwrap();
        let out = r["stdout"].as_str().unwrap();
        assert!(out.contains('3'), "算术 $((x+y)) 应得 3, got: {out}");
        assert!(out.contains('0'), "2>/dev/null 后 $? 应为 0, got: {out}");
    }
}
