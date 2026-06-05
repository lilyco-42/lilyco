use clap::{Arg, ArgAction, Command};
use std::ffi::OsStr;

use triforge_core::prelude::*;
use triforge_core::schema::{ArgKind, ArgSchema, CommandSchema};

// ── CliRenderer ────────────────────────────────────────────

/// CLI 渲染器：把 `CommandSchema` 转换成 `clap::Command`
///
/// 转换规则：
/// - `ArgKind::Flag`        → `--flag`
/// - `ArgKind::Text`        → `--name <value>`
/// - `ArgKind::Number`      → `--count <num>` + range validator
/// - `ArgKind::Enum`        → `--mode <choice>` + PossibleValuesParser
/// - `ArgKind::Path`        → `--file <path>` + exists validator (可选)
/// - `ArgKind::List`        → `--tag a --tag b` (num_args(1..), Append)
///
/// 所有命令自动附加内置标志：
/// - `--schema`           打印 JSON Schema
/// - `--openai-tool`      打印 OpenAI tool 定义
/// - `--anthropic-tool`   打印 Anthropic tool 定义
/// - `--json`             输出格式：单个 JSON
/// - `--json-stream`      输出格式：JSON 流
#[derive(Debug, Clone, Default)]
pub struct CliRenderer;

impl Renderer for CliRenderer {
    type Output = Command;

    fn render(&self, schema: &CommandSchema) -> Self::Output {
        build_command(schema)
    }
}

impl CliRenderer {
    /// 构造
    pub fn new() -> Self {
        Self
    }

    /// 处理内置标志（--schema / --openai-tool / --anthropic-tool）
    ///
    /// 返回 `true` 表示已打印并应退出进程。
    pub fn handle_builtin_flags(schema: &CommandSchema, matches: &clap::ArgMatches) -> bool {
        if matches.get_flag("schema") {
            println!(
                "{}",
                serde_json::to_string_pretty(&schema).unwrap()
            );
            return true;
        }
        if matches.get_flag("openai-tool") {
            println!(
                "{}",
                serde_json::to_string_pretty(&schema.to_openai_tool()).unwrap()
            );
            return true;
        }
        if matches.get_flag("anthropic-tool") {
            println!(
                "{}",
                serde_json::to_string_pretty(&schema.to_anthropic_tool()).unwrap()
            );
            return true;
        }
        false
    }

    /// 从 clap matches 中提取 `OutputFormat`
    pub fn output_format(matches: &clap::ArgMatches) -> OutputFormat {
        if matches.get_flag("json-stream") {
            OutputFormat::JsonStream
        } else if matches.get_flag("json") {
            OutputFormat::Json
        } else {
            OutputFormat::Human
        }
    }

    /// 从 clap matches 中提取用户参数为 `HashMap<String, serde_json::Value>`
    ///
    /// 供 `App::from_args()` 使用。
    pub fn extract_args(
        schema: &CommandSchema,
        matches: &clap::ArgMatches,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        let mut map = std::collections::HashMap::new();

        for arg in &schema.args {
            let name = &arg.name;

            // 跳过内置标志
            if is_builtin_flag(name) {
                continue;
            }

            match &arg.kind {
                ArgKind::Flag => {
                    let val = matches.get_flag(name);
                    map.insert(name.clone(), serde_json::Value::Bool(val));
                }
                ArgKind::List { .. } => {
                    let vals: Vec<String> = matches
                        .get_many::<String>(name)
                        .map(|vs| vs.cloned().collect())
                        .unwrap_or_default();
                    map.insert(
                        name.clone(),
                        serde_json::Value::Array(
                            vals.into_iter().map(serde_json::Value::String).collect(),
                        ),
                    );
                }
                ArgKind::Number { .. } => {
                    if let Some(v) = matches.get_one::<f64>(name) {
                        map.insert(
                            name.clone(),
                            serde_json::json!(v),
                        );
                    }
                }
                ArgKind::Path { .. } => {
                    if let Some(v) = matches.get_one::<std::path::PathBuf>(name) {
                        map.insert(
                            name.clone(),
                            serde_json::Value::String(v.display().to_string()),
                        );
                    }
                }
                ArgKind::Enum { .. } | ArgKind::Text => {
                    if let Some(v) = matches.get_one::<String>(name) {
                        map.insert(name.clone(), serde_json::Value::String(v.clone()));
                    }
                }
            }
        }

        map
    }
}

