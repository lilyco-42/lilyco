//! OpenAI-compatible chat client: OpenRouter, NVIDIA NIM, and any local
//! proxy such as cc-switch all speak the same `POST /chat/completions`.
//!
//! The networking is split from the protocol on purpose: [`build_request`] and
//! [`parse_step`] are pure functions with no I/O, so the wire format is covered
//! by tests that never touch the network.

use std::string::{String, ToString};
use std::vec::Vec;

use serde_json::{json, Value};

use crate::agent::llm::{Call, Decision, Llm, Step, MAX_SPEECH};
use crate::agent::registry::Registry;

/// Base URLs for the providers this MVP knows about.
///
/// Anything else must be passed explicitly through `TINY_AGENT_BASE_URL` —
/// guessing a private port would be worse than asking.
pub fn preset(provider: &str) -> Option<&'static str> {
    match provider {
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "nim" | "nvidia" => Some("https://integrate.api.nvidia.com/v1"),
        "ccswitch" | "cc-switch" | "local" => Some("http://127.0.0.1:8788/v1"),
        _ => None,
    }
}

/// Instructions shared by every provider.
///
/// The `args` convention is explained here rather than encoded in JSON Schema so
/// the tool set stays pipe-parsable on the Rust side.
pub const SYSTEM: &str = "You are a coding agent. You may call at most one tool per turn.\n\
The tool argument is a single string:\n\
- read_file: <relative path>\n\
- list_dir: <relative path or empty>\n\
- edit_file: <path>|<old text>|<new text>  (old must appear exactly once)\n\
Call a tool only if it moves the goal forward. When the goal is met, reply with a short final answer instead of calling a tool.";

fn tool_def(name: &str, summary: &str) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": summary, "parameters": param_schema() }
    })
}

fn param_schema() -> Value {
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
        ctx.goal, seen, ctx.step
    )
}

/// Build the JSON body for one turn.
pub fn build_request(model: &str, ctx: Decision<'_>, tools: &[(&str, &str)]) -> String {
    let defs: Vec<Value> = tools
        .iter()
        .map(|(name, summary)| tool_def(name, summary))
        .collect();
    let mut body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": SYSTEM },
            { "role": "user", "content": user_message(ctx) }
        ],
        "temperature": 0.2
    });
    // An empty `tools` array is rejected by some gateways ("must not be an
    // empty array"), so the field is omitted entirely when there is no tool.
    if !defs.is_empty() {
        body["tools"] = Value::Array(defs);
        body["tool_choice"] = json!("auto");
    }
    body.to_string()
}

/// Drop a leaked reasoning block: reasoning models emit `…<｜end▁of▁thinking｜>answer`
/// inside `content`, and the chain of thought is not what the user asked for.
fn visible(text: &str) -> &str {
    const CLOSERS: [&str; 3] = ["<｜end▁of▁thinking｜>", "</think>", "[/THINK]"];
    for closer in CLOSERS {
        if let Some(at) = text.find(closer) {
            return text[at + closer.len()..].trim_start();
        }
    }
    text
}

/// Turn one response body into the next [`Step`].
pub fn parse_step(raw: &str) -> Step {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Step::Done;
    };
    let message = &value["choices"][0]["message"];
    if let Some(call) = first_call(message) {
        return Step::Call(call);
    }
    match message["content"].as_str() {
        Some(text) if !text.trim().is_empty() => {
            let mut buf = heapless::String::<MAX_SPEECH>::new();
            let _ = buf.push_str(clip(visible(text), MAX_SPEECH));
            Step::Say(buf)
        }
        _ => Step::Done,
    }
}

fn first_call(message: &Value) -> Option<Call> {
    let calls = message["tool_calls"].as_array()?;
    let first = calls.first()?;
    let name = first["function"]["name"].as_str()?;
    let raw_args = first["function"]["arguments"].as_str().unwrap_or("");
    Call::new(name, &normalize_args(raw_args)).ok()
}

/// Flatten provider-specific argument shapes into our `args` string.
///
/// Three shapes occur in practice: `{"args": "..."}`, `{path, old, new}` (models
/// that prefer named parameters), and a bare string.
pub fn normalize_args(raw: &str) -> String {
    let trimmed = raw.trim();
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return trimmed.to_string();
    };
    if let Some(args) = value.get("args").and_then(Value::as_str) {
        return args.to_string();
    }
    let path = value.get("path").and_then(Value::as_str);
    let old = value.get("old").and_then(Value::as_str);
    let new = value.get("new").and_then(Value::as_str);
    match (path, old, new) {
        (Some(p), Some(o), Some(n)) => ::std::format!("{p}|{o}|{n}"),
        (Some(p), _, _) => p.to_string(),
        _ => trimmed.to_string(),
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

/// Provider configuration, filled from flags or environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Base URL, e.g. `https://openrouter.ai/api/v1`.
    pub base_url: String,
    /// Bearer token.
    pub api_key: String,
    /// Model id understood by that provider.
    pub model: String,
}

impl Config {
    /// Resolve from explicit values first, then the environment.
    pub fn resolve(provider: &str, base: Option<String>, model: Option<String>, key: Option<String>) -> Result<Self, String> {
        let base_url = base
            .or_else(|| std::env::var("TINY_AGENT_BASE_URL").ok())
            .or_else(|| preset(provider).map(ToString::to_string))
            .ok_or_else(|| ::std::format!("unknown provider '{provider}': pass --base-url"))?;
        let api_key = key
            .or_else(|| std::env::var("TINY_AGENT_API_KEY").ok())
            .ok_or("missing API key: pass --api-key or set TINY_AGENT_API_KEY")?;
        let model = model
            .or_else(|| std::env::var("TINY_AGENT_MODEL").ok())
            .ok_or("missing model: pass --model or set TINY_AGENT_MODEL")?;
        Ok(Config {
            base_url,
            api_key,
            model,
        })
    }

