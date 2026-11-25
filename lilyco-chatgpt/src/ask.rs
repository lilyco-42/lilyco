//! `lchatgpt ask` —— 把一句话发出去。唯一会消耗额度的命令。
//!
//! # 两种额度，默认只花 chat 的那份
//!
//! 这个账号上有两套互相独立的配额：
//!
//! - **chat 额度**（网页聊天用掉的那份）：默认走这里。做法是借一个已登录 chatgpt.com
//!   的 chromium，把问题打进网页输入框、点发送（见 `chat.rs`），回复再用 CLI 自己的
//!   REST 读回来。花的是网页聊天的配额，不碰 codex。
//! - **codex 额度**（`POST /backend-api/codex/responses`，codex CLI 用的那份）：只有
//!   显式加 `--allow-codex` 才会用。绝不隐式消耗 —— 用户没勾，就不花。
//!
//! 这是硬规矩：chat 花 chat 的，codex 必须由用户勾选。所以两条路都不通时报错也只说
//! 清楚怎么办，不会偷偷换另一条额度去试。

use crate::api::Api;
use crate::auth;
use crate::chat;
use crate::refresh;
use lilyco::prelude::*;
use serde_json::{json, Value};

/// codex responses 对 ChatGPT 账号只认这一个 slug
const DEFAULT_MODEL: &str = "gpt-6.2-sol";
/// 可选推理档位（来自 codex 的模型缓存：low/medium/high/xhigh/max/ultra）
const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max", "ultra"];
/// 没显式给指令时的默认系统提示
const DEFAULT_INSTRUCTIONS: &str =
    "You are ChatGPT, a helpful assistant answering inside a terminal CLI. Be concise and concrete.";

/// 提问
#[derive(App)]
#[app(
    name = "ask",
    run = "run_ask",
    safety = "t1",
    about = "Send one prompt to ChatGPT and print the answer. By default it spends the CHAT quota: it drives a chromium that is already logged in to chatgpt.com (compose the prompt in the web composer, click send) and reads the reply back over REST, returning { quota: chat, conversation_id, model, text, complete, cdp, polls, elapsed_ms }. The CODEX quota is only touched when --allow-codex is passed explicitly (a deliberate opt-in, never implicit); that path calls POST /backend-api/codex/responses and returns { quota: codex, model, effort, response_id, text, usage, events, elapsed_ms }. Either path consumes quota, so the command runs at safety T1; when the plan is exhausted the backend reports usage_limit_reached with the reset time instead of retrying."
)]
pub struct Ask {
    #[arg(about = "The prompt to send")]
    prompt: String,
    #[arg(
        about = "Spend the CODEX quota instead of the chat quota (explicit opt-in; off means chat only)",
        default = false
    )]
    allow_codex: bool,
    #[arg(
        about = "CDP endpoint of the browser used for the chat path, e.g. http://127.0.0.1:9229 (empty means auto-detect)",
        default = ""
    )]
    cdp: String,
    #[arg(
        about = "Codex model slug (empty means gpt-6.2-sol, the only slug a ChatGPT account accepts)",
        default = "gpt-6.2-sol"
    )]
    model: String,
    #[arg(
        about = "Codex reasoning effort: low / medium / high / xhigh / max / ultra",
        default = "low"
    )]
    effort: String,
    #[arg(
        about = "Extra system instructions prepended to the codex call",
        default = ""
    )]
    instructions: String,
}

/// SSE 里的一次增量。只认我们需要的几种事件，其余静默跳过。
enum Chunk {
    Text(String),
    Done {
        id: Option<String>,
        usage: Option<Value>,
    },
    Failed(String),
}

