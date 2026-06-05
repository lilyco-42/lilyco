use std::path::PathBuf;

use triforge_core::prelude::*;
use triforge_core::schema::{CommandSchema, ValueEnum};
use triforge_macros::{App, ValueEnum};

// ── 测试用类型 ────────────────────────────────────────────

#[derive(ValueEnum)]
enum Codec {
    H264,
    H265,
    Av1,
}

#[derive(App)]
#[app(about = "转码视频文件")]
struct Transcode {
    #[arg(about = "输入文件", must_exist = true)]
    input: PathBuf,

    #[arg(about = "输出文件")]
    output: PathBuf,

    #[arg(about = "输出编码", default = "h264")]
    codec: Codec,

    #[arg(about = "质量 0-51", default = 23, range = 0..=51)]
    quality: u8,

    #[arg(about = "仅预览")]
    dry_run: bool,
}

// ── 测试 1：derive_app_generates_schema ──────────────────

#[test]
fn derive_app_generates_schema() {
    let schema: CommandSchema = Transcode::schema();
    assert_eq!(schema.name, "Transcode");
    assert_eq!(schema.about, "转码视频文件");
    assert_eq!(schema.args.len(), 5);

    // input: Path, required
    let input = &schema.args[0];
    assert_eq!(input.name, "input");
    assert!(input.required);

    // dry_run: Flag, not required
    let dry_run = &schema.args[4];
    assert_eq!(dry_run.name, "dry_run");
    assert!(!dry_run.required);
}

// ── 测试 2：derive_app_from_args_works ───────────────────

#[test]
fn derive_app_from_args_works() {
    use std::collections::HashMap;

    let mut args = HashMap::new();
    args.insert("input".into(), serde_json::json!("/tmp/a.mp4"));
    args.insert("output".into(), serde_json::json!("/tmp/b.mp4"));
    args.insert("codec".into(), serde_json::json!("h265"));
    args.insert("quality".into(), serde_json::json!(42));
    args.insert("dry_run".into(), serde_json::json!(true));

    let t: Transcode = Transcode::from_args(&args).unwrap();
    assert!(t.dry_run);
    assert_eq!(t.quality, 42);
}

// ── 测试 3：derive_value_enum_works ──────────────────────

#[test]
fn derive_value_enum_works() {
    let variants = Codec::variants();
    assert_eq!(variants, vec!["h264", "h265", "av1"]);

    assert!(Codec::from_str("h265").is_some());
    assert!(Codec::from_str("vp9").is_none());
}

// ── 测试 4：optional_field_not_required ──────────────────

#[derive(App)]
struct OptCmd {
    #[arg(about = "optional text")]
    name: Option<String>,

    #[arg(about = "required text")]
    title: String,
}

#[test]
fn optional_field_not_required() {
    let schema = OptCmd::schema();
    assert!(!schema.args[0].required, "Option<T> should not be required");
    assert!(schema.args[1].required, "bare String should be required");
}

// ── 测试 5：range_attr_sets_number_bounds ────────────────

#[derive(App)]
struct NumCmd {
    #[arg(about = "score", range = 0..=100)]
    score: u32,
}

#[test]
fn range_attr_sets_number_bounds() {
    let schema = NumCmd::schema();
    let arg = &schema.args[0];
    // Verify it's a Number kind (we check by serializing to JSON)
    let json = serde_json::to_value(arg).unwrap();
    let kind = &json["kind"];
    assert_eq!(kind["type"], "number", "should be Number kind");
}
