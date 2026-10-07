//! lsrt — 自动给视频打字幕：faster-whisper 转录 + ffmpeg 烧字幕。
//!
//! ```bash
//! lsrt --input 1.mp4                          # 默认硬字幕
//! lsrt --input 1.mp4 --output out.mp4         # 指定输出
//! lsrt --input 1.mp4 --soft                   # 软字幕（封装不烧画面）
//! lsrt --input 1.mp4 --model medium --language zh
//! lsrt --input 1.mp4 --font "Microsoft YaHei" --fontsize 42
//! lsrt --mcp                                  # MCP stdio 服务器
//! ```
//!
//! 依赖：系统 ffmpeg 在 PATH；Python faster-whisper 已装（`pip install faster-whisper`）。
//! 配置文件 `~/.lyco/lyco_srt/config.toml`（可选，CLI 参数覆盖配置）。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use lilyco::prelude::*;

/// 字幕模式
#[derive(ValueEnum, Clone, Copy, PartialEq, Debug)]
enum SubMode {
    /// 硬字幕：烧进画面
    Hard,
    /// 软字幕：封装为字幕流（mkv/mp4）
    Soft,
}

/// 自动视频字幕：faster-whisper 转录 + ffmpeg 烧字幕
#[derive(App)]
#[app(
    run = "run_srt",
    about = "Auto-subtitle a video: extract audio -> faster-whisper ASR -> write SRT -> burn/embed subtitles via ffmpeg. Parameters: `input` (video file, must exist), `output` (auto .subbed.mp4 if omitted), `mode` (hard|soft, default hard), `model` (tiny|base|small|medium|large-v3, default small), `language` (zh|en|auto, default zh), `font`, `fontsize`, `overwrite`. Requires ffmpeg on PATH and Python faster-whisper installed. Returns { input, output, srt, segments, mode, duration_ms, success }."
)]
struct Srt {
    /// 输入视频文件（须已存在）
    #[arg(about = "Input video file (must exist)", must_exist = true)]
    input: PathBuf,

    /// 输出视频（缺省 = 输入名 + .subbed.mp4）
    #[arg(about = "Output video path (auto if omitted)")]
    output: Option<PathBuf>,

    /// 字幕模式：hard=烧进画面, soft=封装字幕流
    #[arg(about = "Subtitle mode: hard (burn-in) or soft (mux)", default = "hard")]
    mode: SubMode,

    /// faster-whisper 模型大小
    #[arg(about = "Whisper model: tiny/base/small/medium/large-v3", default = "small")]
    model: String,

    /// 语言
    #[arg(about = "Language: zh/en/auto", default = "zh")]
    language: String,

    /// 字幕字体
    #[arg(about = "Subtitle font name", default = "Microsoft YaHei")]
    font: String,

    /// 字幕字号
    #[arg(about = "Subtitle font size", default = 42)]
    fontsize: u32,

    /// 覆盖已存在输出
    #[arg(about = "Overwrite output if exists")]
    overwrite: bool,
}

fn run_srt(app: &Srt, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();

    if !app.input.is_file() {
        return Err(AppError::InvalidArg(format!(
            "input not a file: {}",
            app.input.display()
        )));
    }

    let output = app.output.clone().unwrap_or_else(|| {
        app.input.with_file_name(format!(
            "{}.subbed.mp4",
            app.input.file_stem().unwrap_or_default().to_string_lossy()
        ))
    });
    let srt_path = app.input.with_extension("srt");
    let ass_path = app.input.with_extension("ass");

    ctx.emit(Progress::Started {
        total: Some(4),
        message: Some(format!("lsrt {}", app.input.display())),
    });

    // 1. 提取音频
    ctx.tick(1, Some(4), "提取音频...");
    let audio_tmp = std::env::temp_dir().join(format!("lsrt_{}.wav", std::process::id()));
    run_ffmpeg(&[
        "-y", "-i", &path_str(&app.input),
        "-vn", "-ac", "1", "-ar", "16000",
        &path_str(&audio_tmp),
    ])?;

    // 2. faster-whisper 转录
    ctx.tick(2, Some(4), &format!("whisper {} ({})...", app.model, app.language));
    let n = run_whisper(&audio_tmp, &srt_path, &app.model, &app.language, ctx)?;

    // 3. 硬字幕: SRT -> ASS + 烧画面; 软字幕: 直接封装
    ctx.tick(3, Some(4), match app.mode {
        SubMode::Hard => "烧硬字幕...",
        SubMode::Soft => "封装软字幕...",
    });

    match app.mode {
        SubMode::Hard => {
            srt_to_ass(&srt_path, &ass_path, &app.font, app.fontsize)?;
            let mut vf = format!(
                "subtitles={}:force_style='FontName={},FontSize={},PrimaryColour=&HFFFFFF&,OutlineColour=&H000000&,BorderStyle=1,Outline=2,Shadow=0,Alignment=2,MarginV=60'",
                ass_path.display(),
                app.font,
                app.fontsize,
            );
            // Windows 路径反斜杠转义
            vf = vf.replace('\\', "/");
            run_ffmpeg(&[
                if app.overwrite { "-y" } else { "-n" },
                "-i", &path_str(&app.input),
                "-vf", &vf,
                "-c:v", "libx264", "-preset", "veryfast", "-crf", "20",
                "-c:a", "copy", "-movflags", "+faststart",
                &path_str(&output),
            ])?;
        }
        SubMode::Soft => {
            run_ffmpeg(&[
                if app.overwrite { "-y" } else { "-n" },
                "-i", &path_str(&app.input),
                "-i", &path_str(&srt_path),
                "-c", "copy", "-c:s", "mov_text",
                "-movflags", "+faststart",
                &path_str(&output),
            ])?;
        }
    }

    let _ = std::fs::remove_file(&audio_tmp);

    ctx.tick(4, Some(4), "完成");
    let duration_ms = start.elapsed().as_millis() as u64;
    let result = serde_json::json!({
        "input": app.input.display().to_string(),
        "output": output.display().to_string(),
        "srt": srt_path.display().to_string(),
        "segments": n,
        "mode": match app.mode { SubMode::Hard => "hard", SubMode::Soft => "soft" },
        "model": app.model,
        "language": app.language,
        "duration_ms": duration_ms,
        "success": true,
    });
    ctx.done(result.clone(), duration_ms);
    Ok(result)
}

