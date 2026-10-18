//! Anthropic Messages API client.
//!
//! Everything cc-switch stores is Anthropic-shaped (`ANTHROPIC_BASE_URL` +
//! `ANTHROPIC_AUTH_TOKEN`), and its local proxy on 127.0.0.1 speaks the same
//! protocol. So this is the client that actually reaches those providers.
//!
//! `POST {base}/v1/messages` with `x-api-key` and `anthropic-version`.
//! As with the OpenAI client, the wire format is built and parsed by pure
//! functions so it can be tested without a network.

use ::std::string::{String, ToString};
use ::std::vec::Vec;

use serde_json::{json, Value};

use crate::agent::llm::{Call, Decision, Llm, Step, MAX_SPEECH};
use crate::agent::registry::Registry;

/// System prompt shared by the Anthropic path.
pub const SYSTEM: &str = "You are a coding agent. You may call at most one tool per turn.\n\
The tool argument is a single string:\n\
- read_file: <relative path>\n\
- list_dir: <relative path or empty>\n\
- edit_file: <path>|<old text>|<new text>  (old must appear exactly once)\n\
Call a tool only if it moves the goal forward. When the goal is met, reply with a short final answer instead of calling a tool.";

/// Anthropic tool definitions use `input_schema` rather than `parameters`.
fn tool_def(name: &str, summary: &str) -> Value {
    json!({
        "name": name,
        "description": summary,
        "input_schema": input_schema()
    })
}

fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "args": arg_schema() },
        "required": ["args"]
    })
}

fn arg_schema() -> Value {
    json!({ "type": "string", "description": "single string argument, see the system prompt" })
}

fn user_message(ctx: Decision<'_>) -> String {
    let seen = if ctx.observation.is_empty() {
        "(none)"
    } else {
        ctx.observation
    };
    ::std::format!(
        "Goal: {}\n\nPrevious observation:\n{}\n\nStep: {}",
        ctx.goal,
        seen,
        ctx.step
    )
}

/// Build a `POST /v1/messages` body.
pub fn build_request(model: &str, ctx: Decision<'_>, tools: &[(&str, &str)], system: &str) -> String {
    let defs: Vec<Value> = tools
        .iter()
        .map(|(name, summary)| tool_def(name, summary))
        .collect();
    let body = json!({
        "model": model,
        "max_tokens": 2048,
        "system": system,
        "messages": [ { "role": "user", "content": user_message(ctx) } ],
        "tools": defs
    });
    body.to_string()
}

/// Parse a response into the next [`Step`].
pub fn parse_step(raw: &str) -> Step {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Step::Done;
    };
    let Some(blocks) = value["content"].as_array() else {
        return Step::Done;
    };
    for block in blocks {
        if block["type"] == "tool_use" {
            let Some(name) = block["name"].as_str() else {
                continue;
            };
            if let Ok(call) = Call::new(name, &input_to_args(&block["input"])) {
                return Step::Call(call);
            }
        }
    }
    for block in blocks {
        if block["type"] == "text" {
            if let Some(text) = block["text"].as_str() {
                let mut buf = heapless::String::<MAX_SPEECH>::new();
                let _ = buf.push_str(clip(text, MAX_SPEECH));
                return Step::Say(buf);
            }
        }
    }
    Step::Done
}

/// Anthropic returns a typed object; flatten it to our `args` convention.
pub fn input_to_args(input: &Value) -> String {
    if let Some(args) = input.get("args").and_then(Value::as_str) {
        return args.to_string();
    }
    let path = input.get("path").and_then(Value::as_str);
    let old = input.get("old").and_then(Value::as_str);
    let new = input.get("new").and_then(Value::as_str);
    match (path, old, new) {
        (Some(p), Some(o), Some(n)) => ::std::format!("{p}|{o}|{n}"),
        (Some(p), _, _) => p.to_string(),
        _ => input.to_string(),
    }
}

fn clip(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// Connection settings for an Anthropic-compatible endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Base URL without a trailing slash, e.g. `http://127.0.0.1:15721`.
    pub base_url: String,
    /// `x-api-key` value.
    pub api_key: String,
    /// Model id.
    pub model: String,
    /// System prompt.
    pub system: String,
}

impl Config {
    /// `POST <base>/v1/messages`.
    pub fn endpoint(&self) -> String {
        ::std::format!("{}/v1/messages", self.base_url.trim_end_matches('/'))
    }
}

