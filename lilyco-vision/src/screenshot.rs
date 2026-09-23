//! `html-screenshot` —— 无头浏览器截图（CHROME_PATH 优先，找不到就报可读错误）。

use lilyco::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::common::default_output;

// ── 8. html-screenshot ─────────────────────────────────────

/// 无头浏览器截图（无额外依赖；找不到浏览器时报 Runtime 错）
#[derive(App)]
#[app(
    run = "run_html_screenshot",
    about = "Screenshot an HTML file with a headless browser: CHROME_PATH, then platform browser installs (Chrome/Edge on Windows, chromium/google-chrome on Unix) are used; waits up to 30s and returns { out, browser, exit_code }."
)]
pub(crate) struct HtmlScreenshot {
    /// 本地 html/htm 文件
    #[arg(about = "HTML file (.html/.htm)", must_exist = true)]
    pub(crate) source: PathBuf,

    /// 视口宽度
    #[arg(default = 1280, range = 320..=8192)]
    pub(crate) width: u16,

    /// 视口高度
    #[arg(default = 800, range = 240..=8192)]
    pub(crate) height: u16,

    /// 输出路径（缺省：输入同目录 {stem}.png，对齐上游 default_output）
    #[arg(about = "Output PNG path")]
    pub(crate) out: Option<String>,
}

/// Windows 常见浏览器安装路径
pub(crate) const WINDOWS_BROWSER_CANDIDATES: &[&str] = &[
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
];

/// Unix 常见浏览器可执行名（PATH 查找）
#[cfg(unix)]
pub(crate) const UNIX_BROWSER_NAMES: &[&str] = &["chromium", "google-chrome", "chromium-browser"];

/// PATHEXT 扩展名列表（Windows 解析 PATH 用；仅 Unix PATH 查找使用）
#[cfg(unix)]
pub(crate) fn path_exts() -> Vec<String> {
    std::env::var("PATHEXT")
        .map(|e| e.split(';').map(|s| s.trim().to_lowercase()).collect())
        .unwrap_or_else(|_| vec![".exe".into(), ".bat".into(), ".cmd".into()])
}

/// 按 PATH 找可执行文件（Unix 浏览器查找）
#[cfg(unix)]
pub(crate) fn find_on_path(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    let sep = if cfg!(windows) { ';' } else { ':' };
    for dir in path.split(sep).filter(|s| !s.is_empty()) {
        let base = Path::new(dir).join(name);
        if cfg!(windows) {
            for ext in path_exts() {
                let candidate = format!("{}{}", base.display(), ext);
                if Path::new(&candidate).is_file() {
                    return Some(candidate);
                }
            }
        } else if base.is_file() {
            return Some(base.display().to_string());
        }
    }
    None
}

/// 解析可用浏览器。
///
/// 优先级：`CHROME_PATH` 环境变量（显式设置但不可用时直接判无，不回落到系统安装，
/// 便于 CI/测试确定性）→ 平台候选路径 → Unix PATH 查找。
pub(crate) fn find_browser() -> Option<String> {
    if let Ok(p) = std::env::var("CHROME_PATH") {
        let p = p.trim().to_string();
        if !p.is_empty() {
            if Path::new(&p).is_file() {
                return Some(p);
            }
            return None;
        }
    }
    for c in WINDOWS_BROWSER_CANDIDATES {
        if Path::new(c).is_file() {
            return Some(c.to_string());
        }
    }
    #[cfg(unix)]
    for name in UNIX_BROWSER_NAMES {
        if let Some(p) = find_on_path(name) {
            return Some(p);
        }
    }
    None
}

/// 无浏览器时的错误消息（列出所有已检查项）
pub(crate) fn no_browser_error() -> AppError {
    let extra = if cfg!(unix) {
        "chromium / google-chrome / chromium-browser on PATH".to_string()
    } else {
        WINDOWS_BROWSER_CANDIDATES.to_vec().join(", ")
    };
    AppError::Runtime(format!(
        "no browser found for html screenshot: checked CHROME_PATH, then {extra}"
    ))
}
/// 业务逻辑：校验扩展名 → 找浏览器 → 无头截图（30s 超时 kill）→ 校验产物
pub(crate) fn run_html_screenshot(
    app: &HtmlScreenshot,
    ctx: &Context,
) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("html-screenshot {}", app.source.display())),
    });

    // 1. 扩展名校验
    let ext = app
        .source
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if ext != "html" && ext != "htm" {
        return Err(AppError::InvalidArg(format!(
            "source must be an .html/.htm file, got {}",
            app.source.display()
        )));
    }

    // 2. 浏览器解析
    let browser = find_browser().ok_or_else(no_browser_error)?;

    // 3. 组装 file:// URL（绝对路径 + 正斜杠）
    let abs = std::fs::canonicalize(&app.source)?;
    let file_url = format!("file:///{}", abs.display().to_string().replace('\\', "/"));

    let width = if app.width == 0 { 1280 } else { app.width };
    let height = if app.height == 0 { 800 } else { app.height };
    let out = app
        .out
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output(&app.source, ".png"));

    let mut cmd = Command::new(&browser);
    cmd.arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--hide-scrollbars")
        .arg(format!("--screenshot={}", out.display()))
        .arg(format!("--window-size={width},{height}"))
        .arg(&file_url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Runtime(format!("spawn {browser}: {e}")))?;

    // 4. try_wait 轮询 + 30s 超时 kill（不引入 wait-timeout 依赖）
    let timeout = Duration::from_secs(30);
    let deadline = start + timeout;
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::Runtime(format!(
                        "browser timed out after {}s: {browser}",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(AppError::Runtime(format!("wait {browser}: {e}"))),
        }
    };

    // 5. 回收 stderr 尾部（子进程已退出，读不阻塞）
    let stderr_tail = child
        .stderr
        .take()
        .map(|mut s| {
            let mut buf = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut buf);
            let start_i = buf.len().saturating_sub(1000);
            buf[start_i..].to_string()
        })
        .unwrap_or_default();

    // 6. 校验产物非空
    if !out.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        let tail = if stderr_tail.trim().is_empty() {
            "(empty)"
        } else {
            stderr_tail.trim()
        };
        return Err(AppError::Runtime(format!(
            "browser exited {browser:?} with code {exit_code:?} and produced no non-empty screenshot at {}; stderr tail: {tail}",
            out.display()
        )));
    }

    let result = serde_json::json!({
        "out": out.display().to_string(),
        "browser": browser,
        "exit_code": exit_code,
        "ok": true,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}
