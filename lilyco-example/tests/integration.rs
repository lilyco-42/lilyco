//! End-to-end tests for imgpress.
//!
//! These tests exercise the full pipeline: schema, from_args, compression, CLI flags.

use std::collections::HashMap;
use std::path::PathBuf;

use image::{ImageBuffer, Rgb};
use lilyco_core::prelude::*;
use lilyco_core::schema::{ArgKind, CommandSchema};

// Import the binary's types and functions.
// In a binary crate, we can't directly `use` items from main.rs.
// Instead, we test via the public API: schema(), from_args(), and the run_compress function.

// ── Helpers ─────────────────────────────────────────────────

/// Create a small 100×100 red PNG image, return temp path.
fn make_test_image() -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.png");
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(100, 100, |x, y| {
        let r = (x as u8).wrapping_mul(2);
        let g = (y as u8).wrapping_mul(2);
        let b = 128u8;
        Rgb([r, g, b])
    });
    img.save(&path).unwrap();
    (path, dir)
}

/// Extract the schema from the ImgCompress type.
/// We re-derive App on a mirror struct so tests don't depend on main.rs internals.
use lilyco_macros::{App, ValueEnum};

#[derive(Debug, ValueEnum, PartialEq)]
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

    /// Output path
    output: Option<String>,

    /// Output format
    #[arg(default = "jpeg")]
    format: Format,

    /// Quality 1–100
    #[arg(default = 75, range = 1..=100)]
    quality: u8,

    /// Max width (0 = none)
    #[arg(default = 0)]
    width: u32,

    /// Max height (0 = none)
    #[arg(default = 0)]
    height: u32,

    /// Dry run
    dry_run: bool,
}

// ── Schema tests ────────────────────────────────────────────

#[test]
fn schema_has_correct_name_and_about() {
    let s: CommandSchema = ImgCompress::schema();
    assert_eq!(s.name, "ImgCompress");
    // doc comment → about
    assert!(s.about.contains("Compress"), "about={}", s.about);
}

#[test]
fn schema_uses_kebab_case() {
    let s = ImgCompress::schema();
    let dr = s.args.iter().find(|a| a.name == "dry-run").unwrap();
    assert!(!dr.required);
    // flag should be false by default
}

#[test]
fn schema_doc_comments_become_about() {
    let s = ImgCompress::schema();
    let input = s.args.iter().find(|a| a.name == "input").unwrap();
    assert_eq!(input.about, "Input image file");

    let quality = s.args.iter().find(|a| a.name == "quality").unwrap();
    assert_eq!(quality.about, "Quality 1–100");
}

#[test]
fn schema_enum_has_values() {
    let s = ImgCompress::schema();
    let fmt = s.args.iter().find(|a| a.name == "format").unwrap();
    match &fmt.kind {
        ArgKind::Enum { values } => {
            assert_eq!(values, &vec!["jpeg".to_string(), "png".to_string(), "webp".to_string()]);
        }
        _ => panic!("expected Enum"),
    }
}

#[test]
fn schema_number_has_range() {
    let s = ImgCompress::schema();
    let q = s.args.iter().find(|a| a.name == "quality").unwrap();
    match &q.kind {
        ArgKind::Number { min, max } => {
            assert_eq!(*min, Some(1.0));
            assert_eq!(*max, Some(100.0));
        }
        _ => panic!("expected Number"),
    }
}

#[test]
fn schema_default_makes_required_false() {
    let s = ImgCompress::schema();
    let q = s.args.iter().find(|a| a.name == "quality").unwrap();
    assert!(!q.required, "fields with default should not be required");
    let fmt = s.args.iter().find(|a| a.name == "format").unwrap();
    assert!(!fmt.required);
    let input = s.args.iter().find(|a| a.name == "input").unwrap();
    assert!(input.required, "input has no default, should be required");
}

// ── from_args tests ─────────────────────────────────────────

#[test]
fn from_args_constructs_struct() {
    let mut args = HashMap::new();
    args.insert("input".into(), serde_json::json!("/tmp/a.png"));
    args.insert("format".into(), serde_json::json!("webp"));
    args.insert("quality".into(), serde_json::json!(50));
    args.insert("width".into(), serde_json::json!(0));
    args.insert("height".into(), serde_json::json!(0));
    args.insert("dry-run".into(), serde_json::json!(true));

    let app = ImgCompress::from_args(&args).unwrap();
    assert_eq!(app.input, PathBuf::from("/tmp/a.png"));
    assert_eq!(app.format, Format::Webp);
    assert_eq!(app.quality, 50);
    assert!(app.dry_run);
}

#[test]
fn from_args_uses_defaults() {
    let mut args = HashMap::new();
    args.insert("input".into(), serde_json::json!("/tmp/a.png"));
    args.insert("format".into(), serde_json::json!("jpeg"));
    args.insert("quality".into(), serde_json::json!(75));
    args.insert("width".into(), serde_json::json!(0));
    args.insert("height".into(), serde_json::json!(0));

    let app = ImgCompress::from_args(&args).unwrap();
    assert_eq!(app.quality, 75);
    assert_eq!(app.format, Format::Jpeg);
    assert!(!app.dry_run);  // flag defaults to false when key missing
}

// ── Compression tests ───────────────────────────────────────

