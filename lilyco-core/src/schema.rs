use serde::{Deserialize, Serialize};

use crate::error::AppError;

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
            let mut prop = arg_kind_to_json_schema(&arg.kind);
            prop["description"] = serde_json::json!(arg.about);
            props.insert(arg.name.to_string(), prop);
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

    /// 导出为 OpenAI Responses API 工具定义（扁平格式）
    ///
    /// 与 Chat Completions 的嵌套 `{"type":"function","function":{…}}` 不同，
    /// Responses API 的 tools 数组直接使用 `{"type":"function","name",…}`。
    pub fn to_openai_responses_tool(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "name": self.name,
            "description": self.about,
            "parameters": self.to_json_schema(),
            "strict": false,
        })
    }

    /// 导出为 OpenAI strict mode（结构化输出）工具定义
    ///
    /// strict:true 的硬性要求（OpenAI 现状）：全部属性进 `required`、
    /// 每个对象 `additionalProperties:false`、~19 个约束关键词（minimum/
    /// maximum/default 等）被剥离。非 required 字段以 `anyOf [T, null]`
    /// 表达可空。参数格式同 [`to_json_schema`] 的净化版。
    pub fn to_openai_tool_strict(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.about,
                "parameters": self.strict_json_schema(),
                "strict": true,
            }
        })
    }

    /// 导出为 Gemini functionDeclarations（OpenAPI 3.0 Schema 子集）
    ///
    /// Gemini 的 Schema proto 不含 `default` → 剥离；minimum/maximum/
    /// enum/required 等 OpenAPI 关键词保留。
    pub fn to_gemini_tool(&self) -> serde_json::Value {
        let mut parameters = self.to_json_schema();
        strip_key_recursively(&mut parameters, "default");
        serde_json::json!({
            "functionDeclarations": [{
                "name": self.name,
                "description": self.about,
                "parameters": parameters,
            }]
        })
    }

    /// [`to_json_schema`] 的 OpenAI strict mode 净化版
    ///
    /// 规则：剥离不支持关键词；每个对象 `additionalProperties:false`；
    /// 全部属性进 `required`；非 required 字段包 `anyOf [T, null]`。
    pub fn strict_json_schema(&self) -> serde_json::Value {
        let mut props = serde_json::Map::new();
        let mut required: Vec<serde_json::Value> = Vec::new();

        for arg in &self.args {
            let mut prop = arg_kind_to_json_schema(&arg.kind);
            sanitize_strict(&mut prop);
            if !arg.required {
                prop = serde_json::json!({ "anyOf": [prop, { "type": "null" }] });
            }
            props.insert(arg.name.clone(), prop);
            required.push(serde_json::Value::String(arg.name.clone()));
        }

        serde_json::json!({
            "type": "object",
            "properties": props,
            "required": required,
            "additionalProperties": false,
        })
    }

    /// 校验一组参数是否符合本 schema（三端共用的唯一校验实现）
    ///
    /// 规则与 CLI clap 渲染语义一致：
    /// - required 且无 default：缺省 / null → 错误
    /// - Text / Path：必须是 string；required 时空串视为未填写
    /// - Path must_exist：路径必须存在
    /// - Number：必须是数字；min/max 范围校验
    /// - Enum：值必须在固定可选值内
    /// - List：必须是数组，逐项按 item 类型校验
    ///
    /// 缺省但非 required（或带 default）→ 放行，由 handler 侧回退。
    /// MCP / Web 直接执行此校验；CLI 已由 clap 在解析层校验。
    pub fn validate_args(&self, args: &serde_json::Value) -> Result<(), AppError> {
        for arg in &self.args {
            let value = args.get(&arg.name).filter(|v| !v.is_null());
            match value {
                None => {
                    if arg.required && arg.default.is_none() {
                        return Err(AppError::InvalidArg(format!("缺少必填参数: {}", arg.name)));
                    }
                }
                Some(v) => {
                    if let Err(msg) = validate_kind(arg, &arg.kind, v) {
                        return Err(AppError::InvalidArg(msg));
                    }
                }
            }
        }
        Ok(())
    }
}