fn parse_event(data: &str) -> Option<Chunk> {
    let v: Value = serde_json::from_str(data).ok()?;
    match v.get("type").and_then(Value::as_str).unwrap_or("") {
        "response.output_text.delta" => v
            .get("delta")
            .and_then(Value::as_str)
            .map(|d| Chunk::Text(d.to_string())),
        "response.completed" => {
            let r = v.get("response");
            Some(Chunk::Done {
                id: r
                    .and_then(|r| r.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                usage: r.and_then(|r| r.get("usage")).cloned(),
            })
        }
        "response.failed" | "error" => {
            let msg = v
                .get("error")
                .or_else(|| v.get("response").and_then(|r| r.get("error")))
                .map(|e| {
                    e.get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                })
                .unwrap_or_default();
            Some(Chunk::Failed(if msg.is_empty() {
                "后端报告 response.failed，但没有给出原因".to_string()
            } else {
                msg
            }))
        }
        _ => None,
    }
}

fn run_ask(app: &Ask, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let prompt = app.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(AppError::InvalidArg(
            "prompt 不能为空；例如 lchatgpt ask --prompt 你好".to_string(),
        ));
    }
    // 默认只花 chat 额度：codex 必须有用户的显式许可
    if !app.allow_codex {
        let result = chat::ask(&prompt, app.cdp.trim(), ctx)?;
        print_answer(ctx, &result);
        ctx.tick(1, Some(1), "");
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    run_codex(app, &prompt, ctx, start)
}

/// `--allow-codex` 那条路：POST /backend-api/codex/responses（SSE）
fn run_codex(
    app: &Ask,
    prompt: &str,
    ctx: &Context,
    start: std::time::Instant,
) -> Result<Value, AppError> {
    let model = match app.model.trim() {
        "" => DEFAULT_MODEL.to_string(),
        "gpt-6.2-sol" => DEFAULT_MODEL.to_string(),
        other => {
            return Err(AppError::InvalidArg(format!(
                "模型 {other} 在 codex responses 上不可用；ChatGPT 账号只接受 {DEFAULT_MODEL}"
            )))
        }
    };
    let effort = match app.effort.trim() {
        "" => "low".to_string(),
        e if EFFORTS.contains(&e) => e.to_string(),
        other => {
            return Err(AppError::InvalidArg(format!(
                "effort 只能是 {}，收到 {other}",
                EFFORTS.join(" / ")
            )))
        }
    };
    let instructions = if app.instructions.trim().is_empty() {
        DEFAULT_INSTRUCTIONS.to_string()
    } else {
        app.instructions.trim().to_string()
    };

    // 发消息要用带 api.connectors.invoke 的凭证；过期就顺手用 refresh_token 续一份
    // （换到的 refresh_token 会写回仓库）
    let api = Api::streaming(refresh::ensure_fresh(auth::load_for_send()?));
    ctx.log(
        LogLevel::Warn,
        "用的是 codex 额度（--allow-codex）".to_string(),
    );
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("codex · {model} / effort={effort}")),
    });
    let body = json!({
        "model": model,
        "instructions": instructions,
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": prompt}],
        }],
        "stream": true,
        "store": false,
        "reasoning": {"effort": effort},
        "include": [],
    });
    let res = api.post_stream("/codex/responses", &body)?;

    let mut text = String::new();
    let mut events = 0u64;
    let mut response_id: Option<String> = None;
    let mut usage: Option<Value> = None;
    let mut failure: Option<String> = None;
    {
        // BodyReader 只实现了 io::Read（没有 BufRead），要自己套一层 BufReader 才能按行读
        use std::io::BufRead;
        let reader = std::io::BufReader::new(res.into_body().into_reader());
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => return Err(AppError::Runtime(format!("读流失败: {e}"))),
            };
            let payload = match line.strip_prefix("data:") {
                Some(p) => p.trim(),
                None => continue,
            };
            if payload.is_empty() {
                continue;
            }
            if payload == "[DONE]" {
                break;
            }
            match parse_event(payload) {
                Some(Chunk::Text(d)) => {
                    text.push_str(&d);
                    events += 1;
                    // 每 20 个增量报一次进度，别把 stderr 刷爆
                    if events.is_multiple_of(20) {
                        ctx.tick(events, None, format!("{} 字", text.chars().count()));
                    }
                }
                Some(Chunk::Done { id, usage: u }) => {
                    response_id = id;
                    usage = u;
                }
                Some(Chunk::Failed(m)) => {
                    failure = Some(m);
                    break;
                }
                None => {}
            }
        }
    }
    if let Some(m) = failure {
        return Err(AppError::Runtime(format!("模型侧失败: {m}")));
    }
    if text.trim().is_empty() {
        return Err(AppError::Runtime(
            "流结束但没有任何文本增量；用 --json-stream 看完整事件流".to_string(),
        ));
    }

    let result = json!({
        "quota": "codex",
        "model": model,
        "effort": effort,
        "response_id": response_id,
        "text": text,
        "usage": usage,
        "elapsed_ms": start.elapsed().as_millis() as u64,
        "events": events,
    });
    print_answer(ctx, &result);
    if let Some(u) = result["usage"].as_object() {
        let i = u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
        let o = u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
        ctx.log(LogLevel::Info, format!("tokens: {i} in / {o} out"));
    }
    ctx.tick(1, Some(1), "");
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// 人读面：把答案按行打出来（全文仍在 --json 里）
fn print_answer(ctx: &Context, result: &Value) {
    for line in result["text"].as_str().unwrap_or("").lines() {
        ctx.log(LogLevel::Info, line.to_string());
    }
    if result["complete"].as_bool() == Some(false) {
        ctx.log(
            LogLevel::Warn,
            "回答可能还没写完（页面没给出收尾标记）；重新读一次会话可看到完整内容".to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(prompt: &str, allow_codex: bool) -> Ask {
        Ask {
            prompt: prompt.into(),
            allow_codex,
            cdp: String::new(),
            model: String::new(),
            effort: String::new(),
            instructions: String::new(),
        }
    }

    fn ctx() -> Context {
        let (tx, _rx) = std::sync::mpsc::channel();
        Context::new_test(tx)
    }

    #[test]
    fn a_text_delta_becomes_a_chunk() {
        match parse_event(r#"{"type":"response.output_text.delta","delta":"你"}"#) {
            Some(Chunk::Text(t)) => assert_eq!(t, "你"),
            _ => panic!("应当解析成文本增量"),
        }
    }

    #[test]
    fn a_completed_event_carries_the_response_id_and_usage() {
        let raw = r#"{"type":"response.completed","response":{"id":"resp_1","usage":{"input_tokens":3,"output_tokens":7}}}"#;
        match parse_event(raw) {
            Some(Chunk::Done { id, usage }) => {
                assert_eq!(id.unwrap(), "resp_1");
                assert_eq!(usage.unwrap()["output_tokens"], 7);
            }
            _ => panic!("应当解析成完成事件"),
        }
    }

    #[test]
    fn an_unknown_event_is_ignored_and_a_failure_is_surfaced() {
        assert!(parse_event(r#"{"type":"response.created"}"#).is_none());
        match parse_event(r#"{"type":"error","error":{"message":"boom"}}"#) {
            Some(Chunk::Failed(m)) => assert_eq!(m, "boom"),
            _ => panic!("应当解析成失败"),
        }
    }

    /// 空 prompt 在任何额度上都不该发出去
    #[test]
    fn an_empty_prompt_is_rejected_before_any_request() {
        for allow in [false, true] {
            let err = run_ask(&app("   ", allow), &ctx()).expect_err("空 prompt 要报错");
            assert!(format!("{err}").contains("prompt"));
        }
    }

    /// 默认（没勾 --allow-codex）绝不能碰 codex 额度：报错文案要写明怎么打开它
    #[test]
    fn without_the_opt_in_the_codex_quota_is_not_touched() {
        let _env = crate::testenv::no_credentials("ask-chat-default");
        let err = run_ask(&app("你好", false), &ctx()).expect_err("没有浏览器时 chat 路径要报错");
        let msg = format!("{err}");
        assert!(msg.contains("chat"), "{msg}");
        assert!(
            msg.contains("--allow-codex"),
            "必须告诉用户怎么显式改用 codex 额度: {msg}"
        );
        assert!(
            !msg.contains("codex/responses"),
            "默认路径不该说已经发往 codex responses: {msg}"
        );
    }

    /// 显式勾选之后才走 codex；模型白名单仍然生效（先校验参数再联网）
    #[test]
    fn a_non_default_model_is_rejected_with_the_reason() {
        let mut a = app("hi", true);
        a.model = "gpt-6-thinking".into();
        let err = run_ask(&a, &ctx()).expect_err("别的模型要报错");
        let msg = format!("{err}");
        assert!(msg.contains("gpt-6.2-sol"), "{msg}");
    }

    #[test]
    fn the_effort_list_matches_the_codex_model_cache() {
        assert_eq!(EFFORTS, &["low", "medium", "high", "xhigh", "max", "ultra"]);
    }
}
