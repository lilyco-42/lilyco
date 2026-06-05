//! imgpress — image compression tool built with Lilyco.
//!
//! ```bash
//! imgpress --input photo.jpg --quality 50 --format webp
//! imgpress --input big.png --width 800 --dry-run
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

// ── Types ─────────────────────────────────────────────────

#[derive(Debug, ValueEnum)]
enum Format {
    Jpeg,
    Png,
    Webp,
}

#[derive(App)]
#[app(about = "Compress and resize image files")]
struct ImgCompress {
    #[arg(about = "Input image file", must_exist = true)]
    input: PathBuf,

    #[arg(about = "Output image file (auto-generated if omitted)")]
    output: Option<String>,

    #[arg(about = "Output format", default = "jpeg")]
    format: Format,

    #[arg(about = "Quality (1-100, JPEG only)", default = 75, range = 1..=100)]
    quality: u8,

    #[arg(about = "Max width in pixels (0 = no resize)")]
    width: u32,

    #[arg(about = "Max height in pixels (0 = no resize)")]
    height: u32,

    #[arg(about = "Preview only — show what would happen")]
    dry_run: bool,
}

// ── Stats ─────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct Stats {
    input_size: u64,
    output_size: u64,
    compression_ratio: f64,
    input_dimensions: String,
    output_dimensions: String,
    format: String,
    quality: u8,
    dry_run: bool,
}

// ── Core logic ────────────────────────────────────────────