// ── 内部实现 ───────────────────────────────────────────────

/// clap 4 的内部类型（Str / Id / OsStr）仅接受 `&'static str`，
/// 不接受 `String`。用 Box::leak 将运行时字符串提升为 'static 是唯一方案。
/// 这些泄漏的值在进程生命周期内有效，无累积问题。
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn build_command(schema: &CommandSchema) -> Command {
    let mut cmd = Command::new(leak_str(&schema.name))
        .about(schema.about.clone())
        .disable_version_flag(true);

    for arg in &schema.args {
        cmd = cmd.arg(arg_to_clap(arg));
    }

    for sub in &schema.subcommands {
        cmd = cmd.subcommand(build_command(sub));
    }

    add_builtin_flags(cmd)
}

fn arg_to_clap(arg: &ArgSchema) -> Arg {
    let name: &'static str = leak_str(&arg.name);
    let mut a = Arg::new(name)
        .long(name)
        .help(arg.about.clone());

    if arg.required {
        a = a.required(true);
    } else if let Some(ref default) = arg.default {
        if let Some(def_str) = json_value_to_os_str(default) {
            a = a.default_value(def_str);
        }
    }

    match &arg.kind {
        ArgKind::Flag => {
            a = a.action(ArgAction::SetTrue);
        }
        ArgKind::Text => {
            a = a.value_parser(clap::value_parser!(String));
        }
        ArgKind::Number { min, max } => {
            let min = *min;
            let max = *max;
            a = a.value_parser(move |s: &str| -> Result<f64, String> {
                let v: f64 = s
                    .parse()
                    .map_err(|e| format!("invalid number: {e}"))?;
                if let Some(lo) = min {
                    if v < lo {
                        return Err(format!("value must be >= {lo}, got {v}"));
                    }
                }
                if let Some(hi) = max {
                    if v > hi {
                        return Err(format!("value must be <= {hi}, got {v}"));
                    }
                }
                Ok(v)
            });
        }
        ArgKind::Enum { values } => {
            let pv: Vec<&'static str> = values.iter().map(|s| leak_str(s)).collect();
            a = a.value_parser(clap::builder::PossibleValuesParser::new(pv));
        }
        ArgKind::Path { must_exist } => {
            let must_exist = *must_exist;
            a = a.value_parser(move |s: &str| -> Result<std::path::PathBuf, String> {
                let p = std::path::PathBuf::from(s);
                if must_exist && !p.exists() {
                    return Err(format!("path does not exist: {s}"));
                }
                Ok(p)
            });
        }
        ArgKind::List { item } => {
            let vp = arg_kind_to_value_parser(item);
            a = a
                .num_args(1..)
                .action(ArgAction::Append)
                .value_parser(vp);
        }
    }

    a
}

fn arg_kind_to_value_parser(kind: &ArgKind) -> clap::builder::ValueParser {
    match kind {
        ArgKind::Flag => clap::builder::ValueParser::bool(),
        ArgKind::Text => clap::builder::ValueParser::string(),
        ArgKind::Number { min, max } => {
            let min = *min;
            let max = *max;
            clap::builder::ValueParser::new(move |s: &str| -> Result<f64, String> {
                let v: f64 = s
                    .parse()
                    .map_err(|e| format!("invalid number: {e}"))?;
                if let Some(lo) = min {
                    if v < lo {
                        return Err(format!("value must be >= {lo}, got {v}"));
                    }
                }
                if let Some(hi) = max {
                    if v > hi {
                        return Err(format!("value must be <= {hi}, got {v}"));
                    }
                }
                Ok(v)
            })
        }
        ArgKind::Enum { values } => {
            let pv: Vec<&'static str> = values.iter().map(|s| leak_str(s)).collect();
            clap::builder::PossibleValuesParser::new(pv).into()
        }
        ArgKind::Path { must_exist } => {
            let must_exist = *must_exist;
            clap::builder::ValueParser::new(move |s: &str| -> Result<std::path::PathBuf, String> {
                let p = std::path::PathBuf::from(s);
                if must_exist && !p.exists() {
                    return Err(format!("path does not exist: {s}"));
                }
                Ok(p)
            })
        }
        ArgKind::List { item } => arg_kind_to_value_parser(item),
    }
}

