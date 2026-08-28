use std::path::PathBuf;

use lilyco_core::prelude::*;
use lilyco_core::schema::{CommandSchema, ValueEnum};
use lilyco_macros::{App, ValueEnum};

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
    assert_eq!(dry_run.name, "dry-run");
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
    args.insert("dry-run".into(), serde_json::json!(true));

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

// ── 测试 8：可选数值字段（Option<u32> / Option<f64>）─────────

#[derive(App)]
struct OptNumCmd {
    #[arg(about = "width")]
    width: Option<u32>,

    #[arg(about = "crf")]
    crf: Option<f64>,

    #[arg(about = "name")]
    name: Option<String>,
}

#[test]
fn optional_numeric_fields_are_optional_number_kind() {
    let schema = OptNumCmd::schema();
    assert_eq!(schema.args.len(), 3);
    for a in &schema.args {
        assert!(!a.required, "{} should not be required", a.name);
    }
    let width_json = serde_json::to_value(&schema.args[0]).unwrap();
    assert_eq!(width_json["kind"]["type"], "number", "width is Number");
    let crf_json = serde_json::to_value(&schema.args[1]).unwrap();
    assert_eq!(crf_json["kind"]["type"], "number", "crf is Number");
}

#[test]
fn optional_numeric_from_args_present_and_absent() {
    use std::collections::HashMap;

    // 提供了 width / name → 有值；没提供 crf → None
    let mut args = HashMap::new();
    args.insert("width".into(), serde_json::json!(1280));
    args.insert("name".into(), serde_json::json!("hello"));
    let cmd: OptNumCmd = OptNumCmd::from_args(&args).unwrap();
    assert_eq!(cmd.width, Some(1280u32));
    assert_eq!(cmd.crf, None);
    assert_eq!(cmd.name.as_deref(), Some("hello"));

    // 全空 → 全 None
    let cmd2: OptNumCmd = OptNumCmd::from_args(&HashMap::new()).unwrap();
    assert_eq!(cmd2.width, None);
    assert_eq!(cmd2.crf, None);
    assert_eq!(cmd2.name, None);
}

// -- Test 6: run attribute wires up the function --

fn test_run_fn(
    _app: &RunCmd,
    _ctx: &lilyco_core::Context,
) -> Result<serde_json::Value, lilyco_core::AppError> {
    Ok(serde_json::json!({"called": true}))
}

/// Test command with run attribute
#[derive(App)]
#[app(about = "test run attribute", run = "test_run_fn")]
struct RunCmd {
    /// name field
    name: String,
}

#[test]
fn run_attribute_calls_specified_function() {
    use std::collections::HashMap;

    let mut args = HashMap::new();
    args.insert("name".into(), serde_json::json!("hello"));

    let app: RunCmd = RunCmd::from_args(&args).unwrap();
    let (tx, _rx) = std::sync::mpsc::channel();
    let ctx = lilyco_core::Context::new_test(tx);

    let result = app.run(&ctx).unwrap();
    assert_eq!(result, serde_json::json!({"called": true}));
}

// -- Test 7: run attribute with about from doc comment --

fn echo_run(
    app: &EchoCmd,
    _ctx: &lilyco_core::Context,
) -> Result<serde_json::Value, lilyco_core::AppError> {
    Ok(serde_json::json!({"echo": app.message}))
}

/// Echo a message
#[derive(App)]
#[app(run = "echo_run")]
struct EchoCmd {
    /// Message to echo
    message: String,
}

#[test]
fn run_attribute_works_with_doc_comment_about() {
    use std::collections::HashMap;

    let schema = EchoCmd::schema();
    assert_eq!(schema.about, "Echo a message");

    let mut args = HashMap::new();
    args.insert("message".into(), serde_json::json!("hello world"));

    let app: EchoCmd = EchoCmd::from_args(&args).unwrap();
    let (tx, _rx) = std::sync::mpsc::channel();
    let ctx = lilyco_core::Context::new_test(tx);

    let result = app.run(&ctx).unwrap();
    assert_eq!(result["echo"], serde_json::json!("hello world"));
}
