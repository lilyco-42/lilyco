//! `CliRenderer`：`CommandSchema` → `clap::Command` 的那一层，以及解析结果的后处理
//! （内置标志、参数提取、输出格式判定）。命令的构造规则本体在 [`crate::command`]。

use clap::Command;

use lilyco_core::prelude::*;

use crate::command::{build_command, is_builtin_flag};

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
            println!("{}", serde_json::to_string_pretty(&schema).unwrap());
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
        if matches.get_flag("openai-responses-tool") {
            println!(
                "{}",
                serde_json::to_string_pretty(&schema.to_openai_responses_tool()).unwrap()
            );
            return true;
        }
        if matches.get_flag("openai-strict-tool") {
            println!(
                "{}",
                serde_json::to_string_pretty(&schema.to_openai_tool_strict()).unwrap()
            );
            return true;
        }
        if matches.get_flag("gemini-tool") {
            println!(
                "{}",
                serde_json::to_string_pretty(&schema.to_gemini_tool()).unwrap()
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
                        map.insert(name.clone(), serde_json::json!(v));
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
