//! 八个视觉命令的行为测试 + 注册表形状断言（无网络、无浏览器：
//! `html-screenshot` 只测「找不到浏览器时的错误形状」）。

use super::build_registry;
use crate::common::{default_output, load_rgba, parse_region, save_png};
use crate::crop::{run_crop, Crop};
use crate::diff::{run_pixel_diff, PixelDiff};
use crate::dominant::{run_dominant_colors, DominantColors};
use crate::foreground::{run_extract_foreground, ExtractForeground};
use crate::info::{run_image_info, ImageInfo};
use crate::resize::{run_resize, Resize};
use crate::screenshot::{run_html_screenshot, HtmlScreenshot};
use crate::trace::{run_trace, strip_background, truncate_decimals, write_svg, Trace};
use image::{ImageBuffer, Rgba, RgbaImage};
use lilyco::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

fn ctx() -> Context {
    let (tx, _) = mpsc::channel();
    Context::new_test(tx)
}

/// 64×64 白底红方块测试图（白色包围红色）
fn red_on_white(w: u32, h: u32) -> RgbaImage {
    ImageBuffer::from_fn(w, h, |x, y| {
        if x >= w / 4 && x < 3 * w / 4 && y >= h / 4 && y < 3 * h / 4 {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    })
}

fn save_png_in(img: &RgbaImage, dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    save_png(img, &p).expect("save png");
    p
}

#[test]
fn image_info_reports_dimensions() {
    let dir = tempfile::tempdir().unwrap();
    let src = save_png_in(&red_on_white(64, 64), dir.path(), "in.png");

    let app = ImageInfo { image: src };
    let r = run_image_info(&app, &ctx()).unwrap();
    assert_eq!(r["format"], "png");
    assert_eq!(r["width"], 64);
    assert_eq!(r["height"], 64);
    assert!(r["file_size_bytes"].as_u64().unwrap() > 0);
    assert_eq!(r["ok"], true);
}

#[test]
fn crop_clamps_and_saves_png() {
    let dir = tempfile::tempdir().unwrap();
    let src = save_png_in(&red_on_white(64, 64), dir.path(), "in.png");
    let out = dir.path().join("crop.png").display().to_string();

    // 区域部分出界：50,50 → 右边/下边超出 64 边界，应收敛到 63
    let app = Crop {
        image: src,
        region: "50,50,120,90".into(),
        scale: 1,
        out: Some(out.clone()),
    };
    let r = run_crop(&app, &ctx()).unwrap();
    assert_eq!(r["width"], 14);
    assert_eq!(r["height"], 14);
    assert_eq!(r["out"], out);
    assert!(Path::new(&out).is_file(), "crop 产物应存在");
}

#[test]
fn resize_preserves_aspect_ratio() {
    let dir = tempfile::tempdir().unwrap();
    // 40×20 → 宽 20 → 高 10
    let img = ImageBuffer::from_fn(40, 20, |_, _| Rgba([128, 128, 128, 255]));
    let src = save_png_in(&img, dir.path(), "in.png");
    let out = dir.path().join("resize.png").display().to_string();

    let app = Resize {
        image: src,
        width: 20,
        height: 0,
        out: Some(out.clone()),
    };
    let r = run_resize(&app, &ctx()).unwrap();
    assert_eq!(r["width"], 20);
    assert_eq!(r["height"], 10);
    assert!(Path::new(&out).is_file());
}

#[test]
fn resize_rejects_zero_both() {
    let dir = tempfile::tempdir().unwrap();
    let src = save_png_in(&red_on_white(16, 16), dir.path(), "in.png");
    let app = Resize {
        image: src,
        width: 0,
        height: 0,
        out: None,
    };
    let err = run_resize(&app, &ctx()).unwrap_err();
    assert!(err.to_string().contains("width"), "got: {err}");
}

#[test]
fn dominant_colors_finds_red_on_white() {
    let dir = tempfile::tempdir().unwrap();
    let src = save_png_in(&red_on_white(64, 64), dir.path(), "in.png");
    let app = DominantColors {
        image: src,
        top: 8,
        tolerance: 2,
        region: None,
        candidates: None,
    };
    let r = run_dominant_colors(&app, &ctx()).unwrap();
    let colors = r["colors"].as_array().unwrap();
    assert!(!colors.is_empty());
    let hexes: Vec<String> = colors
        .iter()
        .filter_map(|c| c["hex"].as_str().map(String::from))
        .collect();
    assert!(
        hexes.contains(&"#ff0000".to_string()),
        "纯红应在主色里，got: {hexes:?}"
    );
    assert_eq!(r["top"], colors.len());
}
#[test]
fn pixel_diff_ranks_changed_region() {
    let dir = tempfile::tempdir().unwrap();
    // 原图：整片灰；重建图：左上角 16×16 变黑
    let a = ImageBuffer::from_fn(64, 64, |_, _| Rgba([200, 200, 200, 255]));
    let b = ImageBuffer::from_fn(64, 64, |x, y| {
        if x < 16 && y < 16 {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([200, 200, 200, 255])
        }
    });
    let orig = save_png_in(&a, dir.path(), "orig.png");
    let rebuilt = save_png_in(&b, dir.path(), "rebuilt.png");

    let app = PixelDiff {
        original: orig,
        rebuilt,
        grid: 4,
        top: 3,
        out_heatmap: Some(dir.path().join("heat.png").display().to_string()),
    };
    let r = run_pixel_diff(&app, &ctx()).unwrap();
    assert_eq!(r["grid"], 4);
    let worst = r["worst"].as_array().unwrap();
    assert!(!worst.is_empty());
    // 左上格应排第一（x1=0,y1=0,x2<=15）
    assert_eq!(worst[0]["x1"], 0);
    assert_eq!(worst[0]["y1"], 0);
    assert!(worst[0]["score"].as_f64().unwrap() > 0.0);
    // 其余格子得分应小于第一名
    let top = worst[0]["score"].as_f64().unwrap();
    assert!(worst.iter().all(|c| c["score"].as_f64().unwrap() <= top));
    assert!(r["mean_diff"].as_f64().unwrap() > 0.0);
    assert!(
        Path::new(&app.out_heatmap.clone().unwrap()).is_file(),
        "热力图产物应存在"
    );
}

#[test]
fn extract_foreground_keeps_center_red_on_white() {
    let dir = tempfile::tempdir().unwrap();
    let src = save_png_in(&red_on_white(64, 64), dir.path(), "in.png");
    let out = save_png_in(
        &ImageBuffer::from_fn(2, 2, |_, _| Rgba([0, 0, 0, 255])),
        dir.path(),
        "placeholder.png",
    );

    let app = ExtractForeground {
        images: vec![src.display().to_string()],
        mode: "color".into(),
        region: None,
        exclude_color: None,
        exclude_tol: 35,
        out: Some(out.display().to_string()),
    };
    let r = run_extract_foreground(&app, &ctx()).unwrap();
    let first = &r["outputs"][0];
    assert!(first["kept_pixels"].as_u64().unwrap() > 0);
    assert!(first["background_pixels"].as_u64().unwrap() > 0);

    let result = load_rgba(Path::new(first["out"].as_str().unwrap())).unwrap();
    // 角落（背景）透明，中心（红方块）不透明
    assert_eq!(result.get_pixel(0, 0)[3], 0);
    assert_eq!(result.get_pixel(32, 32)[3], 255);
}

#[test]
fn trace_produces_svg() {
    let dir = tempfile::tempdir().unwrap();
    let src = save_png_in(&red_on_white(48, 48), dir.path(), "in.png");
    let out = dir.path().join("out.svg");

    let app = Trace {
        image: src,
        color: true,
        polygon: false,
        scale: 1,
        out: Some(out.display().to_string()),
    };
    let r = run_trace(&app, &ctx()).unwrap();
    assert!(r["byte_size"].as_u64().unwrap() > 0);
    let svg = std::fs::read_to_string(&out).expect("read svg");
    assert!(
        svg.trim_start().starts_with("<svg") || svg.contains("<path"),
        "SVG 应含 <svg> 或 <path>，got: {}",
        &svg[..svg.len().min(120)]
    );
}

#[test]
fn extract_foreground_rejects_bad_mode() {
    let dir = tempfile::tempdir().unwrap();
    let src = save_png_in(&red_on_white(16, 16), dir.path(), "in.png");
    let app = ExtractForeground {
        images: vec![src.display().to_string()],
        mode: "neon".into(),
        region: None,
        exclude_color: None,
        exclude_tol: 35,
        out: None,
    };
    let err = run_extract_foreground(&app, &ctx()).unwrap_err();
    assert!(err.to_string().contains("color"), "got: {err}");
}

#[test]
fn html_screenshot_errors_without_browser() {
    let dir = tempfile::tempdir().unwrap();
    let html_path = dir.path().join("page.html");
    std::fs::write(&html_path, "<h1>hi</h1>").unwrap();
    // CHROME_PATH 指向不存在的浏览器 → 必须报错（不需要真实浏览器）
    let fake = dir.path().join("no-browser.exe");
    std::env::set_var("CHROME_PATH", fake.display().to_string());

    let app = HtmlScreenshot {
        source: html_path,
        width: 1280,
        height: 800,
        out: None,
    };
    let err = run_html_screenshot(&app, &ctx()).unwrap_err();
    std::env::remove_var("CHROME_PATH");
    assert!(
        err.to_string().contains("browser"),
        "应报浏览器缺失，got: {err}"
    );
}

#[test]
fn registry_has_eight_tools() {
    assert_eq!(build_registry().visible().count(), 8);
}

// ── 对齐上游 test_crop.py ─────────────────────────────

#[test]
fn crop_parse_rejects_malformed_regions() {
    for bad in ["1,2,3", "1,2,3,x", "1,2"] {
        assert!(
            parse_region(bad, 100, 100).is_err(),
            "region {bad:?} must be rejected"
        );
    }
}

#[test]
fn crop_clamps_negative_and_reversed() {
    // 负坐标夹到 0，越界夹到图像边缘（上游 clamp_box 契约）
    assert_eq!(
        parse_region("-20,5,300,95", 200, 100).unwrap(),
        [0, 5, 200, 95]
    );
    // 颠倒角点归一化
    assert_eq!(
        parse_region("30,40,10,20", 100, 100).unwrap(),
        [10, 20, 30, 40]
    );
}

#[test]
fn crop_empty_region_is_error() {
    // 全出界 → 收敛为空盒 → 报错（上游 "empty" 契约）
    let err = parse_region("500,500,600,600", 200, 100).unwrap_err();
    assert!(err.to_string().contains("empty"), "got: {err}");
}

#[test]
fn crop_scale_naming_and_content() {
    let dir = tempfile::tempdir().unwrap();
    let src = save_png_in(&red_on_white(200, 100), dir.path(), "shot.png");

    // 默认命名：scale>1 → {stem}.crop@{scale}x.png（输入同目录）
    let app = Crop {
        image: src.clone(),
        region: "50,30,150,70".into(),
        scale: 4,
        out: None,
    };
    let r = run_crop(&app, &ctx()).unwrap();
    let out = PathBuf::from(r["out"].as_str().unwrap());
    assert!(out.is_file(), "crop@4x 产物应存在: {out:?}");
    assert!(
        out.to_string_lossy().ends_with("shot.crop@4x.png"),
        "命名应对齐 {{stem}}.crop@{{scale}}x.png: {out:?}"
    );
    let crop = load_rgba(&out).unwrap();
    assert_eq!(crop.dimensions(), (400, 160));
    assert_eq!(
        crop.get_pixel(20, 20),
        &Rgba([255, 0, 0, 255]),
        "放大后红块仍在"
    );

    // 显式 -o 优先
    let custom = dir.path().join("scaled.png").display().to_string();
    let app2 = Crop {
        image: src.clone(),
        region: "50,30,150,70".into(),
        scale: 2,
        out: Some(custom.clone()),
    };
    let r2 = run_crop(&app2, &ctx()).unwrap();
    assert_eq!(r2["out"], custom);
    assert_eq!(
        load_rgba(Path::new(&custom)).unwrap().dimensions(),
        (200, 80)
    );

    // scale=0 拒绝
    let app3 = Crop {
        image: src,
        region: "50,30,150,70".into(),
        scale: 0,
        out: None,
    };
    assert!(run_crop(&app3, &ctx()).is_err());
}

#[test]
fn crop_missing_file_is_error() {
    let app = Crop {
        image: PathBuf::from("C:/definitely/not/here.png"),
        region: "0,0,10,10".into(),
        scale: 1,
        out: None,
    };
    assert!(run_crop(&app, &ctx()).is_err());
}

// ── 对齐上游 test_dominant_colors.py ───────────────────

#[test]
fn dominant_colors_merges_near_duplicates() {
    // #F5F5F5 与 #F3F3F3 差 2，默认容差 8 应合并为单簇
    let dir = tempfile::tempdir().unwrap();
    let img = ImageBuffer::from_fn(200, 100, |x, _y| {
        if x < 100 {
            Rgba([245, 245, 245, 255])
        } else {
            Rgba([243, 243, 243, 255])
        }
    });
    let src = save_png_in(&img, dir.path(), "near.png");
    let app = DominantColors {
        image: src,
        top: 3,
        tolerance: 0, // 0 → 默认 8
        region: None,
        candidates: None,
    };
    let r = run_dominant_colors(&app, &ctx()).unwrap();
    let colors = r["colors"].as_array().unwrap();
    assert!(
        colors[0]["percent"].as_f64().unwrap() > 90.0,
        "近色应合并成主簇: {colors:?}"
    );
}

#[test]
fn dominant_colors_pick_exact_candidate_wins() {
    let dir = tempfile::tempdir().unwrap();
    let img = ImageBuffer::from_fn(60, 30, |_, _| Rgba([245, 245, 245, 255]));
    let src = save_png_in(&img, dir.path(), "gray.png");
    let app = DominantColors {
        image: src,
        top: 5,
        tolerance: 16,
        region: None,
        candidates: Some("#F9FAFA,#F5F5F5,#F3F3F3,#EDEDED".into()),
    };
    let r = run_dominant_colors(&app, &ctx()).unwrap();
    assert_eq!(r["winner"]["text"], "#F5F5F5");
    assert_eq!(r["winner"]["share"], 100.0);
}

#[test]
fn dominant_colors_pick_no_match_reports_closest() {
    let dir = tempfile::tempdir().unwrap();
    let img = ImageBuffer::from_fn(40, 40, |_, _| Rgba([0, 0, 255, 255])); // blue
    let src = save_png_in(&img, dir.path(), "blue.png");
    let app = DominantColors {
        image: src,
        top: 5,
        tolerance: 16,
        region: None,
        candidates: Some("#F9FAFA,#F5F5F5".into()),
    };
    let r = run_dominant_colors(&app, &ctx()).unwrap();
    assert!(r["winner"].is_null(), "容差内无命中应无赢家: {r}");
    assert_eq!(r["closest"]["text"], "#F5F5F5", "蓝更接近 #F5F5F5: {r}");
}

#[test]
fn dominant_colors_region_clamp_and_empty_reject() {
    let dir = tempfile::tempdir().unwrap();
    let src = save_png_in(&red_on_white(100, 80), dir.path(), "in.png");
    // 负坐标夹紧 → 正常执行
    let ok = DominantColors {
        image: src.clone(),
        top: 3,
        tolerance: 8,
        region: Some("-10,-10,120,90".into()),
        candidates: None,
    };
    assert!(run_dominant_colors(&ok, &ctx()).is_ok());
    // 空盒 → 报错
    let empty = DominantColors {
        image: src,
        top: 3,
        tolerance: 8,
        region: Some("50,50,50,60".into()),
        candidates: None,
    };
    let err = run_dominant_colors(&empty, &ctx()).unwrap_err();
    assert!(err.to_string().contains("empty"), "got: {err}");
}

// ── 对齐上游 test_pixel_diff.py ────────────────────────

#[test]
fn pixel_diff_composites_transparent_on_white() {
    let dir = tempfile::tempdir().unwrap();
    let white = ImageBuffer::from_fn(240, 120, |_, _| Rgba([255, 255, 255, 255]));
    let transparent = ImageBuffer::from_fn(240, 120, |_, _| Rgba([0, 0, 0, 0]));
    let orig = save_png_in(&white, dir.path(), "a.png");
    let rebuilt = save_png_in(&transparent, dir.path(), "t.png");
    let app = PixelDiff {
        original: orig,
        rebuilt,
        grid: 4,
        top: 1,
        out_heatmap: None,
    };
    let r = run_pixel_diff(&app, &ctx()).unwrap();
    assert_eq!(r["overall_diff_pct"], 0.0, "透明重建应合成白底而非黑底");
}

#[test]
fn pixel_diff_ranks_corrupted_cell() {
    let dir = tempfile::tempdir().unwrap();
    let base = ImageBuffer::from_fn(240, 120, |_, _| Rgba([255, 255, 255, 255]));
    let broken = ImageBuffer::from_fn(240, 120, |x, y| {
        if x >= 180 && y < 60 {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    });
    let orig = save_png_in(&base, dir.path(), "a.png");
    let rebuilt = save_png_in(&broken, dir.path(), "b.png");
    let app = PixelDiff {
        original: orig,
        rebuilt,
        grid: 4,
        top: 1,
        out_heatmap: None,
    };
    let r = run_pixel_diff(&app, &ctx()).unwrap();
    let worst = r["worst"][0].clone();
    assert_eq!(worst["x1"], 180, "最差格应指向被破坏的格子: {worst}");
    let score = worst["score"].as_f64().unwrap();
    assert!((score - 1.0).abs() < 0.01, "全黑对全白应 ≈100%: {score}");
    assert!(r["overall_diff_pct"].as_f64().unwrap() > 0.0);
}

// ── 对齐上游 test_extract_fg.py（本实现契约）────────────

/// 徽章图：浅蓝圆盘 + 白环 + 深蓝字形 + 右下角深灰噪声（上游 _make_badge）
fn badge_image(w: u32, h: u32) -> RgbaImage {
    let cx = w / 2;
    let cy = h / 2;
    ImageBuffer::from_fn(w, h, |x, y| {
        // 右下角深灰噪声（连接背景，应被洪水填充 + 去噪剔除）
        if x >= w - 30 && x < w - 15 && y >= h - 20 && y < h - 8 {
            return Rgba([85, 85, 85, 255]);
        }
        let dx = (x as i64 - cx as i64) as f64;
        let dy = (y as i64 - cy as i64) as f64;
        let d = (dx * dx + dy * dy).sqrt();
        if d <= 20.0 {
            Rgba([47, 95, 191, 255]) // glyph 深蓝
        } else if d <= 60.0 {
            Rgba([180, 211, 240, 255]) // disc 浅蓝
        } else if d <= 64.0 {
            Rgba([255, 255, 255, 255]) // ring 白环
        } else {
            Rgba([234, 241, 249, 255]) // background
        }
    })
}

#[test]
fn extract_fg_multi_image_and_noise() {
    let dir = tempfile::tempdir().unwrap();
    let a = save_png_in(&badge_image(200, 200), dir.path(), "a.png");
    let b = save_png_in(&badge_image(200, 200), dir.path(), "b.png");
    let app = ExtractForeground {
        images: vec![a.display().to_string(), b.display().to_string()],
        mode: "color".into(),
        region: None,
        exclude_color: None,
        exclude_tol: 35,
        out: None,
    };
    let r = run_extract_foreground(&app, &ctx()).unwrap();
    assert_eq!(r["count"], 2);
    let outs = r["outputs"].as_array().unwrap();
    for (i, name) in ["a.clean.png", "b.clean.png"].iter().enumerate() {
        let out = outs[i]["out"].as_str().unwrap();
        assert!(out.ends_with(name), "命名应对齐 {{stem}}.clean.png: {out}");
        let img = load_rgba(Path::new(out)).unwrap();
        // 前景数量足够
        let fg: Vec<&Rgba<u8>> = img.pixels().filter(|p| p[3] > 128).collect();
        assert!(fg.len() > 200, "前景太少: {}", fg.len());
        // 深蓝字形保留
        let deep_blue = fg
            .iter()
            .filter(|p| {
                p[2] > p[0] + 40 && (p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])) > 60
            })
            .count();
        assert!(deep_blue > 200, "深蓝字形应保留: {deep_blue}");
        // 灰噪声被剔除
        let noise = fg
            .iter()
            .filter(|p| p[0].abs_diff(85) < 25 && p[1].abs_diff(85) < 25)
            .count();
        assert_eq!(noise, 0, "灰噪声不应泄漏进前景");
    }
}

