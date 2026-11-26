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
use crate::util;
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
    about = "Send one prompt to ChatGPT and print the answer. By default it spends the CHAT quota: it drives a chromium that is already logged in to chatgpt.com (compose the prompt in the web composer, click send) and reads the reply back over REST, returning { quota: chat, conversation_id, model, text, images, complete, cdp, polls, elapsed_ms } (image generations come back with an empty text body and an images[] list); pass --save-images DIR to also download the PNGs, or --inline-images to embed them as base64 (data_base64) so an MCP client can render them as image content blocks. The CODEX quota is only touched when --allow-codex is passed explicitly (a deliberate opt-in, never implicit); that path calls POST /backend-api/codex/responses and returns { quota: codex, model, effort, response_id, text, usage, events, elapsed_ms }. Either path consumes quota, so the command runs at safety T1; when the plan is exhausted the backend reports usage_limit_reached with the reset time instead of retrying."
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
        about = "Model slug (chat path: e.g. gpt-6-2 / gpt-6 / gpt-5-6 / gpt-6-2-instant / gpt-6-2-thinking, empty = web default; codex path: gpt-6.2-sol)",
        default = ""
    )]
    model: String,
    #[arg(
        about = "Reasoning effort (chat path: instant|low / medium / high|max, empty = web default; codex path: low / medium / high / xhigh / max / ultra)",
        default = ""
    )]
    effort: String,
    #[arg(
        about = "Extra system instructions prepended to the codex call",
        default = ""
    )]
    instructions: String,
    #[arg(
        about = "Directory to save generated images into (empty means do not download them)",
        default = ""
    )]
    save_images: std::path::PathBuf,
    #[arg(
        about = "Download generated images and inline them as images[].data_base64 (needed for MCP clients to receive them as image content blocks)",
        default = false
    )]
    inline_images: bool,
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
        let model = (!app.model.trim().is_empty()).then_some(app.model.trim());
        let effort = (!app.effort.trim().is_empty()).then_some(app.effort.trim());
        let opts = chat::ChatOptions {
            model,
            effort,
            ..Default::default()
        };
        let mut result = chat::ask_with_options(&prompt, app.cdp.trim(), opts, ctx)?;
        fetch_images(app, ctx, &mut result);
        print_answer(ctx, &result);
        ctx.tick(1, Some(1), "");
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    run_codex(app, &prompt, ctx, start)
}

/// codex responses 对 ChatGPT 账号只认这一个 slug
pub(crate) fn resolve_model(model: &str) -> Result<String, AppError> {
    match model.trim() {
        "" | "gpt-6.2-sol" => Ok(DEFAULT_MODEL.to_string()),
        other => Err(AppError::InvalidArg(format!(
            "模型 {other} 在 codex responses 上不可用；ChatGPT 账号只接受 {DEFAULT_MODEL}"
        ))),
    }
}

/// 推理档位白名单校验（省略 = low）
pub(crate) fn resolve_effort(effort: &str) -> Result<String, AppError> {
    match effort.trim() {
        "" => Ok("low".to_string()),
        e if EFFORTS.contains(&e) => Ok(e.to_string()),
        other => Err(AppError::InvalidArg(format!(
            "effort 只能是 {}，收到 {other}",
            EFFORTS.join(" / ")
        ))),
    }
}

/// 客户端没给系统提示时用的那份（`serve` 需要拿到同一个默认值）
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub(crate) fn default_instructions() -> &'static str {
    DEFAULT_INSTRUCTIONS
}

