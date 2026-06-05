//! imgpress — compress images from the command line.
//!
//! ```bash
//! imgpress --input photo.jpg --quality 50 --format webp
//! imgpress --input big.png --width 800
//! imgpress --schema
//! imgpress --anthropic-tool
//! ```

use std::path::PathBuf;
use std::time::Instant;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ExtendedColorType, GenericImageView};
use image::imageops::FilterType as ResizeFilter;

use lilyco_core::prelude::*;
use lilyco_macros::{App, ValueEnum};

// ── Types: convention over configuration ───────────────────

#[derive(Debug, ValueEnum)]
enum Format {
    Jpeg,
    Png,
    Webp,
}

/// Compress and resize image files
#[derive(App)]
struct ImgCompress {
    /// Input image file
    #[arg(must_exist = true)]
    input: PathBuf,

    /// Output path (auto-generated if omitted)
    output: Option<String>,

    /// Output format
    #[arg(default = "jpeg")]
    format: Format,

    /// Quality 1–100 (JPEG only)
    #[arg(default = 75, range = 1..=100)]
    quality: u8,

    /// Max width in pixels (0 = no resize)
    #[arg(default = 0)]
    width: u32,

    /// Max height in pixels (0 = no resize)
    #[arg(default = 0)]
    height: u32,

    /// Preview only — don't write output
    dry_run: bool,
}

// ── Stats ─────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct Stats {
    input_size: u64,
    output_size: u64,
    compression_ratio: f64,
    input_dimensions: String,
    output_dimensions: String,
    quality: u8,
    dry_run: bool,
}

// ── Business logic ────────────────────────────────────────

fn run_compress(app: &ImgCompress, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();

    ctx.emit(Progress::Started { total: Some(5), message: None });
    ctx.tick(1, Some(5), "Reading input");

    let input_data = std::fs::read(&app.input)?;
    let input_size = input_data.len() as u64;
    let img = image::load_from_memory(&input_data)
        .map_err(|e| AppError::Runtime(format!("decode: {e}")))?;
    let (in_w, in_h) = img.dimensions();

    ctx.tick(2, Some(5), format!("{in_w}×{in_h}"));

    let img = resize(img, app.width, app.height);
    let (out_w, out_h) = img.dimensions();

    let output_path = match &app.output {
        Some(p) => PathBuf::from(p),
        None => app.input.with_file_name(format!(
            "compressed.{}",
            match app.format {
                Format::Jpeg => "jpg",
                Format::Png => "png",
                Format::Webp => "webp",
            }
        )),
    };

    if app.dry_run {
        ctx.log(LogLevel::Info, format!("Dry run: {} → {}", app.input.display(), output_path.display()));
        ctx.log(LogLevel::Info, format!("{in_w}×{in_h} → {out_w}×{out_h}"));
        let s = Stats { input_size, output_size: 0, compression_ratio: 0.0,
            input_dimensions: format!("{in_w}×{in_h}"),
            output_dimensions: format!("{out_w}×{out_h}"),
            quality: app.quality, dry_run: true };
        let r = serde_json::to_value(&s).map_err(AppError::Serialize)?;
        ctx.done(r.clone(), start.elapsed().as_millis() as u64);
        return Ok(r);
    }

    ctx.tick(3, Some(5), "Encoding...");
    let compressed = encode(&img, &app.format, app.quality)?;
    let output_size = compressed.len() as u64;
    std::fs::write(&output_path, &compressed)?;

    ctx.tick(5, Some(5), "Done");

    let ratio = if input_size > 0 { output_size as f64 / input_size as f64 * 100.0 } else { 0.0 };
    let size_change = if output_size < input_size { format!("−{:.1}%", 100.0 - ratio) }
                      else { format!("+{:.1}%", ratio - 100.0) };

    ctx.log(LogLevel::Info, format!(
        "{in_w}×{in_h} → {out_w}×{out_h}  |  {size_change}  |  {}ms",
        start.elapsed().as_millis()
    ));

    let s = Stats { input_size, output_size, compression_ratio: (ratio * 10.0).round() / 10.0,
        input_dimensions: format!("{in_w}×{in_h}"),
        output_dimensions: format!("{out_w}×{out_h}"),
        quality: app.quality, dry_run: false };
    let r = serde_json::to_value(&s).map_err(AppError::Serialize)?;
    ctx.done(r.clone(), start.elapsed().as_millis() as u64);
    Ok(r)
}

// ── Helpers ───────────────────────────────────────────────