#[test]
fn extract_fg_manual_region_exclude_tol() {
    let dir = tempfile::tempdir().unwrap();
    let src = save_png_in(&badge_image(200, 200), dir.path(), "icon.png");
    let app = ExtractForeground {
        images: vec![src.display().to_string()],
        mode: "color".into(),
        region: Some("40,40,160,160".into()),
        exclude_color: Some("#EAF1F9".into()), // 背景色，容差 35
        exclude_tol: 35,
        out: None,
    };
    let r = run_extract_foreground(&app, &ctx()).unwrap();
    let first = &r["outputs"][0];
    assert!(first["kept_pixels"].as_u64().unwrap() > 0);
    assert!(first["background_pixels"].as_u64().unwrap() > 0);
    let img = load_rgba(Path::new(first["out"].as_str().unwrap())).unwrap();
    let deep_blue = img
        .pixels()
        .filter(|p| {
            p[3] > 128
                && p[2] > p[0] + 40
                && (p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])) > 60
        })
        .count();
    assert!(deep_blue > 200, "手动区域应保留深蓝字形: {deep_blue}");
}

// ── 对齐上游 test_trace.py ─────────────────────────────

#[test]
fn trace_post_processors() {
    let svg = "<svg><path d=\"M0,0 L9,0 Z\" fill=\"#FFFFFF\" transform=\"x\"/><path d=\"M1.23456,7.891011 L2,3\" fill=\"#000000\"/></svg>";
    let stripped = strip_background(svg);
    assert!(
        !stripped.contains("fill=\"#FFFFFF\""),
        "白色整底 path 应被剥掉"
    );
    assert!(stripped.contains("fill=\"#000000\""));

    let kept = strip_background("<svg><path d=\"M0,0\" fill=\"#000000\"/></svg>");
    assert!(kept.contains("fill=\"#000000\""), "非白首 path 应保留");

    let truncated = truncate_decimals(&stripped);
    assert!(!truncated.contains("1.23456") && truncated.contains("1.23"));
    assert!(!truncated.contains("7.891011") && truncated.contains("7.89"));
}