fn add_builtin_flags(cmd: Command) -> Command {
    cmd.arg(
        Arg::new("schema")
            .long("schema")
            .help("打印 JSON Schema 并退出")
            .action(ArgAction::SetTrue)
            .exclusive(true),
    )
    .arg(
        Arg::new("openai-tool")
            .long("openai-tool")
            .help("打印 OpenAI tool 定义并退出")
            .action(ArgAction::SetTrue)
            .exclusive(true),
    )
    .arg(
        Arg::new("anthropic-tool")
            .long("anthropic-tool")
            .help("打印 Anthropic tool 定义并退出")
            .action(ArgAction::SetTrue)
            .exclusive(true),
    )
    .arg(
        Arg::new("json")
            .long("json")
            .help("输出格式：单个 JSON")
            .action(ArgAction::SetTrue),
    )
    .arg(
        Arg::new("json-stream")
            .long("json-stream")
            .help("输出格式：JSON 流")
            .action(ArgAction::SetTrue),
    )
}

fn is_builtin_flag(name: &str) -> bool {
    matches!(
        name,
        "schema" | "openai-tool" | "anthropic-tool" | "json" | "json-stream"
    )
}

/// 将 `serde_json::Value` 转成 clap 可用的默认值（'static OsStr）
fn json_value_to_os_str(v: &serde_json::Value) -> Option<&'static OsStr> {
    let s: String = match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };
    // clap 的 OsStr 仅接受 &'static OsStr，无 String 或 Id 等价物
    Some(Box::leak(std::ffi::OsString::from(s).into_boxed_os_str()))
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use triforge_core::schema::CommandSchema;

    /// 构建一个示例 schema：transcode --env prod --quality 23
    fn transcode_schema() -> CommandSchema {
        CommandSchema {
            name: "transcode".into(),
            about: "转码视频".into(),
            args: vec![
                ArgSchema {
                    name: "env".into(),
                    about: "环境".into(),
                    kind: ArgKind::Text,
                    required: true,
                    default: None,
                },
                ArgSchema {
                    name: "quality".into(),
                    about: "质量 0-51".into(),
                    kind: ArgKind::Number {
                        min: Some(0.0),
                        max: Some(51.0),
                    },
                    required: false,
                    default: Some(serde_json::json!(23)),
                },
                ArgSchema {
                    name: "codec".into(),
                    about: "编码格式".into(),
                    kind: ArgKind::Enum {
                        values: vec!["h264".into(), "h265".into(), "av1".into()],
                    },
                    required: false,
                    default: Some(serde_json::json!("h264")),
                },
                ArgSchema {
                    name: "input".into(),
                    about: "输入文件".into(),
                    kind: ArgKind::Path { must_exist: false },
                    required: true,
                    default: None,
                },
                ArgSchema {
                    name: "tags".into(),
                    about: "标签列表".into(),
                    kind: ArgKind::List {
                        item: Box::new(ArgKind::Text),
                    },
                    required: false,
                    default: None,
                },
            ],
            subcommands: vec![],
        }
    }

    // ─── 基础 render ───────────────────────────────────

    #[test]
    fn render_produces_valid_command() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);
        assert_eq!(cmd.get_name(), "transcode");
    }

    // ─── parse 能力 ────────────────────────────────────

    #[test]
    fn parse_env_and_quality() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let matches = cmd.try_get_matches_from([
            "transcode",
            "--env", "prod",
            "--quality", "23",
            "--input", "test.mp4",
        ]).unwrap();

        assert_eq!(matches.get_one::<String>("env").unwrap(), "prod");
        assert_eq!(*matches.get_one::<f64>("quality").unwrap(), 23.0);
    }

    #[test]
    fn parse_with_defaults() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let matches = cmd.try_get_matches_from([
            "transcode",
            "--env", "prod",
            "--input", "test.mp4",
        ]).unwrap();

        // codec 有默认值 "h264"
        assert_eq!(matches.get_one::<String>("codec").unwrap(), "h264");
        // quality 有默认值 23
        assert_eq!(*matches.get_one::<f64>("quality").unwrap(), 23.0);
    }

    #[test]
    fn parse_flag() {
        let schema = CommandSchema {
            name: "cmd".into(),
            about: "test".into(),
            args: vec![ArgSchema {
                name: "verbose".into(),
                about: "详细输出".into(),
                kind: ArgKind::Flag,
                required: false,
                default: None,
            }],
            subcommands: vec![],
        };
        let renderer = CliRenderer::new();
        let cmd = renderer.render(&schema);

        let m = cmd.clone().try_get_matches_from(["cmd", "--verbose"]).unwrap();
        assert!(m.get_flag("verbose"));

        let m = cmd.try_get_matches_from(["cmd"]).unwrap();
        assert!(!m.get_flag("verbose"));
    }

    #[test]
    fn parse_list() {
        let schema = CommandSchema {
            name: "cmd".into(),
            about: "test".into(),
            args: vec![ArgSchema {
                name: "tags".into(),
                about: "标签".into(),
                kind: ArgKind::List {
                    item: Box::new(ArgKind::Text),
                },
                required: false,
                default: None,
            }],
            subcommands: vec![],
        };
        let renderer = CliRenderer::new();
        let cmd = renderer.render(&schema);

        let m = cmd
            .try_get_matches_from(["cmd", "--tags", "a", "--tags", "b"])
            .unwrap();
        let tags: Vec<&str> = m
            .get_many::<String>("tags")
            .unwrap()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(tags, vec!["a", "b"]);
    }

    #[test]
    fn parse_enum() {
        let schema = CommandSchema {
            name: "cmd".into(),
            about: "test".into(),
            args: vec![ArgSchema {
                name: "codec".into(),
                about: "编码".into(),
                kind: ArgKind::Enum {
                    values: vec!["h264".into(), "h265".into()],
                },
                required: true,
                default: None,
            }],
            subcommands: vec![],
        };
        let renderer = CliRenderer::new();
        let cmd = renderer.render(&schema);

        let m = cmd.clone()
            .try_get_matches_from(["cmd", "--codec", "h265"])
            .unwrap();
        assert_eq!(m.get_one::<String>("codec").unwrap(), "h265");

        // 非法值应失败
        let err = cmd
            .try_get_matches_from(["cmd", "--codec", "vp9"])
            .unwrap_err();
        assert!(err.to_string().contains("h264"), "expected hint: {err}");
    }

    #[test]
    fn parse_subcommand() {
        let schema = CommandSchema {
            name: "git".into(),
            about: "fake git".into(),
            args: vec![],
            subcommands: vec![
                CommandSchema {
                    name: "clone".into(),
                    about: "克隆仓库".into(),
                    args: vec![ArgSchema {
                        name: "url".into(),
                        about: "仓库地址".into(),
                        kind: ArgKind::Text,
                        required: true,
                        default: None,
                    }],
                    subcommands: vec![],
                },
                CommandSchema {
                    name: "push".into(),
                    about: "推送".into(),
                    args: vec![ArgSchema {
                        name: "force".into(),
                        about: "强制推送".into(),
                        kind: ArgKind::Flag,
                        required: false,
                        default: None,
                    }],
                    subcommands: vec![],
                },
            ],
        };
        let renderer = CliRenderer::new();
        let cmd = renderer.render(&schema);

        let m = cmd
            .try_get_matches_from(["git", "clone", "--url", "https://example.com"])
            .unwrap();
        let (sub_name, sub_m) = m.subcommand().unwrap();
        assert_eq!(sub_name, "clone");
        assert_eq!(
            sub_m.get_one::<String>("url").unwrap(),
            "https://example.com"
        );
    }

    // ─── 数字范围校验 ──────────────────────────────────

    #[test]
    fn number_range_validation() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        // 超出上限
        let err = cmd
            .try_get_matches_from([
                "transcode",
                "--env", "prod",
                "--quality", "100",
                "--input", "test.mp4",
            ])
            .unwrap_err();
        assert!(err.to_string().contains("<= 51"), "expected range error: {err}");
    }

    #[test]
    fn number_range_lower_bound() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let err = cmd
            .try_get_matches_from([
                "transcode",
                "--env", "prod",
                "--quality=-5",
                "--input", "test.mp4",
            ])
            .unwrap_err();
        assert!(err.to_string().contains(">= 0"), "expected range error: {err}");
    }

    // ─── 内置标志 ──────────────────────────────────────

    #[test]
    fn schema_flag_outputs_valid_json() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let m = cmd
            .try_get_matches_from(["transcode", "--schema"])
            .unwrap();
        assert!(m.get_flag("schema"));

        let json_str = serde_json::to_string_pretty(&schema).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn openai_tool_flag_recognized() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let m = cmd
            .try_get_matches_from(["transcode", "--openai-tool"])
            .unwrap();
        assert!(m.get_flag("openai-tool"));
    }

    #[test]
    fn anthropic_tool_flag_recognized() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let m = cmd
            .try_get_matches_from(["transcode", "--anthropic-tool"])
            .unwrap();
        assert!(m.get_flag("anthropic-tool"));
    }

    #[test]
    fn json_flag_recognized() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let m = cmd
            .try_get_matches_from(["transcode", "--json", "--env", "prod", "--input", "test.mp4"])
            .unwrap();
        assert!(m.get_flag("json"));
    }

    #[test]
    fn json_stream_flag_recognized() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let m = cmd
            .try_get_matches_from(["transcode", "--json-stream", "--env", "prod", "--input", "test.mp4"])
            .unwrap();
        assert!(m.get_flag("json-stream"));
        assert!(!m.get_flag("json"));
    }

    // ─── OutputFormat 提取 ──────────────────────────────

    #[test]
    fn output_format_defaults_to_human() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let m = cmd
            .try_get_matches_from(["transcode", "--env", "prod", "--input", "test.mp4"])
            .unwrap();
        assert_eq!(CliRenderer::output_format(&m), OutputFormat::Human);
    }

    #[test]
    fn output_format_json() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let m = cmd
            .try_get_matches_from(["transcode", "--json", "--env", "prod", "--input", "test.mp4"])
            .unwrap();
        assert_eq!(CliRenderer::output_format(&m), OutputFormat::Json);
    }

    #[test]
    fn output_format_json_stream() {
        let renderer = CliRenderer::new();
        let schema = transcode_schema();
        let cmd = renderer.render(&schema);

        let m = cmd
            .try_get_matches_from(["transcode", "--json-stream", "--env", "prod", "--input", "test.mp4"])
            .unwrap();
        assert_eq!(CliRenderer::output_format(&m), OutputFormat::JsonStream);
    }

    // ─── handle_builtin_flags ───────────────────────────

    #[test]
    fn handle_builtin_flags_returns_true_for_schema() {
        let schema = transcode_schema();
        let renderer = CliRenderer::new();
        let cmd = renderer.render(&schema);
        let m = cmd
            .try_get_matches_from(["transcode", "--schema"])
            .unwrap();
        assert!(CliRenderer::handle_builtin_flags(&schema, &m));
    }

    #[test]
    fn handle_builtin_flags_returns_false_without_flags() {
        let schema = transcode_schema();
        let renderer = CliRenderer::new();
        let cmd = renderer.render(&schema);
        let m = cmd
            .try_get_matches_from(["transcode", "--env", "prod", "--input", "test.mp4"])
            .unwrap();
        assert!(!CliRenderer::handle_builtin_flags(&schema, &m));
    }

    // ─── extract_args ──────────────────────────────────

    #[test]
    fn extract_args_basic() {
        let schema = transcode_schema();
        let renderer = CliRenderer::new();
        let cmd = renderer.render(&schema);
        let m = cmd
            .try_get_matches_from(["transcode", "--env", "prod", "--input", "test.mp4"])
            .unwrap();
        let args = CliRenderer::extract_args(&schema, &m);
        assert_eq!(args.get("env").unwrap(), &serde_json::json!("prod"));
        assert_eq!(args.get("input").unwrap(), &serde_json::json!("test.mp4"));
        assert_eq!(args.get("quality").unwrap(), &serde_json::json!(23.0)); // default
    }

    #[test]
    fn extract_args_includes_list() {
        let schema = transcode_schema();
        let renderer = CliRenderer::new();
        let cmd = renderer.render(&schema);
        let m = cmd
            .try_get_matches_from([
                "transcode",
                "--env", "prod",
                "--input", "test.mp4",
                "--tags", "drama",
                "--tags", "action",
            ])
            .unwrap();
        let args = CliRenderer::extract_args(&schema, &m);
        assert_eq!(
            args.get("tags").unwrap(),
            &serde_json::json!(["drama", "action"])
        );
    }

    #[test]
    fn extract_args_skips_builtin_flags() {
        let schema = transcode_schema();
        let renderer = CliRenderer::new();
        let cmd = renderer.render(&schema);
        let m = cmd
            .try_get_matches_from([
                "transcode",
                "--json-stream",
                "--env", "prod",
                "--input", "test.mp4",
            ])
            .unwrap();
        let args = CliRenderer::extract_args(&schema, &m);
        // json-stream 是内置标志，不应出现在 args 中
        assert!(!args.contains_key("json-stream"));
        assert!(args.contains_key("env"));
    }
}