    /// `POST <base>/chat/completions`.
    pub fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        // Some providers (and free OpenAI-compatible gateways) publish the full
        // path instead of a `/v1` prefix; honour it instead of appending twice.
        let complete = base.ends_with("/chat/completions") || base.ends_with("/openai");
        if complete {
            return base.to_string();
        }
        ::std::format!("{base}/chat/completions")
    }
}


/// True when a provider rejected the request because of tool schemas.
fn mentions_tools(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("tool")
}

/// A model reached over HTTP.
pub struct HttpLlm {
    config: Config,
    registry: &'static Registry,
    /// Some OpenAI-compatible gateways (notably free ones) stall when a
    /// `tools` array is present, so tool use is opt-out.
    tools: bool,
}

impl HttpLlm {
    /// Bind to a registry so tool schemas can be sent every turn.
    pub fn new(config: Config, registry: &'static Registry) -> Self {
        HttpLlm {
            config,
            registry,
            tools: true,
        }
    }

    /// Same client, but the request carries no `tools` array: the model can only
    /// answer in prose. Used for gateways that hang on tool schemas.
    pub fn without_tools(config: Config, registry: &'static Registry) -> Self {
        HttpLlm {
            config,
            registry,
            tools: false,
        }
    }


    /// Retry once without tool schemas.
    fn answer_without_tools(&self, ctx: Decision<'_>) -> Step {
        let plain = build_request(&self.config.model, ctx, &[]);
        match post(&self.config, &plain) {
            Ok(text) => parse_step(&text),
            Err(err) => {
                ::std::eprintln!("model call failed (no tools): {err}");
                Step::Done
            }
        }
    }

    fn tool_list(&self) -> Vec<(&'static str, &'static str)> {
        self.registry
            .specs()
            .map(|s| (s.name, s.summary))
            .collect()
    }
}

impl Llm for HttpLlm {
    fn plan(&mut self, ctx: Decision<'_>) -> Step {
        let tools = if self.tools { self.tool_list() } else { Vec::new() };
        let body = build_request(&self.config.model, ctx, &tools);
        match post(&self.config, &body) {
            Ok(text) => parse_step(&text),
            // Some servers (vLLM without `--enable-auto-tool-choice`, free
            // gateways) reject tool schemas. Answering in prose beats failing.
            Err(err) if self.tools && mentions_tools(&err) => self.answer_without_tools(ctx),
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
        .set("Authorization", &::std::format!("Bearer {}", config.api_key))
        .set("HTTP-Referer", "https://github.com/lilyco-42/lilyco")
        .set("X-Title", "Tiny Agent")
        .timeout(std::time::Duration::from_secs(90))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Decision<'static> {
        Decision {
            goal: "fix the build",
            step: 2,
            observation: "  1| alpha",
        }
    }

    #[test]
    fn presets_cover_the_three_providers() {
        assert!(preset("openrouter").unwrap().contains("openrouter.ai"));
        assert!(preset("nim").unwrap().contains("nvidia.com"));
        assert!(preset("ccswitch").unwrap().starts_with("http://127.0.0.1"));
        assert!(preset("nope").is_none());
    }

    #[test]
    fn request_carries_goal_observation_and_tools() {
        let body = build_request("m", ctx(), &[("read_file", "read")]);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["model"], "m");
        assert_eq!(v["tools"][0]["function"]["name"], "read_file");
        let user = v["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("fix the build"));
        assert!(user.contains("1| alpha"));
        assert!(v["messages"][0]["content"].as_str().unwrap().contains("edit_file"));
    }

    #[test]
    fn tool_call_is_parsed() {
        let raw = r#"{"choices":[{"message":{"tool_calls":[{"function":{"name":"read_file","arguments":"{\"args\":\"src/main.rs\"}"}}]}}]}"#;
        match parse_step(raw) {
            Step::Call(c) => {
                assert_eq!(c.tool.as_str(), "read_file");
                assert_eq!(c.args.as_str(), "src/main.rs");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn final_answer_is_parsed() {
        let raw = r#"{"choices":[{"message":{"content":"done"}}]}"#;
        assert!(matches!(parse_step(raw), Step::Say(_)));
    }

    #[test]
    fn reasoning_blocks_are_hidden() {
        assert_eq!(visible("plan…\n<｜end▁of▁thinking｜>\n\n答案"), "答案");
        assert_eq!(visible("no think here"), "no think here");
    }

    #[test]
    fn malformed_response_stops_cleanly() {
        assert_eq!(parse_step("not json"), Step::Done);
        assert_eq!(parse_step("{}"), Step::Done);
    }

    #[test]
    fn named_parameters_are_flattened() {
        assert_eq!(
            normalize_args(r#"{"path":"a.rs","old":"x","new":"y"}"#),
            "a.rs|x|y"
        );
        assert_eq!(normalize_args(r#"{"path":"a.rs"}"#), "a.rs");
        assert_eq!(normalize_args(r#"{"args":"plain"}"#), "plain");
        assert_eq!(normalize_args("plain"), "plain");
    }

    #[test]
    fn full_paths_are_not_appended() {
        let cfg = Config {
            base_url: "https://text.pollinations.ai/openai".into(),
            api_key: "none".into(),
            model: "openai".into(),
        };
        assert_eq!(cfg.endpoint(), "https://text.pollinations.ai/openai");
    }

    #[test]
    fn endpoint_has_one_slash() {
        let cfg = Config {
            base_url: "https://x/v1/".into(),
            api_key: "k".into(),
            model: "m".into(),
        };
        assert_eq!(cfg.endpoint(), "https://x/v1/chat/completions");
    }
}
