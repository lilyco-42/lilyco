//! CLI 后端的全部单测：渲染形状、参数解析、范围校验、内置标志、参数提取，
//! 以及多命令（可见 / 别名 / 隐藏）语义 —— 与 TUI / Web / MCP 那张对照表一一对应。

use lilyco_core::prelude::*;

use crate::registry::{build_registry_command, resolve_registry_command};
use crate::renderer::CliRenderer;

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
        safety: lilyco_core::safety::SafetyTier::ReadOnly,
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

    let matches = cmd
        .try_get_matches_from([
            "transcode",
            "--env",
            "prod",
            "--quality",
            "23",
            "--input",
            "test.mp4",
        ])
        .unwrap();

    assert_eq!(matches.get_one::<String>("env").unwrap(), "prod");
    assert_eq!(*matches.get_one::<f64>("quality").unwrap(), 23.0);
}

#[test]
fn parse_with_defaults() {
    let renderer = CliRenderer::new();
    let schema = transcode_schema();
    let cmd = renderer.render(&schema);

    let matches = cmd
        .try_get_matches_from(["transcode", "--env", "prod", "--input", "test.mp4"])
        .unwrap();

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
        safety: lilyco_core::safety::SafetyTier::ReadOnly,
    };
    let renderer = CliRenderer::new();
    let cmd = renderer.render(&schema);

    let m = cmd
        .clone()
        .try_get_matches_from(["cmd", "--verbose"])
        .unwrap();
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
        safety: lilyco_core::safety::SafetyTier::ReadOnly,
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
        safety: lilyco_core::safety::SafetyTier::ReadOnly,
    };
    let renderer = CliRenderer::new();
    let cmd = renderer.render(&schema);

    let m = cmd
        .clone()
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
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
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
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
            },
        ],
        safety: lilyco_core::safety::SafetyTier::ReadOnly,
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
            "--env",
            "prod",
            "--quality",
            "100",
            "--input",
            "test.mp4",
        ])
        .unwrap_err();
    assert!(
        err.to_string().contains("<= 51"),
        "expected range error: {err}"
    );
}

#[test]
fn number_range_lower_bound() {
    let renderer = CliRenderer::new();
    let schema = transcode_schema();
    let cmd = renderer.render(&schema);

    let err = cmd
        .try_get_matches_from([
            "transcode",
            "--env",
            "prod",
            "--quality=-5",
            "--input",
            "test.mp4",
        ])
        .unwrap_err();
    assert!(
        err.to_string().contains(">= 0"),
        "expected range error: {err}"
    );
}

// ─── 内置标志 ──────────────────────────────────────

#[test]
fn schema_flag_outputs_valid_json() {
    let renderer = CliRenderer::new();
    let schema = transcode_schema();
    let cmd = renderer.render(&schema);

    let m = cmd.try_get_matches_from(["transcode", "--schema"]).unwrap();
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
fn new_protocol_flags_recognized() {
    let schema = transcode_schema();
    let renderer = CliRenderer::new();
    for flag in ["openai-responses-tool", "openai-strict-tool", "gemini-tool"] {
        let cmd = renderer.render(&schema);
        let m = cmd
            .try_get_matches_from(["transcode", &format!("--{flag}")])
            .unwrap_or_else(|e| panic!("{flag}: {e}"));
        assert!(m.get_flag(flag), "{flag} not set");
        assert!(
            CliRenderer::handle_builtin_flags(&schema, &m),
            "{flag} not handled"
        );
    }
}

#[test]
fn json_flag_recognized() {
    let renderer = CliRenderer::new();
    let schema = transcode_schema();
    let cmd = renderer.render(&schema);

    let m = cmd
        .try_get_matches_from([
            "transcode",
            "--json",
            "--env",
            "prod",
            "--input",
            "test.mp4",
        ])
        .unwrap();
    assert!(m.get_flag("json"));
}

#[test]
fn json_stream_flag_recognized() {
    let renderer = CliRenderer::new();
    let schema = transcode_schema();
    let cmd = renderer.render(&schema);

    let m = cmd
        .try_get_matches_from([
            "transcode",
            "--json-stream",
            "--env",
            "prod",
            "--input",
            "test.mp4",
        ])
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
        .try_get_matches_from([
            "transcode",
            "--json",
            "--env",
            "prod",
            "--input",
            "test.mp4",
        ])
        .unwrap();
    assert_eq!(CliRenderer::output_format(&m), OutputFormat::Json);
}

#[test]
fn output_format_json_stream() {
    let renderer = CliRenderer::new();
    let schema = transcode_schema();
    let cmd = renderer.render(&schema);

    let m = cmd
        .try_get_matches_from([
            "transcode",
            "--json-stream",
            "--env",
            "prod",
            "--input",
            "test.mp4",
        ])
        .unwrap();
    assert_eq!(CliRenderer::output_format(&m), OutputFormat::JsonStream);
}

// ─── handle_builtin_flags ───────────────────────────

#[test]
fn handle_builtin_flags_returns_true_for_schema() {
    let schema = transcode_schema();
    let renderer = CliRenderer::new();
    let cmd = renderer.render(&schema);
    let m = cmd.try_get_matches_from(["transcode", "--schema"]).unwrap();
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
            "--env",
            "prod",
            "--input",
            "test.mp4",
            "--tags",
            "drama",
            "--tags",
            "action",
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
            "--env",
            "prod",
            "--input",
            "test.mp4",
        ])
        .unwrap();
    let args = CliRenderer::extract_args(&schema, &m);
    // json-stream 是内置标志，不应出现在 args 中
    assert!(!args.contains_key("json-stream"));
    assert!(args.contains_key("env"));
}

