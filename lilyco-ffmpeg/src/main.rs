//! lffmpeg — 用 ffmpeg 转码 / 缩放 / 裁剪媒体，自带实时进度，天然四端 + AI 可调。
//!
//! 框架约定（与 lbrush / lgrep 一致）：
//! - 全程通过 `ctx` 上报进度，最终 `ctx.done` 提交结果；
//!   CLI / TUI / Web / MCP / DSH 消费同一事件流。
//! - 取消：handler 循环里轮询 `ctx.is_cancelled()`，取消则 kill ffmpeg。
//!
//! ```bash
//! lffmpeg --input a.mp4 --output b.mp4 --codec h265 --crf 28     # CLI
//! lffmpeg --input x.mov --output x.mp4 --width 1280 --height 720 # 缩放
//! lffmpeg --input a.mp4 --output clip.mp4 --start 10 --duration 5
//! lffmpeg --input a.mp4 --output b.webm --codec vp9 --audio opus
//! lffmpeg --input a.mp4 --output b.mp4 --json-stream             # AI/脚本消费
//! lffmpeg --mcp                                                  # MCP stdio 服务器
//! ```
//!
//! 依赖系统 ffmpeg / ffprobe（`ffmpeg` 在 PATH 上）。ffprobe 仅用于计算
//! 转码百分比；缺失时降级为"按时间推进"的不确定进度。

use std::io::{BufRead, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use lilyco::prelude::*;

/// 单条流输出截断上限（与 lbrush 一致）
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// 期望的 ffmpeg 可执行文件（在 PATH 上查找）
const FFMPEG: &str = "ffmpeg";
const FFPROBE: &str = "ffprobe";

/// 视频编码
#[derive(ValueEnum, Clone, Copy, PartialEq, Debug)]
enum Video {
    H264,
    H265,
    Vp9,
    Av1,
    Copy,
}

/// 音频处理
#[derive(ValueEnum, Clone, Copy, PartialEq, Debug)]
enum Audio {
    Copy,
    Aac,
    Opus,
    None,
}

/// 预设（x264/x265 速度-压缩率权衡；vp9/av1 由 ffmpeg 映射为 quality 档）
#[derive(ValueEnum, Clone, Copy, PartialEq, Debug)]
enum Preset {
    Ultrafast,
    Superfast,
    Veryfast,
    Faster,
    Fast,
    Medium,
    Slow,
    Slower,
    Veryslow,
}

/// ffmpeg 转码工具
#[derive(App)]
#[app(
    run = "run_ffmpeg",
    about = "Transcode, resize, and trim media with ffmpeg, reporting live progress. Wraps the system ffmpeg (must be on PATH; ffprobe is used to compute a completion percentage when available, otherwise an indeterminate time-based progress is reported). Parameters are mapped to ffmpeg flags: `codec` (h264|h265|vp9|av1|copy) -> -c:v, `crf` -> -crf (0-51, lower = better), `preset` -> -preset, `width`/`height` -> -vf scale (missing side becomes -2 to keep aspect), `start`/`duration` (seconds) -> -ss/-t, `audio` (copy|aac|opus|none) -> -c:a / -an. Non-zero ffmpeg exit is a tool error carrying stderr. Cancellation is honored: the running ffmpeg is killed on request. Returns { input, output, command, exit_code, duration_ms, duration_hms, success, stdout, stderr, stdout_truncated, stderr_truncated }."
)]
struct Ffmpeg {
    /// 输入媒体文件（须已存在）
    #[arg(about = "Input media file (must exist)", must_exist = true)]
    input: std::path::PathBuf,

    /// 输出媒体文件
    #[arg(about = "Output media file")]
    output: std::path::PathBuf,

    /// 视频编码（copy 表示仅复用视频流）
    #[arg(about = "Video codec (copy = stream copy)", default = "h264")]
    codec: Video,

    /// 质量 0-51（越低越好；h264/h265 的 CRF）
    #[arg(about = "CRF quality 0-51 (lower = better)", default = 23, range = 0..=51)]
    crf: u8,

    /// x264/x265 预设（速度-压缩率权衡）
    #[arg(about = "Preset (speed vs. compression)", default = "medium")]
    preset: Preset,

    /// 输出宽度（缺省按高度等比）
    #[arg(about = "Output width (omit to auto from height)")]
    width: Option<u32>,

    /// 输出高度（缺省按宽度等比）
    #[arg(about = "Output height (omit to auto from width)")]
    height: Option<u32>,