fn resize(img: DynamicImage, max_w: u32, max_h: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    let tw = if max_w > 0 { max_w } else { w };
    let th = if max_h > 0 { max_h } else { h };
    if tw >= w && th >= h { return img; }
    let ratio = (tw as f64 / w as f64).min(th as f64 / h as f64);
    img.resize_exact((w as f64 * ratio).max(1.0) as u32, (h as f64 * ratio).max(1.0) as u32, ResizeFilter::Lanczos3)
}

fn encode(img: &DynamicImage, fmt: &Format, quality: u8) -> Result<Vec<u8>, AppError> {
    let mut buf = Vec::new();
    match fmt {
        Format::Jpeg => {
            let rgb = img.to_rgb8();
            JpegEncoder::new_with_quality(&mut buf, quality)
                .encode(&rgb, rgb.width(), rgb.height(), ExtendedColorType::Rgb8)
                .map_err(|e| AppError::Runtime(format!("JPEG: {e}")))?;
        }
        Format::Png => {
            img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
                .map_err(|e| AppError::Runtime(format!("PNG: {e}")))?;
        }
        Format::Webp => {
            let rgba = img.to_rgba8();
            WebPEncoder::new_lossless(&mut buf)
                .encode(&rgba, rgba.width(), rgba.height(), ExtendedColorType::Rgba8)
                .map_err(|e| AppError::Runtime(format!("WebP: {e}")))?;
        }
    }
    Ok(buf)
}

// ── main: one line ────────────────────────────────────────

fn main() {
    let schema = ImgCompress::schema();
    let cmd = lilyco_cli::CliRenderer::new().render(&schema);
    let matches = cmd.get_matches();

    // Built-in flags (--schema, --anthropic-tool, --openai-tool)
    if lilyco_cli::CliRenderer::handle_builtin_flags(&schema, &matches) {
        return;
    }

    // --gui: launch web interface
    if matches.get_flag("gui") {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let gui = lilyco_gui::GuiRenderer::new(8080);
            let schema_clone = schema.clone();
            gui.serve(schema_clone, std::sync::Arc::new(move |args, gui_tx| {
                let app = ImgCompress::from_args(&args).unwrap();
                Box::pin(async move {
                    let (std_tx, rx) = std::sync::mpsc::channel();
                    let ctx = Context::new_test(std_tx);
                    let handle = std::thread::spawn(move || run_compress(&app, &ctx));

                    // Forward progress events from std channel → GUI's tokio channel
                    for event in rx {
                        let json = serde_json::to_value(&event).unwrap();
                        if gui_tx.send(json).await.is_err() {
                            break;
                        }
                        if matches!(event, Progress::Done { .. } | Progress::Error { .. }) {
                            break;
                        }
                    }
                    let _ = handle.join();
                })
            })).await;
        });
        return;
    }

    // Default: CLI
    let output_format = lilyco_cli::CliRenderer::output_format(&matches);
    let args = lilyco_cli::CliRenderer::extract_args(&schema, &matches);
    let app = ImgCompress::from_args(&args).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ctx = Context::new(tx, cancel.clone(), output_format.clone());
    let handle = std::thread::spawn(move || run_compress(&app, &ctx));

    match output_format {
        OutputFormat::JsonStream => {
            for event in rx {
                println!("{}", serde_json::to_string(&event).unwrap());
                if matches!(event, Progress::Done { .. } | Progress::Error { .. }) { break; }
            }
        }
        _ => {
            for event in rx {
                match &event {
                    Progress::Tick { message, percent, .. } => {
                        if let Some(msg) = message {
                            let pct = percent.map(|p| format!("{:3.0}%", p * 100.0)).unwrap_or_default();
                            eprintln!("\r  {pct}  {msg}");
                        }
                    }
                    Progress::Log { level, message } => eprintln!("  [{level:?}] {message}"),
                    Progress::Done { result, duration_ms } => {
                        if let Ok(stats) = serde_json::from_value::<serde_json::Value>(result.clone()) {
                            if stats != serde_json::json!(null) {
                                println!("{}", serde_json::to_string_pretty(&stats).unwrap());
                            }
                        }
                        eprintln!("\n  Done in {duration_ms}ms");
                        break;
                    }
                    Progress::Error { message, .. } => { eprintln!("\n  Error: {message}"); break; }
                    _ => {}
                }
            }
            eprintln!();
        }
    }

    if let Err(e) = handle.join() { eprintln!("Error: {e:?}"); }
}