/// A model reached over the Anthropic Messages API.
pub struct AnthropicLlm {
    config: Config,
    registry: &'static Registry,
}

impl AnthropicLlm {
    /// Bind to a registry so tool schemas are sent every turn.
    pub fn new(config: Config, registry: &'static Registry) -> Self {
        AnthropicLlm { config, registry }
    }

    fn tools(&self) -> Vec<(&'static str, &'static str)> {
        self.registry.specs().map(|s| (s.name, s.summary)).collect()
    }
}

impl Llm for AnthropicLlm {
    fn plan(&mut self, ctx: Decision<'_>) -> Step {
        let tools = self.tools();
        let body = build_request(&self.config.model, ctx, &tools, &self.config.system);
        match post(&self.config, &body) {
            Ok(text) => parse_step(&text),
            Err(err) => {
                ::std::eprintln!("model call failed: {err}");
                Step::Done
            }
        }
    }
}

fn post(config: &Config, body: &str) -> Result<String, String> {
    let response = ureq::post(&config.endpoint())
        .set("Content-Type", "application/json")
        .set("x-api-key", &config.api_key)
        .set("anthropic-version", "2023-06-01")
        .set("user-agent", "tiny-agent/0.1 (claude-cli-compatible)")
        .timeout(::std::time::Duration::from_secs(90))
        .send_string(body);
    match response {
        Ok(resp) => resp.into_string().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(code, resp)) => {
            let detail = resp.into_string().unwrap_or_default();
            Err(::std::format!("HTTP {code}: {}", clip(&detail, 300)))
        }
        Err(err) => Err(err.to_string()),
    }
}

/// One cheap round trip, used by `--probe` to tell a dead key from a dead path.
pub fn probe(config: &Config) -> Result<String, String> {
    let body = json!({
        "model": config.model,
        "max_tokens": 16,
        "messages": [ { "role": "user", "content": "reply with the single word: ok" } ]
    })
    .to_string();
    let raw = post(config, &body)?;
    match parse_step(&raw) {
        Step::Say(text) => Ok(text.to_string()),
        _ => Ok("<no text>".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Decision<'static> {
        Decision {
            goal: "fix build",
            step: 1,
            observation: "1| alpha",
        }
    }

    #[test]
    fn request_uses_anthropic_shape() {
        let body = build_request("m", ctx(), &[("read_file", "read")], "sys");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["model"], "m");
        assert_eq!(v["system"], "sys");
        assert_eq!(v["tools"][0]["name"], "read_file");
        assert!(v["tools"][0]["input_schema"]["properties"]["args"].is_object());
        assert!(v["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("fix build"));
    }

    #[test]
    fn tool_use_block_becomes_a_call() {
        let raw = r#"{"content":[{"type":"tool_use","name":"read_file","input":{"args":"a.rs"}}]}"#;
        match parse_step(raw) {
            Step::Call(c) => {
                assert_eq!(c.tool.as_str(), "read_file");
                assert_eq!(c.args.as_str(), "a.rs");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn text_block_becomes_a_say() {
        let raw = r#"{"content":[{"type":"text","text":"done"}]}"#;
        assert!(matches!(parse_step(raw), Step::Say(_)));
    }

    #[test]
    fn malformed_bodies_stop_cleanly() {
        assert_eq!(parse_step("nope"), Step::Done);
        assert_eq!(parse_step("{}"), Step::Done);
        assert_eq!(parse_step(r#"{"content":[]}"#), Step::Done);
    }

    #[test]
    fn named_input_is_flattened() {
        let v: Value = serde_json::from_str(r#"{"path":"a.rs","old":"x","new":"y"}"#).unwrap();
        assert_eq!(input_to_args(&v), "a.rs|x|y");
        let v2: Value = serde_json::from_str(r#"{"path":"a.rs"}"#).unwrap();
        assert_eq!(input_to_args(&v2), "a.rs");
    }

    #[test]
    fn endpoint_is_normalised() {
        let cfg = Config {
            base_url: "http://127.0.0.1:15721/".into(),
            api_key: "k".into(),
            model: "m".into(),
            system: "s".into(),
        };
        assert_eq!(cfg.endpoint(), "http://127.0.0.1:15721/v1/messages");
    }
}