fn run_ffmpeg(args: &[String]) -> Result<(), AppError> {
    let out = Command::new("ffmpeg")
        .args(args)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| AppError::Runtime(format!("spawn ffmpeg: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let last = stderr.lines().rev().find(|l| !l.is_empty()).unwrap_or("unknown");
        return Err(AppError::Runtime(format!("ffmpeg: {last}")));
    }
    Ok(())
}

fn run_whisper(
    audio: &Path,
    srt_out: &Path,
    model: &str,
    lang: &str,
    ctx: &Context,
) -> Result<usize, AppError> {
    // 调 Python faster-whisper，输出 SRT
    let script = format!(r#"
import sys
from faster_whisper import WhisperModel
m = WhisperModel("{model}", device="cpu", compute_type="int8")
segs, _ = m.transcribe(r"{audio}", language="{lang}", vad_filter=True)
def ts(s):
    h=int(s//3600); mi=int((s%3600)//60); se=s%60
    return f"{{h:02d}}:{{mi:02d}}:{{se:06.3f}}".replace(".", ",")
n=0
with open(r"{srt}", "w", encoding="utf-8") as f:
    for i, seg in enumerate(segs, 1):
        f.write(f"{{i}}\n{{ts(seg.start)}} --> {{ts(seg.end)}}\n{{seg.text.strip()}}\n\n")
        n+=1
        print(f"{{ts(seg.start)}} {{seg.text.strip()}}", flush=True)
print(f"DONE:{{n}}")
"#,
        model = model,
        audio = audio.display(),
        lang = lang,
        srt = srt_out.display(),
    );
    let out = Command::new("python")
        .args(["-c", &script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| AppError::Runtime(format!("spawn python: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(AppError::Runtime(format!("whisper: {stderr}")));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let n = stdout
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix("DONE:")?.parse::<usize>().ok())
        .unwrap_or(0);
    // 打印转录进度到 ctx log
    for line in stdout.lines() {
        if !line.starts_with("DONE:") {
            ctx.log(LogLevel::Info, line.to_string());
        }
    }
    Ok(n)
}

fn srt_to_ass(srt: &Path, ass: &Path, font: &str, fontsize: u32) -> Result<(), AppError> {
    let header = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\n\n\
         [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Default,{font},{fontsize},&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2.5,0,2,40,40,60,1\n\n\
         [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
    );
    let mut body = String::new();
    let content = std::fs::read_to_string(srt).map_err(|e| AppError::Runtime(format!("read srt: {e}")))?;
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.is_empty() || line.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if let Some((st, et)) = line.split_once(" --> ") {
            let conv = |t: &str| -> String {
                let t = t.replace(',', ".");
                if let Ok(secs) = t.parse::<f64>() {
                    let h = (secs as u64) / 3600;
                    let m = ((secs as u64) % 3600) / 60;
                    let s = secs % 60.0;
                    format!("{h}:{m:02d}:{s:05.2f}")
                } else {
                    "0:00:00.00".into()
                }
            };
            let txt = lines.next().unwrap_or("").trim();
            body += &format!("Dialogue: 0,{},{},Default,,0,0,0,,{}\n", conv(st), conv(et), txt);
        }
    }
    std::fs::write(ass, header + &body).map_err(|e| AppError::Runtime(format!("write ass: {e}")))?;
    Ok(())
}

fn path_str(p: &Path) -> String {
    p.display().to_string()
}

fn main() {
    lilyco::run::<Srt>();
}