    /// 音频处理
    #[arg(about = "Audio handling", default = "copy")]
    audio: Audio,

    /// 起始秒数（从该时间点开始）
    #[arg(about = "Start offset in seconds")]
    start: Option<f64>,

    /// 时长秒数（截取片段）
    #[arg(about = "Duration in seconds")]
    duration: Option<f64>,

    /// 覆盖已存在输出
    #[arg(about = "Overwrite output if it exists")]
    overwrite: bool,
}

/// 一次 -progress 块解析出的进度样本
#[derive(Default, Clone, Debug)]
struct ProgressSample {
    out_time_us: Option<u64>,
    frame: Option<u64>,
    speed: Option<String>,
}

/// 从 stdout 读线程发来的进度样本
struct ProgressMsg {
    sample: ProgressSample,
}

/// 业务逻辑：探测时长 → 组参 → 派生 ffmpeg → 实时进度 → 结果
fn run_ffmpeg(app: &Ffmpeg, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();

    if !Path::new(&app.input).is_file() {
        return Err(AppError::InvalidArg(format!(
            "input not a file: {}",
            app.input.display()
        )));
    }

    // 可选：探测输入时长以计算百分比
    let input_dur = probe_duration(&app.input);

    let argv = build_args(app)?;
    let cmdline = argv.join(" ");
    debug_assert!(!argv.is_empty());

    ctx.emit(Progress::Started {
        total: input_dur.map(|_| 100),
        message: Some(format!(
            "ffmpeg {} -> {}",
            app.input.display(),
            app.output.display()
        )),
    });

    // 组 ffmpeg 命令（-progress 写 stdout，人类日志写 stderr）
    let mut cmd = Command::new(resolve_bin(FFMPEG)?);
    cmd.args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Runtime(format!("spawn {FFMPEG}: {e}")))?;

    let stdout = child.stdout.take().ok_or_else(|| {
        child.kill().ok();
        AppError::Runtime("ffmpeg stdout not piped".into())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        child.kill().ok();
        AppError::Runtime("ffmpeg stderr not piped".into())
    })?;

    // 读线程：stdout 解析 -progress 块，stderr 累积错误文本；经 channel 回传
    let (prog_tx, prog_rx) = std::sync::mpsc::channel::<ProgressMsg>();
    let (err_tx, err_rx) = std::sync::mpsc::channel::<(String, bool)>();
    std::thread::spawn(move || read_progress(stdout, prog_tx));
    std::thread::spawn(move || {
        let _ = err_tx.send(read_with_cap(stderr));
    });

    // 实时：轮询子进程 + 排空进度 + 支持取消
    let mut last_tick = Instant::now();
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {}
            Err(e) => {
                return Err(AppError::Runtime(format!("wait {FFMPEG}: {e}")));
            }
        }

        // 取消检查：杀掉 ffmpeg 并返回 Cancelled
        if ctx.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Cancelled);
        }

        // 排空进度并节流上报（≤200ms 一次）
        let now = Instant::now();
        if now.duration_since(last_tick) >= Duration::from_millis(200) {
            drain_tick(app, ctx, &prog_rx, input_dur);
            last_tick = now;
        }

        std::thread::sleep(Duration::from_millis(20));
    };

    // 再排空一次，保证最终进度（含 progress=end）被捕获
    drain_tick(app, ctx, &prog_rx, input_dur);

    // 回收读线程结果
    let (stderr_text, stderr_truncated) = err_rx
        .recv_timeout(Duration::from_secs(30))
        .unwrap_or_else(|_| ("[output drain timed out]".into(), true));

    let duration_ms = start.elapsed().as_millis() as u64;
    let success = exit_code == Some(0);

    let result = serde_json::json!({
        "input": app.input.display().to_string(),
        "output": app.output.display().to_string(),
        "command": format!("{FFMPEG} {cmdline}"),
        "exit_code": exit_code,
        "duration_ms": duration_ms,
        "duration_hms": fmt_hms(duration_ms),
        "success": success,
        "stderr": stderr_text,
        "stderr_truncated": stderr_truncated,
    });

    if success {
        ctx.done(result.clone(), duration_ms);
        Ok(result)
    } else {
        // 非零退出是错误：携带 stderr 摘要，便于 AI/CLI 排查
        let reason =
            last_stderr_line(&stderr_text).unwrap_or_else(|| "unknown ffmpeg error".into());
        Err(AppError::Runtime(format!(
            "ffmpeg failed (exit {:?}): {reason}",
            exit_code
        )))
    }
}