fn run_compress(app: &ImgCompress, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let format_name = format!("{:?}", app.format).to_lowercase();

    // Step 1: load
    ctx.emit(Progress::Started {
        total: Some(5),
        message: Some(format!("Loading {}...", app.input.display())),
    });
    ctx.tick(1, Some(5), "Reading input file");

    let input_data = std::fs::read(&app.input)?;
    let input_size = input_data.len() as u64;
    let img = image::load_from_memory(&input_data)
        .map_err(|e| AppError::Runtime(format!("cannot decode image: {e}")))?;
    let (in_w, in_h) = img.dimensions();

    // Step 2: resize
    ctx.tick(2, Some(5), format!("Original: {in_w}×{in_h}"));
    let img = resize_image(img, app.width, app.height);
    let (out_w, out_h) = img.dimensions();

    // Step 3: output path
    ctx.tick(3, Some(5), "Encoding...");
    let output_path = match &app.output {
        Some(p) => PathBuf::from(p),
        None => {
            let stem = app.input.file_stem().unwrap_or_default().to_string_lossy();
            let ext = match format_name.as_str() {
                "jpeg" => "jpg",
                "webp" => "webp",
                _ => "png",
            };
            PathBuf::from(format!("{stem}_compressed.{ext}"))
        }
    };

    // Step 4: encode (or dry run)
    if app.dry_run {
        ctx.tick(5, Some(5), "Dry run");
        let stats = build_stats(app, input_size, 0, in_w, in_h, out_w, out_h, true);
        let result = serde_json::to_value(&stats).map_err(AppError::Serialize)?;
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }

    ctx.tick(4, Some(5), format!("Writing {}", output_path.display()));
    let compressed = encode_image(&img, &app.format, app.quality)?;
    let output_size = compressed.len() as u64;
    std::fs::write(&output_path, &compressed)?;

    // Step 5: done
    ctx.tick(5, Some(5), "Complete");
    let ratio = if input_size > 0 {
        (output_size as f64 / input_size as f64 * 100.0 * 10.0).round() / 10.0
    } else {
        0.0
    };
    let size_change = if output_size < input_size {
        format!("-{:.1}%", 100.0 - ratio)
    } else {
        format!("+{:.1}%", ratio - 100.0)
    };
    ctx.log(LogLevel::Info, format!(
        "{} → {}  |  {in_w}×{in_h} → {out_w}×{out_h}  |  {size_change}  |  {}ms",
        app.input.file_name().unwrap_or_default().to_string_lossy(),
        output_path.file_name().unwrap_or_default().to_string_lossy(),
        start.elapsed().as_millis(),
    ));

    let stats = build_stats(app, input_size, output_size, in_w, in_h, out_w, out_h, false);
    let result = serde_json::to_value(&stats).map_err(AppError::Serialize)?;
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

fn build_stats(
    app: &ImgCompress, input_size: u64, output_size: u64,
    in_w: u32, in_h: u32, out_w: u32, out_h: u32, dry_run: bool,
) -> Stats {
    let ratio = if input_size > 0 {
        (output_size as f64 / input_size as f64 * 100.0 * 10.0).round() / 10.0
    } else { 0.0 };
    Stats {
        input_size,
        output_size,
        compression_ratio: ratio,
        input_dimensions: format!("{in_w}×{in_h}"),
        output_dimensions: format!("{out_w}×{out_h}"),
        format: format!("{:?}", app.format).to_lowercase(),
        quality: app.quality,
        dry_run,
    }
}

// ── Image helpers ─────────────────────────────────────────

fn resize_image(img: DynamicImage, max_w: u32, max_h: u32) -> DynamicImage {
    let (w, h) = img.dimensions();

    let target_w = if max_w > 0 { max_w } else { w };
    let target_h = if max_h > 0 { max_h } else { h };

    if target_w >= w && target_h >= h {
        return img;
    }

    let ratio = (target_w as f64 / w as f64).min(target_h as f64 / h as f64);
    let new_w = (w as f64 * ratio).max(1.0) as u32;
    let new_h = (h as f64 * ratio).max(1.0) as u32;

    img.resize_exact(new_w, new_h, ResizeFilter::Lanczos3)
}

fn encode_image(img: &DynamicImage, format: &Format, quality: u8) -> Result<Vec<u8>, AppError> {
    let mut buf = Vec::new();

    match format {
        Format::Jpeg => {
            let mut encoder = JpegEncoder::new_with_quality(&mut buf, quality);
            let rgb = img.to_rgb8();
            encoder
                .encode(&rgb, rgb.width(), rgb.height(), ExtendedColorType::Rgb8)
                .map_err(|e| AppError::Runtime(format!("JPEG: {e}")))?;
        }
        Format::Png => {
            // PNG: use image's built-in encoder (no quality param — use write_to)
            let mut cursor = std::io::Cursor::new(&mut buf);
            img.write_to(&mut cursor, image::ImageFormat::Png)
                .map_err(|e| AppError::Runtime(format!("PNG: {e}")))?;
        }
        Format::Webp => {
            let rgba = img.to_rgba8();
            let encoder = WebPEncoder::new_lossless(&mut buf);
            encoder
                .encode(&rgba, rgba.width(), rgba.height(), ExtendedColorType::Rgba8)
                .map_err(|e| AppError::Runtime(format!("WebP: {e}")))?;
        }
    }

    Ok(buf)
}

// ── main ──────────────────────────────────────────────────

fn main() {
    let schema = ImgCompress::schema();
    let renderer = lilyco_cli::CliRenderer::new();
    let cmd = renderer.render(&schema);

    let matches = cmd.get_matches();

    // Handle --schema / --anthropic-tool / --openai-tool
    if lilyco_cli::CliRenderer::handle_builtin_flags(&schema, &matches) {
        return;
    }

    let output_format = lilyco_cli::CliRenderer::output_format(&matches);
    let args = lilyco_cli::CliRenderer::extract_args(&schema, &matches);

    let app = match ImgCompress::from_args(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ctx = Context::new(tx, cancel.clone(), output_format.clone());

    let handle = std::thread::spawn(move || run_compress(&app, &ctx));

    match output_format {
        OutputFormat::JsonStream => {
            for event in rx {
                let line = serde_json::to_string(&event).unwrap();
                println!("{line}");
                if matches!(event, Progress::Done { .. } | Progress::Error { .. }) {
                    break;
                }
            }
        }
        _ => {
            for event in rx {
                match &event {
                    Progress::Tick { message, percent, .. } => {
                        if let Some(msg) = message {
                            let pct = percent
                                .map(|p| format!("{:3.0}%", p * 100.0))
                                .unwrap_or_default();
                            eprintln!("\r  {pct}  {msg}");
                        }
                    }
                    Progress::Log { level, message } => {
                        eprintln!("  [{level:?}] {message}");
                    }
                    Progress::Done { result, duration_ms } => {
                        if let Ok(stats) = serde_json::from_value::<Stats>(result.clone()) {
                            println!("{}", serde_json::to_string_pretty(&stats).unwrap());
                        }
                        eprintln!("\n  Done in {duration_ms}ms");
                        break;
                    }
                    Progress::Error { message, .. } => {
                        eprintln!("\n  Error: {message}");
                        break;
                    }
                    _ => {}
                }
            }
            eprintln!();
        }
    }

    if let Err(e) = handle.join() {
        eprintln!("Error: {e:?}");
        std::process::exit(1);
    }
}