/// 跑一次 `POST /backend-api/codex/responses`，边走边把**累计全文**回调出去。
///
/// `input` 是 responses 协议里的输入项数组（`[{type:message,role,content:[…]}]`）：
/// CLI 那条路只放一条用户消息，`serve` 会放整段对话。
pub(crate) fn stream_codex(
    input: &Value,
    model: &str,
    effort: &str,
    instructions: &str,
    ctx: &Context,
    on: &mut dyn FnMut(&str),
) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    // 发消息要用带 api.connectors.invoke 的凭证；过期就顺手用 refresh_token 续一份
    // （换到的 refresh_token 会写回仓库）
    let api = Api::streaming(refresh::ensure_fresh(auth::load_for_send()?));
    let body = json!({
        "model": model,
        "instructions": instructions,
        "input": input,
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
                    on(&text);
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
    Ok(json!({
        "model": model,
        "response_id": response_id,
        "text": text,
        "usage": usage,
        "events": events,
        "elapsed_ms": start.elapsed().as_millis() as u64,
    }))
}

/// `--allow-codex` 那条路：POST /backend-api/codex/responses（SSE）
fn run_codex(
    app: &Ask,
    prompt: &str,
    ctx: &Context,
    start: std::time::Instant,
) -> Result<Value, AppError> {
    let model = resolve_model(&app.model)?;
    let effort = resolve_effort(&app.effort)?;
    let instructions = if app.instructions.trim().is_empty() {
        DEFAULT_INSTRUCTIONS.to_string()
    } else {
        app.instructions.trim().to_string()
    };

    ctx.log(
        LogLevel::Warn,
        "用的是 codex 额度（--allow-codex）".to_string(),
    );
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("codex · {model} / effort={effort}")),
    });
    let input = json!([{
        "type": "message",
        "role": "user",
        "content": [{"type": "input_text", "text": prompt}],
    }]);
    let v = stream_codex(&input, &model, &effort, &instructions, ctx, &mut |_| {})?;

    let result = json!({
        "quota": "codex",
        "model": model,
        "effort": effort,
        "response_id": v["response_id"].clone(),
        "text": v["text"].clone(),
        "usage": v["usage"].clone(),
        "elapsed_ms": start.elapsed().as_millis() as u64,
        "events": v["events"].clone(),
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

/// 人读面：把答案按行打出来（全文仍在 --json 里）。
///
/// 生图那一路正文通常是空的，所以要单独报一句「生成了 N 张图」——
/// 否则人读面看上去像是什么也没回。
fn print_answer(ctx: &Context, result: &Value) {
    for line in result["text"].as_str().unwrap_or("").lines() {
        ctx.log(LogLevel::Info, line.to_string());
    }
    if let Some(images) = result["images"].as_array() {
        if !images.is_empty() {
            ctx.log(
                LogLevel::Info,
                format!(
                    "生成了 {} 张图（file_id 见 --json 的 images[]）",
                    images.len()
                ),
            );
            for (n, img) in images.iter().enumerate() {
                let id = img["file_id"].as_str().unwrap_or("");
                let w = img["width"].as_u64().unwrap_or(0);
                let h = img["height"].as_u64().unwrap_or(0);
                let saved = img["saved_to"].as_str().unwrap_or("");
                let tail = if saved.is_empty() {
                    String::new()
                } else {
                    format!(" -> {saved}")
                };
                ctx.log(LogLevel::Info, format!("  [{}] {id} {w}x{h}{tail}", n + 1));
            }
        }
    }
    if result["complete"].as_bool() == Some(false) {
        ctx.log(
            LogLevel::Warn,
            "回答可能还没写完（页面没给出收尾标记）；重新读一次会话可看到完整内容".to_string(),
        );
    }
}

/// 把生成的图下载下来：`--save-images` 落盘，`--inline-images` 内联成 base64。
///
/// 两条用途不同：落盘给人看；内联是为了让 MCP 客户端（如 dsh）能收到真正的
/// `{type:"image"}` 内容块 —— 它拿不到我们的登录态，没法自己下载 `sediment://`。
///
/// 下载要两步、都是同一套认证头（缺 chatgpt-account-id 会被 403）：
/// `GET /files/{file_id}/download` 拿一个带签名的 `download_url`，再 GET 那个地址取字节。
/// 下载失败只警告、不打断提问 —— 额度已经花掉了，答案该照常交出去。
fn fetch_images(app: &Ask, ctx: &Context, result: &mut Value) {
    let dir = app.save_images.clone();
    let inline = app.inline_images;
    if dir.as_os_str().is_empty() && !inline {
        return;
    }
    let files: Vec<String> = match result["images"].as_array() {
        Some(list) => list
            .iter()
            .filter_map(|i| i["file_id"].as_str().map(str::to_string))
            .collect(),
        None => return,
    };
    if files.is_empty() {
        return;
    }
    if !dir.as_os_str().is_empty() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            ctx.log(
                LogLevel::Warn,
                format!("建不出目录 {}：{e}（图没有落盘）", dir.display()),
            );
        }
    }
    let creds = match auth::load() {
        Ok(c) => refresh::ensure_fresh(c),
        Err(e) => {
            ctx.log(LogLevel::Warn, format!("拿不到凭证，图没有下载：{e}"));
            return;
        }
    };
    let api = Api::new(creds);
    let mut saved: Vec<(String, String)> = Vec::new();
    let mut inlined: Vec<(String, String, String)> = Vec::new();
    for file_id in &files {
        match api.download_file(file_id) {
            Ok(bytes) => {
                let (mime, ext) = util::sniff_image_mime(&bytes);
                if !dir.as_os_str().is_empty() {
                    let path = dir.join(format!("{file_id}.{ext}"));
                    match std::fs::write(&path, &bytes) {
                        Ok(()) => {
                            ctx.log(
                                LogLevel::Info,
                                format!("已存 {} 字节 → {}", bytes.len(), path.display()),
                            );
                            saved.push((file_id.clone(), path.display().to_string()));
                        }
                        Err(e) => {
                            ctx.log(LogLevel::Warn, format!("写不进 {}：{e}", path.display()))
                        }
                    }
                }
                if inline {
                    inlined.push((
                        file_id.clone(),
                        mime.to_string(),
                        util::base64_encode(&bytes),
                    ));
                }
            }
            Err(e) => ctx.log(LogLevel::Warn, format!("下载 {file_id} 失败：{e}")),
        }
    }
    if let Some(list) = result["images"].as_array_mut() {
        for img in list.iter_mut() {
            let id = img["file_id"].as_str().unwrap_or("").to_string();
            if let Some((_, path)) = saved.iter().find(|(f, _)| *f == id) {
                img["saved_to"] = Value::String(path.clone());
            }
            if let Some((_, mime, b64)) = inlined.iter().find(|(f, _, _)| *f == id) {
                img["mime_type"] = Value::String(mime.clone());
                img["data_base64"] = Value::String(b64.clone());
            }
        }
    }
    if inline {
        ctx.log(
            LogLevel::Info,
            format!(
                "已把 {} 张图内联成 base64（MCP 客户端可当 image 内容块收）",
                inlined.len()
            ),
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
            save_images: std::path::PathBuf::new(),
            inline_images: false,
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