#[test]
fn trace_auto_upscales_small_icon() {
    let dir = tempfile::tempdir().unwrap();
    // 24×24 白底 + 2×2 黑块（默认自动放大到短边 256）
    let img = ImageBuffer::from_fn(24, 24, |x, y| {
        if (10..12).contains(&x) && (10..12).contains(&y) {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    });
    let src = save_png_in(&img, dir.path(), "icon.png");
    let out = dir.path().join("icon.svg");

    let app = Trace {
        image: src.clone(),
        color: false,
        polygon: true,
        scale: 0, // 自动
        out: Some(out.display().to_string()),
    };
    let r = run_trace(&app, &ctx()).unwrap();
    assert!(
        r["traced_at"].as_u64().unwrap() > 1,
        "小图标应自动放大: {}",
        r["traced_at"]
    );
    let svg = std::fs::read_to_string(&out).unwrap();
    assert!(svg.contains("<path"), "自动放大后小图标应可矢量化");

    // 显式 scale=1：2×2 黑块 < speckle 8 → 0 paths → 报错并指明 --scale
    let out2 = dir.path().join("icon2.svg");
    let app1x = Trace {
        image: src,
        color: false,
        polygon: true,
        scale: 1,
        out: Some(out2.display().to_string()),
    };
    let err = run_trace(&app1x, &ctx()).unwrap_err();
    assert!(
        err.to_string().contains("--scale"),
        "空 trace 应点名恢复手段: {err}"
    );
}

#[test]
fn trace_writes_exact_utf8_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("multiline.svg");
    let payload = "<svg>\n<title>几何</title>\n<path/>\n</svg>\n";
    let reported = write_svg(&out, payload).unwrap();
    let written = std::fs::read(&out).unwrap();
    assert_eq!(written, payload.as_bytes(), "SVG 应保留精确 UTF-8 字节");
    assert_eq!(reported, written.len(), "报告字节数应等于磁盘字节数");
    assert!(
        reported > payload.chars().count(),
        "字节契约不能按字符数计算"
    );
}

// ── 对齐上游 test_html_shot.py（命名契约，路径差异已记录）─

#[test]
fn html_shot_default_naming() {
    // 无父目录 → 裸 stem.png；有父目录 → 输入同目录（我方约定，上游是 cwd）
    assert_eq!(
        default_output(Path::new("page.html"), ".png"),
        PathBuf::from("page.png")
    );
    let with_dir = default_output(Path::new("/tmp/a/b/page.html"), ".png");
    assert!(
        with_dir.to_string_lossy().contains("page.png"),
        "应保留 stem.png: {with_dir:?}"
    );
    assert_eq!(
        with_dir.parent().and_then(|p| p.file_name()),
        Some(std::ffi::OsStr::new("b")),
        "应落在输入同目录: {with_dir:?}"
    );
}