/// 单个参数值校验（内部用 String 错误，便于 List 元素带下标包装）
fn validate_kind(arg: &ArgSchema, kind: &ArgKind, v: &serde_json::Value) -> Result<(), String> {
    let name = &arg.name;
    match kind {
        ArgKind::Flag => {
            if v.is_boolean() {
                Ok(())
            } else {
                Err(format!("参数 {name} 需要布尔值"))
            }
        }
        ArgKind::Text => {
            let s = v
                .as_str()
                .ok_or_else(|| format!("参数 {name} 需要字符串"))?;
            if arg.required && s.is_empty() {
                return Err(format!("必填参数 {name} 不能为空"));
            }
            Ok(())
        }
        ArgKind::Number { min, max } => {
            let n = v.as_f64().ok_or_else(|| format!("参数 {name} 需要数字"))?;
            if let Some(lo) = min {
                if n < *lo {
                    return Err(format!("参数 {name} 需 >= {lo}，得到 {n}"));
                }
            }
            if let Some(hi) = max {
                if n > *hi {
                    return Err(format!("参数 {name} 需 <= {hi}，得到 {n}"));
                }
            }
            Ok(())
        }
        ArgKind::Enum { values } => {
            let s = v
                .as_str()
                .ok_or_else(|| format!("参数 {name} 需要字符串"))?;
            if values.iter().any(|c| c == s) {
                Ok(())
            } else {
                Err(format!("参数 {name} 的值 `{s}` 不在可选范围 {values:?}"))
            }
        }
        ArgKind::Path { must_exist } => {
            let s = v
                .as_str()
                .ok_or_else(|| format!("参数 {name} 需要字符串"))?;
            if arg.required && s.is_empty() {
                return Err(format!("必填参数 {name} 不能为空"));
            }
            if *must_exist && !std::path::Path::new(s).exists() {
                return Err(format!("路径不存在: {name} = {s}"));
            }
            Ok(())
        }
        ArgKind::List { item } => {
            let arr = v
                .as_array()
                .ok_or_else(|| format!("参数 {name} 需要数组"))?;
            if arg.required && arr.is_empty() {
                return Err(format!("必填参数 {name} 不能为空"));
            }
            for (i, el) in arr.iter().enumerate() {
                validate_kind(arg, item, el).map_err(|m| format!("{name}[{i}]: {m}"))?;
            }
            Ok(())
        }
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

// ── AI 协议导出辅助 ────────────────────────────────────────

/// strict mode 不支持、需剥离的关键词（OpenAI 结构化输出现状，~19 个的子集）
const STRICT_UNSUPPORTED: &[&str] = &[
    "minimum",
    "maximum",
    "default",
    "multipleOf",
    "minLength",
    "maxLength",
    "pattern",
    "format",
    "minItems",
    "maxItems",
    "exclusiveMinimum",
    "exclusiveMaximum",
];

/// 递归净化为 strict mode 合规形态：剥关键词、对象加 additionalProperties:false、
/// 全属性进 required
fn sanitize_strict(schema: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = schema {
        for k in STRICT_UNSUPPORTED {
            map.remove(*k);
        }
        let is_object = map.get("type").and_then(|t| t.as_str()) == Some("object");
        if is_object {
            map.insert("additionalProperties".into(), serde_json::json!(false));
        }
        if let Some(items) = map.get_mut("items") {
            sanitize_strict(items);
        }
        if is_object {
            let keys = {
                let props = map.get_mut("properties").and_then(|p| p.as_object_mut());
                if let Some(props) = props {
                    for v in props.values_mut() {
                        sanitize_strict(v);
                    }
                    Some(props.keys().cloned().collect::<Vec<_>>())
                } else {
                    None
                }
            };
            if let Some(keys) = keys {
                map.insert("required".into(), serde_json::json!(keys));
            }
        }
    }
}

/// 递归删除指定关键词（Gemini Schema proto 无 default 等）
fn strip_key_recursively(schema: &mut serde_json::Value, key: &str) {
    if let serde_json::Value::Object(map) = schema {
        map.remove(key);
        for v in map.values_mut() {
            strip_key_recursively(v, key);
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

// ── validate_args 测试 ─────────────────────────────────────

#[cfg(test)]
mod validate_tests {
    use super::*;

    fn schema_with(args: Vec<ArgSchema>) -> CommandSchema {
        CommandSchema {
            name: "demo".into(),
            about: "demo".into(),
            args,
            subcommands: vec![],
        }
    }

    fn arg(name: &str, kind: ArgKind, required: bool) -> ArgSchema {
        ArgSchema {
            name: name.into(),
            about: name.into(),
            kind,
            required,
            default: None,
        }
    }

    #[test]
    fn missing_required_arg_is_rejected() {
        let s = schema_with(vec![arg("input", ArgKind::Text, true)]);
        let err = s
            .validate_args(&serde_json::json!({}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("缺少必填参数"), "{err}");
        assert!(err.contains("input"), "{err}");
    }

    #[test]
    fn missing_optional_arg_passes() {
        let s = schema_with(vec![arg("tag", ArgKind::Text, false)]);
        assert!(s.validate_args(&serde_json::json!({})).is_ok());
    }

    #[test]
    fn missing_required_with_default_passes() {
        let mut a = arg(
            "count",
            ArgKind::Number {
                min: None,
                max: None,
            },
            true,
        );
        a.default = Some(serde_json::json!(5));
        let s = schema_with(vec![a]);
        assert!(s.validate_args(&serde_json::json!({})).is_ok());
    }

    #[test]
    fn null_value_treated_as_missing() {
        let s = schema_with(vec![arg("input", ArgKind::Text, true)]);
        assert!(s
            .validate_args(&serde_json::json!({ "input": null }))
            .is_err());
    }

    #[test]
    fn empty_required_text_rejected() {
        let s = schema_with(vec![arg("input", ArgKind::Text, true)]);
        let err = s
            .validate_args(&serde_json::json!({ "input": "" }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("不能为空"), "{err}");
    }

    #[test]
    fn number_range_enforced() {
        let s = schema_with(vec![arg(
            "quality",
            ArgKind::Number {
                min: Some(0.0),
                max: Some(51.0),
            },
            false,
        )]);
        assert!(s
            .validate_args(&serde_json::json!({ "quality": 23 }))
            .is_ok());
        assert!(s
            .validate_args(&serde_json::json!({ "quality": 100 }))
            .is_err());
        assert!(s
            .validate_args(&serde_json::json!({ "quality": -1 }))
            .is_err());
        // 字符串数字不通过（类型错误）
        assert!(s
            .validate_args(&serde_json::json!({ "quality": "23" }))
            .is_err());
    }

    #[test]
    fn enum_membership_enforced() {
        let s = schema_with(vec![arg(
            "codec",
            ArgKind::Enum {
                values: vec!["h264".into(), "h265".into()],
            },
            false,
        )]);
        assert!(s
            .validate_args(&serde_json::json!({ "codec": "h265" }))
            .is_ok());
        let err = s
            .validate_args(&serde_json::json!({ "codec": "vp9" }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("vp9"), "{err}");
    }

    #[test]
    fn path_must_exist_enforced() {
        let s = schema_with(vec![arg("file", ArgKind::Path { must_exist: true }, false)]);
        assert!(s
            .validate_args(&serde_json::json!({ "file": "definitely/not/exist.bin" }))
            .is_err());
        // must_exist = false 不检查
        let s2 = schema_with(vec![arg(
            "file",
            ArgKind::Path { must_exist: false },
            false,
        )]);
        assert!(s2
            .validate_args(&serde_json::json!({ "file": "definitely/not/exist.bin" }))
            .is_ok());
        // 真实存在的路径通过
        let s3 = schema_with(vec![arg("file", ArgKind::Path { must_exist: true }, false)]);
        assert!(s3
            .validate_args(&serde_json::json!({ "file": env!("CARGO_MANIFEST_DIR") }))
            .is_ok());
    }

    #[test]
    fn list_items_validated_recursively() {
        let s = schema_with(vec![arg(
            "tags",
            ArgKind::List {
                item: Box::new(ArgKind::Text),
            },
            false,
        )]);
        assert!(s
            .validate_args(&serde_json::json!({ "tags": ["a", "b"] }))
            .is_ok());
        assert!(s
            .validate_args(&serde_json::json!({ "tags": "not-an-array" }))
            .is_err());
        // 数字元素不符合 Text 类型，报错带下标
        let err = s
            .validate_args(&serde_json::json!({ "tags": ["a", 3] }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("tags[1]"), "{err}");
    }

    #[test]
    fn number_list_range_validated() {
        let s = schema_with(vec![arg(
            "nums",
            ArgKind::List {
                item: Box::new(ArgKind::Number {
                    min: Some(0.0),
                    max: Some(10.0),
                }),
            },
            false,
        )]);
        assert!(s
            .validate_args(&serde_json::json!({ "nums": [1, 10] }))
            .is_ok());
        assert!(s
            .validate_args(&serde_json::json!({ "nums": [1, 99] }))
            .is_err());
    }

    #[test]
    fn flag_requires_boolean() {
        let s = schema_with(vec![arg("verbose", ArgKind::Flag, false)]);
        assert!(s
            .validate_args(&serde_json::json!({ "verbose": true }))
            .is_ok());
        assert!(s
            .validate_args(&serde_json::json!({ "verbose": "yes" }))
            .is_err());
    }

    // ─── AI 协议导出（Responses / strict / Gemini） ─────

    fn tool_schema() -> CommandSchema {
        schema_with(vec![
            arg("input", ArgKind::Text, true),
            arg(
                "quality",
                ArgKind::Number {
                    min: Some(0.0),
                    max: Some(51.0),
                },
                false,
            ),
            arg(
                "codec",
                ArgKind::Enum {
                    values: vec!["h264".into()],
                },
                false,
            ),
        ])
    }

    #[test]
    fn openai_responses_tool_is_flattened() {
        let t = tool_schema().to_openai_responses_tool();
        assert_eq!(t["type"], "function");
        assert_eq!(t["name"], "demo");
        // 扁平格式：parameters 在顶层，无嵌套 function 对象
        assert!(t["parameters"].is_object());
        assert!(t.get("function").is_none());
        assert_eq!(t["strict"], false);
    }

    #[test]
    fn strict_tool_sanitizes_constraints_and_requires_all() {
        let t = tool_schema().to_openai_tool_strict();
        assert_eq!(t["function"]["strict"], true);
        let params = &t["function"]["parameters"];
        // 全属性进 required
        let req = params["required"].as_array().unwrap();
        assert_eq!(req.len(), 3, "all fields required in strict: {req:?}");
        // 约束关键词被剥离
        let raw = serde_json::to_string(params).unwrap();
        assert!(
            !raw.contains("minimum") && !raw.contains("maximum"),
            "{raw}"
        );
        assert!(!raw.contains("\"default\""), "{raw}");
        // 每个对象 additionalProperties: false
        assert_eq!(params["additionalProperties"], false);
        // 非 required 字段以 anyOf [T, null] 表达可空
        assert!(params["properties"]["quality"]["anyOf"].is_array(), "{raw}");
        // required 字段保持原类型
        assert_eq!(params["properties"]["input"]["type"], "string");
    }

    #[test]
    fn gemini_tool_strips_default_and_keeps_openapi_keywords() {
        let mut s = tool_schema();
        s.args[0].about = "输入文件".into();
        let t = s.to_gemini_tool();
        let decl = &t["functionDeclarations"][0];
        assert_eq!(decl["name"], "demo");
        let raw = serde_json::to_string(decl).unwrap();
        // default 剥离；minimum（OpenAPI 关键词）保留
        assert!(!raw.contains("\"default\""), "{raw}");
        // to_json_schema 注入 description
        assert!(raw.contains("输入文件"), "{raw}");
    }

    #[test]
    fn json_schema_includes_per_arg_description() {
        let mut s = tool_schema();
        s.args[0].about = "输入文件".into();
        let js = s.to_json_schema();
        assert_eq!(js["properties"]["input"]["description"], "输入文件");
    }

    #[test]
    fn full_args_pass() {
        let s = schema_with(vec![
            arg("input", ArgKind::Text, true),
            arg(
                "codec",
                ArgKind::Enum {
                    values: vec!["h264".into()],
                },
                false,
            ),
            arg(
                "quality",
                ArgKind::Number {
                    min: Some(0.0),
                    max: Some(51.0),
                },
                false,
            ),
            arg("dry", ArgKind::Flag, false),
        ]);
        assert!(s
            .validate_args(&serde_json::json!({
                "input": "a.mp4",
                "codec": "h264",
                "quality": 23,
                "dry": false
            }))
            .is_ok());
    }
}