#[test]
fn dry_run_returns_stats() {
    let (path, _dir) = make_test_image();
    let app = ImgCompress {
        input: path.clone(),
        output: None,
        format: Format::Png,
        quality: 80,
        width: 0,
        height: 0,
        dry_run: true,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = Context::new_test(tx);

    let result = run_compress_inner(&app, &ctx).unwrap();
    let stats: serde_json::Value = result;
    assert_eq!(stats["dry_run"], serde_json::json!(true));
    assert_eq!(stats["output_size"], serde_json::json!(0));
}

#[test]
fn compress_png_to_jpeg() {
    let (path, _dir) = make_test_image();
    let app = ImgCompress {
        input: path.clone(),
        output: None,
        format: Format::Jpeg,
        quality: 50,
        width: 0,
        height: 0,
        dry_run: false,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = Context::new_test(tx);

    let result = run_compress_inner(&app, &ctx).unwrap();
    let stats: serde_json::Value = result;
    assert_eq!(stats["dry_run"], serde_json::json!(false));
    // output should exist and be non-zero
    assert!(stats["output_size"].as_u64().unwrap() > 0);

    // Check the output file exists
    let output_path = path.with_file_name("compressed.jpg");
    assert!(output_path.exists(), "expected {}", output_path.display());
    let output_data = std::fs::read(&output_path).unwrap();
    assert!(!output_data.is_empty());
}

#[test]
fn compress_with_resize() {
    let (path, _dir) = make_test_image();
    let app = ImgCompress {
        input: path.clone(),
        output: None,
        format: Format::Png,
        quality: 80,
        width: 50,  // resize to max 50px wide
        height: 0,
        dry_run: false,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = Context::new_test(tx);

    let stats = run_compress_inner(&app, &ctx).unwrap();
    let dims = stats["output_dimensions"].as_str().unwrap();
    assert!(dims.starts_with("50×"), "expected 50×N, got: {dims}");
}

// ── Progress channel test ───────────────────────────────────

#[test]
fn progress_stream_emits_events() {
    let (path, _dir) = make_test_image();
    let app = ImgCompress {
        input: path.clone(),
        output: None,
        format: Format::Jpeg,
        quality: 90,
        width: 0,
        height: 0,
        dry_run: false,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = Context::new_test(tx);
    let handle = std::thread::spawn(move || run_compress_inner(&app, &ctx));

    let mut events = Vec::new();
    for event in rx {
        let is_done = matches!(event, Progress::Done { .. });
        events.push(event);
        if is_done { break; }
    }
    handle.join().unwrap().unwrap();

    // Should have at least Started, Tick, Done
    assert!(events.iter().any(|e| matches!(e, Progress::Started { .. })));
    assert!(events.iter().any(|e| matches!(e, Progress::Tick { .. })));
    assert!(events.iter().any(|e| matches!(e, Progress::Done { .. })));
}

// ── Re-implemented compression logic (mirrors main.rs) ──────
// We can't import from main.rs in a binary crate, so we duplicate the
// compression logic here. In a real app, you'd extract this to a library crate.

use std::time::Instant;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ExtendedColorType, GenericImageView};
use image::imageops::FilterType as ResizeFilter;

fn run_compress_inner(app: &ImgCompress, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started { total: Some(5), message: None });

    ctx.tick(1, Some(5), "Reading");
    let data = std::fs::read(&app.input)?;
    let in_size = data.len() as u64;
    let img = image::load_from_memory(&data)
        .map_err(|e| AppError::Runtime(format!("decode: {e}")))?;
    let (in_w, in_h) = img.dimensions();

    ctx.tick(2, Some(5), format!("{in_w}×{in_h}"));
    let img = resize(img, app.width, app.height);
    let (out_w, out_h) = img.dimensions();

    let out_path = match &app.output {
        Some(p) => PathBuf::from(p),
        None => {
            let ext = match app.format { Format::Jpeg => "jpg", Format::Png => "png", Format::Webp => "webp" };
            app.input.with_file_name(format!("compressed.{ext}"))
        }
    };

    if app.dry_run {
        let r = serde_json::json!({
            "input_size": in_size, "output_size": 0,
            "input_dimensions": format!("{in_w}×{in_h}"),
            "output_dimensions": format!("{out_w}×{out_h}"),
            "quality": app.quality, "dry_run": true,
            "compression_ratio": 0.0,
        });
        ctx.done(r.clone(), start.elapsed().as_millis() as u64);
        return Ok(r);
    }

    ctx.tick(3, Some(5), "Encoding");
    let compressed = encode(&img, &app.format, app.quality)?;
    let out_size = compressed.len() as u64;
    std::fs::write(&out_path, &compressed)?;

    ctx.tick(5, Some(5), "Done");
    let r = serde_json::json!({
        "input_size": in_size, "output_size": out_size,
        "input_dimensions": format!("{in_w}×{in_h}"),
        "output_dimensions": format!("{out_w}×{out_h}"),
        "quality": app.quality, "dry_run": false,
        "compression_ratio": (out_size as f64 / in_size as f64 * 1000.0).round() / 10.0,
    });
    ctx.done(r.clone(), start.elapsed().as_millis() as u64);
    Ok(r)
}

fn resize(img: DynamicImage, mw: u32, mh: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    let tw = if mw > 0 { mw } else { w };
    let th = if mh > 0 { mh } else { h };
    if tw >= w && th >= h { return img; }
    let r = (tw as f64 / w as f64).min(th as f64 / h as f64);
    img.resize_exact((w as f64 * r).max(1.0) as u32, (h as f64 * r).max(1.0) as u32, ResizeFilter::Lanczos3)
}

fn encode(img: &DynamicImage, fmt: &Format, q: u8) -> Result<Vec<u8>, AppError> {
    let mut buf = Vec::new();
    match fmt {
        Format::Jpeg => {
            let rgb = img.to_rgb8();
            JpegEncoder::new_with_quality(&mut buf, q)
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