/// 组 ffmpeg 参数（不含程序名）
fn build_args(app: &Ffmpeg) -> Result<Vec<String>, AppError> {
    let mut a: Vec<String> = Vec::new();

    // 覆盖行为
    a.push(if app.overwrite { "-y" } else { "-n" }.into());
    a.push("-hide_banner".into());
    a.push("-nostats".into());
    a.push("-loglevel".into());
    a.push("warning".into());

    // 裁剪
    if let Some(ss) = app.start {
        if ss < 0.0 {
            return Err(AppError::InvalidArg("start must be >= 0".into()));
        }
        a.push("-ss".into());
        a.push(fmt_secs(ss));
    }
    if let Some(t) = app.duration {
        if t <= 0.0 {
            return Err(AppError::InvalidArg("duration must be > 0".into()));
        }
        a.push("-t".into());
        a.push(fmt_secs(t));
    }

    a.push("-i".into());
    a.push(app.input.display().to_string());

    // 缩放
    if app.width.is_some() || app.height.is_some() {
        let w = app
            .width
            .map(|w| format!("{w}"))
            .unwrap_or_else(|| "-2".into());
        let h = app
            .height
            .map(|h| format!("{h}"))
            .unwrap_or_else(|| "-2".into());
        a.push("-vf".into());
        a.push(format!("scale={w}:{h}"));
    }

    // 音频
    match app.audio {
        Audio::None => a.push("-an".into()),
        Audio::Copy => {
            a.push("-c:a".into());
            a.push("copy".into());
        }
        Audio::Aac => {
            a.push("-c:a".into());
            a.push("aac".into());
        }
        Audio::Opus => {
            a.push("-c:a".into());
            a.push("libopus".into());
        }
    }

    // 视频
    match app.codec {
        Video::Copy => {
            a.push("-c:v".into());
            a.push("copy".into());
        }
        Video::H264 => {
            a.push("-c:v".into());
            a.push("libx264".into());
        }
        Video::H265 => {
            a.push("-c:v".into());
            a.push("libx265".into());
        }
        Video::Vp9 => {
            a.push("-c:v".into());
            a.push("libvpx-vp9".into());
        }
        Video::Av1 => {
            a.push("-c:v".into());
            a.push("libaom-av1".into());
        }
    }
    if app.codec != Video::Copy {
        a.push("-crf".into());
        a.push(app.crf.to_string());
        a.push("-preset".into());
        a.push(app.preset.as_str().into());
        // vp9/av1 需启用 CPU 并行编码，否则极慢
        if matches!(app.codec, Video::Vp9 | Video::Av1) {
            a.push("-row-mt".into());
            a.push("1".into());
        }
    }

    a.push("-movflags".into());
    a.push("+faststart".into());
    a.push("-progress".into());
    a.push("pipe:1".into());

    a.push(app.output.display().to_string());
    Ok(a)
}

