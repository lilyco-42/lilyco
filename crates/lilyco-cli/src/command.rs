//! schema → clap 的构造规则本体：一个参数怎么变成 `clap::Arg`、内置标志有哪些、
//! 值解析器怎么按 [`ArgKind`] 选。渲染入口与执行都不在这里，见 `renderer` / `registry`。

use std::ffi::OsStr;

use clap::{Arg, ArgAction, Command};

use lilyco_core::schema::{ArgKind, ArgSchema, CommandSchema};

/// clap 4 的内部类型（Str / Id / OsStr）仅接受 `&'static str`，
/// 不接受 `String`。用 Box::leak 将运行时字符串提升为 'static 是唯一方案。
/// 这些泄漏的值在进程生命周期内有效，无累积问题。
pub(crate) fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

pub(crate) fn build_command(schema: &CommandSchema) -> Command {
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

pub(crate) fn arg_to_clap(arg: &ArgSchema) -> Arg {
    let name: &'static str = leak_str(&arg.name);
    let mut a = Arg::new(name).long(name).help(arg.about.clone());

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
                let v: f64 = s.parse().map_err(|e| format!("invalid number: {e}"))?;
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
            a = a.num_args(1..).action(ArgAction::Append).value_parser(vp);
        }
    }

    a
}

pub(crate) fn arg_kind_to_value_parser(kind: &ArgKind) -> clap::builder::ValueParser {
    match kind {
        ArgKind::Flag => clap::builder::ValueParser::bool(),
        ArgKind::Text => clap::builder::ValueParser::string(),
        ArgKind::Number { min, max } => {
            let min = *min;
            let max = *max;
            clap::builder::ValueParser::new(move |s: &str| -> Result<f64, String> {
                let v: f64 = s.parse().map_err(|e| format!("invalid number: {e}"))?;
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

pub(crate) fn add_builtin_flags(cmd: Command) -> Command {
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
        Arg::new("openai-responses-tool")
            .long("openai-responses-tool")
            .help("打印 OpenAI Responses API 工具定义并退出（扁平格式）")
            .action(ArgAction::SetTrue)
            .exclusive(true),
    )
    .arg(
        Arg::new("openai-strict-tool")
            .long("openai-strict-tool")
            .help("打印 OpenAI strict mode 工具定义并退出（结构化输出）")
            .action(ArgAction::SetTrue)
            .exclusive(true),
    )
    .arg(
        Arg::new("gemini-tool")
            .long("gemini-tool")
            .help("打印 Gemini functionDeclarations 并退出")
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
    .arg(
        Arg::new("gui")
            .long("gui")
            .help("启动 Web GUI")
            .action(ArgAction::SetTrue)
            .exclusive(true),
    )
}

pub(crate) fn is_builtin_flag(name: &str) -> bool {
    matches!(
        name,
        "schema"
            | "openai-tool"
            | "openai-responses-tool"
            | "openai-strict-tool"
            | "gemini-tool"
            | "anthropic-tool"
            | "json"
            | "json-stream"
            | "gui"
    )
}

/// 将 `serde_json::Value` 转成 clap 可用的默认值（'static OsStr）
pub(crate) fn json_value_to_os_str(v: &serde_json::Value) -> Option<&'static OsStr> {
    let s: String = match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };
    // clap 的 OsStr 仅接受 &'static OsStr，无 String 或 Id 等价物
    Some(Box::leak(std::ffi::OsString::from(s).into_boxed_os_str()))
}
