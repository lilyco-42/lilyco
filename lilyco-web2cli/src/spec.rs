//! 声明式 `WebCliSpec` 模型及其向 `lilyco::__core::registry::Registry` 的编译器。
//!
//! 任何网页、OpenAPI 文档或 HAR 抓包都会被规整为 `WebCliSpec`（可存为 JSON/TOML），
//! 再通过 `to_registry()` 编译成标准的 Lilyco 四端注册表（CLI / TUI / Web / MCP）。

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use lilyco::__core::error::AppError;
use lilyco::__core::registry::{Handler, RegisteredCommand, Registry};
use lilyco::__core::safety::{SafetyPolicy, SafetyTier};
use lilyco::__core::schema::{ArgKind, ArgSchema, CommandSchema};
use serde::{Deserialize, Serialize};

use crate::exec::execute_web_command;
use crate::session::SessionManager;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArgLocation {
    Path,
    #[default]
    Query,
    Form,
    JsonBody,
    Header,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BodyEncoding {
    #[default]
    Query,
    Form,
    Multipart,
    Json,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "mode", content = "selector", rename_all = "snake_case")]
pub enum ResponseExtract {
    #[default]
    Auto,
    Json,
    Distill,
    Markdown,
    Links,
    Tables,
    Selector(String),
    Raw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebArgKind {
    Flag,
    Text,
    Number { min: Option<f64>, max: Option<f64> },
    Enum { values: Vec<String> },
    Path,
}

impl WebArgKind {
    pub fn to_arg_kind(&self) -> ArgKind {
        match self {
            Self::Flag => ArgKind::Flag,
            Self::Text => ArgKind::Text,
            Self::Number { min, max } => ArgKind::Number {
                min: *min,
                max: *max,
            },
            Self::Enum { values } => ArgKind::Enum {
                values: values.clone(),
            },
            Self::Path => ArgKind::Path { must_exist: false },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebArgSpec {
    /// CLI/MCP 参数名（kebab-case / snake_case）
    pub name: String,
    /// 实际发往 HTTP 的参数名
    pub param_name: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub location: ArgLocation,
    pub kind: WebArgKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebCommandSpec {
    pub name: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub method: HttpMethod,
    pub url: String,
    #[serde(default)]
    pub encoding: BodyEncoding,
    #[serde(default)]
    pub args: Vec<WebArgSpec>,
    #[serde(default)]
    pub static_params: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub extract: ResponseExtract,
    #[serde(default)]
    pub safety: SafetyTier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebCliSpec {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub commands: Vec<WebCommandSpec>,
}

impl WebCliSpec {
    pub fn from_str_auto(content: &str) -> Result<Self, AppError> {
        let trimmed = content.trim_start();
        if trimmed.starts_with('{') {
            serde_json::from_str(trimmed)
                .map_err(|e| AppError::InvalidInput(format!("解析 WebCliSpec JSON 失败: {e}")))
        } else {
            toml::from_str(trimmed)
                .map_err(|e| AppError::InvalidInput(format!("解析 WebCliSpec TOML 失败: {e}")))
        }
    }

    pub fn to_json_pretty(&self) -> Result<String, AppError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Runtime(format!("序列化 JSON 失败: {e}")))
    }

    pub fn to_toml_pretty(&self) -> Result<String, AppError> {
        toml::to_string_pretty(self)
            .map_err(|e| AppError::Runtime(format!("序列化 TOML 失败: {e}")))
    }

    /// 规整命令名与参数名，确保无重名、无非法字符
    pub fn normalize(&mut self) {
        let mut seen_cmds = HashSet::new();
        for cmd in &mut self.commands {
            let base = sanitize_ident(&cmd.name, "cmd");
            let mut candidate = base.clone();
            let mut idx = 2usize;
            while !seen_cmds.insert(candidate.clone()) {
                candidate = format!("{base}-{idx}");
                idx += 1;
            }
            cmd.name = candidate;

            let mut seen_args = HashSet::new();
            for arg in &mut cmd.args {
                let abase = sanitize_ident(&arg.name, "arg");
                let mut acand = abase.clone();
                let mut aidx = 2usize;
                while !seen_args.insert(acand.clone()) {
                    acand = format!("{abase}-{aidx}");
                    aidx += 1;
                }
                arg.name = acand;
            }
        }
    }

    /// 编译为 `lilyco` 标准 `Registry`（可在 CLI / TUI / Web / MCP 四端直接运行）。
    pub fn to_registry(
        &self,
        session: Arc<SessionManager>,
        policy: Option<Arc<dyn SafetyPolicy>>,
    ) -> Result<Registry, AppError> {
        let mut reg = Registry::new();
        if let Some(p) = policy {
            reg = reg.with_policy(p);
        }
        let base_url = self.base_url.clone();
        for cmd in &self.commands {
            let schema = cmd.to_command_schema();
            let cmd_spec = cmd.clone();
            let sess = Arc::clone(&session);
            let base = base_url.clone();
            let handler: Handler =
                Arc::new(move |ctx, args| execute_web_command(ctx, &sess, &base, &cmd_spec, args));
            let registered = RegisteredCommand::new(cmd.name.clone(), schema).with_handler(handler);
            reg.register(registered).map_err(|e| {
                AppError::InvalidInput(format!("注册命令 `{}` 失败: {e}", cmd.name))
            })?;
        }
        Ok(reg)
    }
}

impl WebCommandSpec {
    pub fn to_command_schema(&self) -> CommandSchema {
        let args = self
            .args
            .iter()
            .map(|a| ArgSchema {
                name: a.name.clone(),
                about: if a.about.is_empty() {
                    format!("HTTP {:?} parameter `{}`", a.location, a.param_name)
                } else {
                    a.about.clone()
                },
                kind: a.kind.to_arg_kind(),
                required: a.required,
                default: a.default.clone(),
            })
            .collect();
        CommandSchema {
            name: self.name.clone(),
            about: if self.about.is_empty() {
                format!("{} {}", self.method.as_str(), self.url)
            } else {
                self.about.clone()
            },
            args,
            subcommands: Vec::new(),
            safety: self.safety,
        }
    }
}

pub fn sanitize_ident(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        fallback.to_string()
    } else if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("{fallback}-{out}")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_spec_to_lilyco_registry_and_schema() {
        let mut spec = WebCliSpec {
            name: "demo".into(),
            base_url: "https://example.com".into(),
            about: "Demo site".into(),
            commands: vec![WebCommandSpec {
                name: "Search Docs!".into(),
                about: "Search documentation".into(),
                method: HttpMethod::Get,
                url: "/search".into(),
                encoding: BodyEncoding::Query,
                args: vec![WebArgSpec {
                    name: "query".into(),
                    param_name: "q".into(),
                    about: "Search keyword".into(),
                    location: ArgLocation::Query,
                    kind: WebArgKind::Text,
                    required: true,
                    default: None,
                }],
                static_params: BTreeMap::new(),
                headers: BTreeMap::new(),
                extract: ResponseExtract::Auto,
                safety: SafetyTier::ReadOnly,
            }],
        };
        spec.normalize();
        assert_eq!(spec.commands[0].name, "search-docs");

        let sess = Arc::new(SessionManager::with_dir("test", std::env::temp_dir()));
        let reg = spec.to_registry(sess, None).expect("registry");
        let cmd = reg.get("search-docs").expect("command registered");
        assert_eq!(cmd.schema.args.len(), 1);
        assert!(cmd.schema.args[0].required);
    }
}