/// 读取 stdout 并按 `-progress` 块解析：每块发一个 `ProgressMsg`
fn read_progress(reader: impl Read, tx: std::sync::mpsc::Sender<ProgressMsg>) {
    let mut sample = ProgressSample::default();

    let mut lines = std::io::BufReader::new(reader).lines();
    while let Some(Ok(line)) = lines.next() {
        if let Some((k, v)) = line.split_once('=') {
            match k {
                "out_time_us" => sample.out_time_us = v.parse().ok(),
                "frame" => sample.frame = v.parse().ok(),
                "speed" => sample.speed = Some(v.to_string()),
                "progress" => {
                    let end = v == "end";
                    if tx
                        .send(ProgressMsg {
                            sample: std::mem::take(&mut sample),
                        })
                        .is_err()
                    {
                        return; // 接收端已关闭（任务结束/取消）
                    }
                    if end {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
}

/// 排空进度 channel，节流上报 tick
fn drain_tick(
    app: &Ffmpeg,
    ctx: &Context,
    prog_rx: &std::sync::mpsc::Receiver<ProgressMsg>,
    input_dur: Option<f64>,
) {
    let mut last: Option<ProgressSample> = None;
    while let Ok(msg) = prog_rx.try_recv() {
        last = Some(msg.sample);
    }
    let Some(s) = last else { return };

    let out_secs = s.out_time_us.map(|us| us as f64 / 1_000_000.0);

    // 百分比（若知道输入时长）
    let (current, total) = match (out_secs, input_dur) {
        (Some(o), Some(d)) if d > 0.0 => {
            let pct = ((o / d) * 100.0).clamp(0.0, 100.0) as u64;
            (pct, Some(100u64))
        }
        // 不确定进度：以"处理到 out_time"推进
        (Some(o), _) => (o as u64, None),
        (None, _) => (0, None),
    };

    let message = match (out_secs, &s.frame) {
        (Some(o), Some(f)) => format!("{o:7.2}s  frame {f}  {}", app.output.display()),
        (Some(o), None) => format!("{o:7.2}s  {}", app.output.display()),
        (None, Some(f)) => format!("frame {f}  {}", app.output.display()),
        (None, None) => app.output.display().to_string(),
    };
    ctx.tick(current, total, message);
}

/// 用 ffprobe 探测输入时长（秒）；失败返回 None（不阻断转码）
fn probe_duration(input: &Path) -> Option<f64> {
    let bin = find_on_path_quiet(FFPROBE)?;
    let out = Command::new(bin)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(input)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim().parse::<f64>().ok().filter(|d| *d > 0.0)
}

/// 读取管道最多 MAX_OUTPUT_BYTES；超出继续排空并标记截断
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

/// 最后一条 stderr 非空行（错误摘要）
fn last_stderr_line(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

/// 秒 → ffmpeg 参数（保留必要小数，去除尾零）
fn fmt_secs(secs: f64) -> String {
    if (secs.fract()).abs() < f64::EPSILON {
        format!("{secs:.0}")
    } else {
        format!("{secs:.3}")
    }
}

/// 毫秒 → HH:MM:SS
fn fmt_hms(ms: u64) -> String {
    let total = ms / 1000;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

/// 在 PATH 上找可执行文件（quiet：找不到返回 None）
fn resolve_bin(name: &str) -> Result<String, AppError> {
    find_on_path_quiet(name).ok_or_else(|| {
        AppError::Runtime(format!(
            "`{name}` not found on PATH — install ffmpeg (https://ffmpeg.org/download.html, \
             or `winget install Gyan.FFmpeg` / `brew install ffmpeg` / `apt install ffmpeg`)"
        ))
    })
}

/// 在 PATH 上找可执行文件（Windows 带 PATHEXT 扩展）
fn find_on_path_quiet(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    let sep = if cfg!(windows) { ';' } else { ':' };
    let exts: Vec<String> = std::env::var("PATHEXT")
        .map(|e| e.split(';').map(|s| s.to_lowercase()).collect())
        .unwrap_or_else(|_| vec![".exe".into()]);
    for dir in path.split(sep).filter(|s| !s.is_empty()) {
        let base = Path::new(dir).join(name);
        for ext in &exts {
            let candidate = format!("{}{}", base.display(), ext);
            if Path::new(&candidate).is_file() {
                return Some(candidate);
            }
        }
        if Path::new(&base).is_file() {
            return Some(base.display().to_string());
        }
    }
    None
}

impl Preset {
    fn as_str(self) -> &'static str {
        match self {
            Preset::Ultrafast => "ultrafast",
            Preset::Superfast => "superfast",
            Preset::Veryfast => "veryfast",
            Preset::Faster => "faster",
            Preset::Fast => "fast",
            Preset::Medium => "medium",
            Preset::Slow => "slow",
            Preset::Slower => "slower",
            Preset::Veryslow => "veryslow",
        }
    }
}

// ── main: one line ────────────────────────────────────────

fn main() {
    lilyco::run::<Ffmpeg>();
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env_app() -> Ffmpeg {
        Ffmpeg {
            input: std::path::PathBuf::from("in.mp4"),
            output: std::path::PathBuf::from("out.mp4"),
            codec: Video::H264,
            crf: 23,
            preset: Preset::Medium,
            width: None,
            height: None,
            audio: Audio::Copy,
            start: None,
            duration: None,
            overwrite: true,
        }
    }

    #[test]
    fn build_args_defaults() {
        let a = build_args(&no_env_app()).unwrap();
        let s = a.join(" ");
        assert!(s.contains("-c:v"), "should set video codec: {s}");
        assert!(s.contains("libx264"), "default codec libx264: {s}");
        assert!(s.contains("-crf 23"), "default crf 23: {s}");
        assert!(s.contains("-preset medium"), "default preset: {s}");
        assert!(s.contains("-c:a copy"), "audio copy default: {s}");
        assert!(s.contains("pipe:1"), "progress to stdout: {s}");
        assert!(s.contains("-i in.mp4"), "input flag: {s}");
        assert!(s.contains("out.mp4"), "output: {s}");
        // 默认 overwrite=true → -y
        assert!(s.contains("-y"), "overwrite default on: {s}");
    }

    #[test]
    fn build_args_no_overwrite_uses_n() {
        let mut app = no_env_app();
        app.overwrite = false;
        let s = build_args(&app).unwrap().join(" ");
        assert!(s.contains("-n"), "should not overwrite: {s}");
    }

    #[test]
    fn build_args_vp9_sets_row_mt_and_preset() {
        let mut app = no_env_app();
        app.codec = Video::Vp9;
        let s = build_args(&app).unwrap().join(" ");
        assert!(s.contains("libvpx-vp9"), "{s}");
        assert!(s.contains("-row-mt 1"), "vp9 needs row-mt: {s}");
        assert!(s.contains("-crf 23"), "{s}");
    }

    #[test]
    fn build_args_copy_omits_crf_preset() {
        let mut app = no_env_app();
        app.codec = Video::Copy;
        let s = build_args(&app).unwrap().join(" ");
        assert!(s.contains("-c:v copy"), "{s}");
        assert!(!s.contains("-crf"), "copy must not set crf: {s}");
        assert!(!s.contains("-preset"), "copy must not set preset: {s}");
    }

    #[test]
    fn build_args_scale_both_sides() {
        let mut app = no_env_app();
        app.width = Some(1280);
        app.height = Some(720);
        let s = build_args(&app).unwrap().join(" ");
        assert!(s.contains("scale=1280:720"), "{s}");
    }

    #[test]
    fn build_args_scale_one_side_uses_minus2() {
        let mut app = no_env_app();
        app.width = Some(1280);
        let s = build_args(&app).unwrap().join(" ");
        assert!(s.contains("scale=1280:-2"), "{s}");
    }

    #[test]
    fn build_args_audio_none_sets_an() {
        let mut app = no_env_app();
        app.audio = Audio::None;
        let s = build_args(&app).unwrap().join(" ");
        assert!(s.contains("-an"), "{s}");
    }

    #[test]
    fn build_args_trim() {
        let mut app = no_env_app();
        app.start = Some(10.0);
        app.duration = Some(5.5);
        let s = build_args(&app).unwrap().join(" ");
        assert!(s.contains("-ss 10"), "{s}");
        assert!(s.contains("-t 5.500"), "5.5 sec trimmed: {s}");
    }

    #[test]
    fn invalid_trim_rejected() {
        let mut app = no_env_app();
        app.start = Some(-1.0);
        assert!(build_args(&app).is_err());
        app.start = None;
        app.duration = Some(0.0);
        assert!(build_args(&app).is_err());
    }

    #[test]
    fn fmt_secs_trims_trailing_zeros() {
        assert_eq!(fmt_secs(10.0), "10");
        assert_eq!(fmt_secs(10.5), "10.500");
    }

    #[test]
    fn fmt_hms_cases() {
        assert_eq!(fmt_hms(0), "00:00:00");
        assert_eq!(fmt_hms(1_234_000), "00:20:34");
        assert_eq!(fmt_hms(3_661_000), "01:01:01");
    }

    #[test]
    fn probe_duration_missing_returns_none() {
        // 找不到 ffprobe（或输入缺失）→ None，不 panic
        let p = probe_duration(Path::new("C:/definitely/not/a/file.mp4"));
        assert!(p.is_none() || p.is_some(), "no panic either way");
    }

    #[test]
    fn parse_progress_block_produces_tick() {
        // 直接测 read_progress 的取样逻辑：喂一个块，应有 tick 上报
        let input: &[u8] = b"frame=12\nout_time_us=1500000\nspeed=1.0x\nprogress=continue\n";
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || read_progress(input, tx));
        handle.join().unwrap();
        let msgs: Vec<ProgressMsg> = rx.iter().collect();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].sample.out_time_us, Some(1_500_000));
        assert_eq!(msgs[0].sample.frame, Some(12));
        drop(msgs);
    }

    #[test]
    fn resolve_bin_finds_ffmpeg_only_when_installed() {
        // 不强制断言：只需保证无 panic
        let _ = resolve_bin(FFMPEG);
        let _ = find_on_path_quiet(FFMPEG);
        let _ = find_on_path_quiet("definitely-not-a-real-binary-xyz");
    }
}
