use serde::{Deserialize, Serialize};

/// 一个参数的完整机器可读描述
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgSchema {
    pub name: String,
    pub about: String,
    pub kind: ArgKind,
    pub required: bool,
    pub default: Option<serde_json::Value>,
}

/// 参数类型的枚举，决定三端如何渲染输入控件
///
/// CLI: Flag → --flag, Text → --name <value>, Number → --count <num>,
/// Enum → --mode <choice>, Path → --file <path>, List → --tag a --tag b
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ArgKind {
    /// bool → --flag
    Flag,
    /// String → --name <value>
    Text,
    /// 数字，带可选范围
    Number { min: Option<f64>, max: Option<f64> },
    /// 枚举，固定可选值
    Enum { values: Vec<String> },
    /// 文件路径
    Path { must_exist: bool },
    /// Vec\<T\> → --tag a --tag b
    List { item: Box<ArgKind> },
}

/// 一个命令（或子命令）的完整描述
///
/// 这是整个框架的核心数据结构，CLI / TUI / GUI 都从它派生。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSchema {
    pub name: String,
    pub about: String,
    pub args: Vec<ArgSchema>,
    pub subcommands: Vec<CommandSchema>,
}

impl CommandSchema {
    /// 导出为 JSON Schema（用于 AI Function Calling）
    pub fn to_json_schema(&self) -> serde_json::Value {
        let mut props = serde_json::Map::new();
        let mut required: Vec<serde_json::Value> = Vec::new();

        for arg in &self.args {
            props.insert(arg.name.to_string(), arg_kind_to_json_schema(&arg.kind));
            if arg.required {
                required.push(serde_json::Value::String(arg.name.to_string()));
            }
        }

        let mut schema = serde_json::json!({
            "type": "object",
            "properties": props,
        });

        if !required.is_empty() {
            schema["required"] = serde_json::Value::Array(required);
        }

        schema
    }

    /// 导出为 OpenAI function calling 工具定义格式
    pub fn to_openai_tool(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.about,
                "parameters": self.to_json_schema(),
            }
        })
    }

    /// 导出为 Anthropic tool_use 工具定义格式
    pub fn to_anthropic_tool(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "description": self.about,
            "input_schema": self.to_json_schema(),
        })
    }
}

// ── helpers ────────────────────────────────────────────────

fn arg_kind_to_json_schema(kind: &ArgKind) -> serde_json::Value {
    match kind {
        ArgKind::Flag => {
            serde_json::json!({ "type": "boolean" })
        }
        ArgKind::Text => {
            serde_json::json!({ "type": "string" })
        }
        ArgKind::Number { min, max } => {
            let mut s = serde_json::json!({ "type": "number" });
            if let Some(v) = min {
                s["minimum"] = serde_json::json!(v);
            }
            if let Some(v) = max {
                s["maximum"] = serde_json::json!(v);
            }
            s
        }
        ArgKind::Enum { values } => {
            serde_json::json!({
                "type": "string",
                "enum": values,
            })
        }
        ArgKind::Path { .. } => {
            serde_json::json!({ "type": "string" })
        }
        ArgKind::List { item } => {
            serde_json::json!({
                "type": "array",
                "items": arg_kind_to_json_schema(item),
            })
        }
    }
}

// ── ValueEnum trait ─────────────────────────────────────────

/// 为枚举类型提供可发现的值列表和字符串互转
///
/// 由 `#[derive(ValueEnum)]` 过程宏自动实现。
pub trait ValueEnum: Sized {
    /// 返回所有可能的字符串表示
    fn variants() -> Vec<&'static str>;
    /// 从字符串反序列化
    fn from_str(s: &str) -> Option<Self>;
}