// ─── Registry 多命令 ───────────────────────────────

/// 单参数（--x）的最小 schema
fn simple_schema(name: &str) -> CommandSchema {
    CommandSchema {
        name: name.into(),
        about: "test command".into(),
        args: vec![ArgSchema {
            name: "x".into(),
            about: "arg x".into(),
            kind: ArgKind::Text,
            required: false,
            default: None,
        }],
        subcommands: vec![],
        safety: lilyco_core::safety::SafetyTier::ReadOnly,
    }
}

/// 构建多命令注册表：可见 / 别名 / 隐藏各一条
fn multi_command_registry() -> Registry {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::new("ping", simple_schema("ping")))
        .unwrap();
    reg.register(
        RegisteredCommand::new("img-compress", simple_schema("img-compress")).alias("imgc"),
    )
    .unwrap();
    reg.register(RegisteredCommand::new("secret", simple_schema("secret")).hidden(true))
        .unwrap();
    reg
}

#[test]
fn registry_command_contains_all_schemas() {
    let reg = multi_command_registry();
    let root = build_registry_command("tool", &reg);
    assert!(root.find_subcommand("ping").is_some());
    assert!(root.find_subcommand("img-compress").is_some());
    assert!(root.find_subcommand("secret").is_some());
}

#[test]
fn hidden_command_hidden_but_callable() {
    let reg = multi_command_registry();
    let root = build_registry_command("tool", &reg);
    let secret = root.find_subcommand("secret").unwrap();
    assert!(secret.is_hide_set(), "hidden command must be hide(true)");

    // 仍可调用
    let m = root
        .try_get_matches_from(["tool", "secret", "--x", "1"])
        .unwrap();
    let (name, _) = m.subcommand().unwrap();
    assert_eq!(name, "secret");
}

#[test]
fn alias_subcommand_resolves_to_canonical() {
    let reg = multi_command_registry();
    let root = build_registry_command("tool", &reg);
    let m = root
        .try_get_matches_from(["tool", "imgc", "--x", "v"])
        .unwrap();
    let (name, _) = m.subcommand().unwrap();
    // 无论 clap 返回别名还是规范名，registry 解析都命中规范命令
    let resolved = resolve_registry_command(&reg, name).unwrap();
    assert_eq!(resolved.name, "img-compress");
}

#[test]
fn resolve_falls_back_to_schema_name() {
    // 声明式加载：注册名与 schema.name 不一致也能命中
    let mut reg = Registry::new();
    let mut entry = RegisteredCommand::new("reg-name", simple_schema("schema-name"));
    entry.schema.name = "schema-name".into();
    reg.register(entry).unwrap();
    let resolved = resolve_registry_command(&reg, "schema-name").unwrap();
    assert_eq!(resolved.schema.name, "schema-name");
}

#[test]
fn root_schema_flag_parses() {
    let reg = multi_command_registry();
    let root = build_registry_command("tool", &reg);
    let m = root.try_get_matches_from(["tool", "--schema"]).unwrap();
    assert!(m.get_flag("schema"));
    // 清单 JSON 可解析且包含全部命令
    let json = reg.to_json();
    assert!(json.is_array());
}

#[test]
fn bare_invocation_is_error() {
    let reg = multi_command_registry();
    let root = build_registry_command("tool", &reg);
    assert!(root.try_get_matches_from(["tool"]).is_err());
}

#[test]
fn subcommand_builtin_flags_still_work() {
    let reg = multi_command_registry();
    let root = build_registry_command("tool", &reg);
    let m = root
        .try_get_matches_from(["tool", "ping", "--json-stream"])
        .unwrap();
    let (_, sub) = m.subcommand().unwrap();
    assert!(sub.get_flag("json-stream"));
    assert_eq!(CliRenderer::output_format(sub), OutputFormat::JsonStream);
}

#[test]
fn subcommand_args_extracted_for_handler() {
    let reg = multi_command_registry();
    let root = build_registry_command("tool", &reg);
    let m = root
        .try_get_matches_from(["tool", "imgc", "--x", "prod"])
        .unwrap();
    let (name, sub) = m.subcommand().unwrap();
    let entry = resolve_registry_command(&reg, name).unwrap();
    let args = CliRenderer::extract_args(&entry.schema, sub);
    assert_eq!(args.get("x").unwrap(), &serde_json::json!("prod"));
}
