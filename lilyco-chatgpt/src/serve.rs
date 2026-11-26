//! `lchatgpt serve` —— 把 ChatGPT 账号后端开成一个 **OpenAI 兼容**的本地端口。
//!
//! 为什么要这一层：dsh（DeepSeek Harness）的 `llm-pi-ai` 插件只认四条线上协议
//! （`openai-completions` / `openai-responses` / `anthropic-messages` /
//! `google-generative-ai`），而 chatgpt.com 那边实际能给的是「网页聊天」和
//! `codex/responses` 两种。于是这里把两者都包装成最普通的
//! `POST /v1/chat/completions`，dsh 侧只要加一条 `api: openai-completions` 的
//! route 指到 `http://127.0.0.1:8790/v1` 即可。
//!
//! # 额度归属不变
//!
//! 这个端**不新增任何花钱的路子**，它只是把已有的两条路原样暴露出来：
//!
//! - 模型 `lchatgpt:chat`（默认）：走 `chat::ask_streaming`，驱动一台已登录
//!   chatgpt.com 的 chromium —— 花 **chat 额度**。
//! - 模型 `lchatgpt:codex`：走 `ask::stream_codex`（`POST /backend-api/codex/responses`）
//!   —— 花 **codex 额度**，而且**只有启动时显式加了 `--allow-codex` 才会挂上**。
//!   没挂的时候请求它直接 400，不会偷偷换成别的额度。
//!
//! # 一个已知的形变：多轮对话被压成一段文字
//!
//! chat 那条路的会话历史活在浏览器页面里，CLI 这边只能「往输入框里打字」。所以
//! `messages` 数组会被拼成一段带 `User:` / `Assistant:` 标签的转录文本一起发过去
//! （见 `transcript`）。codex 那条支持真正的多轮，`messages` 会逐条翻成
//! responses 协议的 `input` 项。
//!
//! # 流式：网页只有一个「当前答案」
//!
//! `chat::ask_streaming` 的回调给的是**累计全文**，这里按 `Diff` 逐次算出新增量再
//! 发 `delta`，所以客户端看到的是粗粒度的增长（约每 1.2 秒一批），不是 token 级流。

use std::convert::Infallible;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{extract::State, Json, Router};
use lilyco::prelude::*;
use serde_json::{json, Value};

use crate::api::Api;
use crate::ask;
use crate::auth;
use crate::chat;
use crate::refresh;
use crate::store;
use crate::util;

/// 省略 `--port` 时用哪个端口（数字参数省略会变成 0，0 在这里等于「用默认」）
const DEFAULT_PORT: u16 = 8790;
/// 默认只听本机回环：这个端背后是你的登录态和额度，不该暴露到网络上
const DEFAULT_HOST: &str = "127.0.0.1";
/// chat 额度那条路的模型 id
pub const MODEL_CHAT: &str = "lchatgpt:chat";
/// codex 额度那条路的模型 id（只在挂了 codex 额度时存在）
pub const MODEL_CODEX: &str = "lchatgpt:codex";
/// 报给客户端的上下文窗口（chat 那条由网页决定，这里给一个诚实的常见值）
const CONTEXT_WINDOW: u64 = 128_000;
const MAX_OUTPUT_TOKENS: u64 = 32_000;
/// 把两套额度包成 OpenAI 兼容端口
#[derive(App)]
#[app(
    name = "serve",
    run = "run_serve",
    safety = "t1",
    about = "Expose the ChatGPT account backend as a local OpenAI-compatible HTTP endpoint (GET /v1/models, POST /v1/chat/completions streaming or not, POST /v1/images/generations returning b64_json, GET /health) so a harness such as dsh can declare this as a provider with api=openai-completions pointing at http://127.0.0.1:8790/v1. Every request spends quota: model lchatgpt:chat drives a chromium already logged in to chatgpt.com and therefore spends the CHAT quota (the default), while model lchatgpt:codex calls POST /backend-api/codex/responses and spends the CODEX quota, existing only when the server was started with --allow-codex. Multi-turn message arrays are flattened into one transcript on the chat route. The command blocks until the process is killed and prints no answer on stdout: it only serves."
)]
pub struct Serve {
    #[arg(about = "Port to listen on (0 or empty means 8790)", default = 0)]
    port: u16,
    #[arg(
        about = "Bind address (empty means 127.0.0.1; this endpoint fronts your login state, keep it on loopback)",
        default = ""
    )]
    host: String,
    #[arg(
        about = "CDP endpoint of the chromium used by the chat route, e.g. http://127.0.0.1:9229 (empty means auto-detect)",
        default = ""
    )]
    cdp: String,
    #[arg(
        about = "Also offer the codex-quota model lchatgpt:codex (off by default: the codex quota is opt-in)",
        default = false
    )]
    allow_codex: bool,
    #[arg(
        about = "Codex reasoning effort for the codex route: low / medium / high / xhigh / max / ultra",
        default = "low"
    )]
    effort: String,
}

/// 一个端口进程共享的设置
struct Config {
    cdp: String,
    allow_codex: bool,
    effort: String,
}

fn run_serve(app: &Serve, ctx: &Context) -> Result<Value, AppError> {
    let host = match app.host.trim() {
        "" => DEFAULT_HOST,
        h => h,
    };
    let port = if app.port == 0 {
        DEFAULT_PORT
    } else {
        app.port
    };
    let bind = format!("{host}:{port}");
    // 起步就校验档位：别等第一条 codex 请求才发现拼错了
    let effort = ask::resolve_effort(&app.effort)?;
    let cfg = Arc::new(Config {
        cdp: app.cdp.trim().to_string(),
        allow_codex: app.allow_codex,
        effort,
    });

    let models = if cfg.allow_codex {
        format!("{MODEL_CHAT} 与 {MODEL_CODEX}（codex 额度已挂上）")
    } else {
        format!("{MODEL_CHAT}（只有 chat 额度；codex 要 --allow-codex）")
    };
    ctx.log(
        LogLevel::Info,
        format!("OpenAI 兼容端: http://{bind}/v1 —— 提供的模型: {models}"),
    );
    ctx.emit(Progress::Started {
        total: None,
        message: Some(format!("serve {bind}")),
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::Runtime(format!("创建 tokio 运行时失败: {e}")))?;
    rt.block_on(serve_on(bind.clone(), cfg))?;
    Ok(json!({ "bind": bind }))
}

async fn serve_on(bind: String, cfg: Arc<Config>) -> Result<(), AppError> {
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|e| AppError::Runtime(format!("绑定 `{bind}` 失败: {e}")))?;
    eprintln!("lchatgpt serve: 监听 http://{bind}/v1（Ctrl-C 结束）");
    axum::serve(listener, router(cfg))
        .await
        .map_err(|e| AppError::Runtime(format!("HTTP 端异常退出: {e}")))
}

fn router(cfg: Arc<Config>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/images/generations", post(images_generations))
        .with_state(cfg)
}

async fn health(State(cfg): State<Arc<Config>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "lchatgpt",
        "quota": if cfg.allow_codex { "chat+codex" } else { "chat" },
        "models": model_ids(&cfg),
    }))
}

async fn models(State(cfg): State<Arc<Config>>) -> Json<Value> {
    let mut data = vec![json!({
        "id": MODEL_CHAT,
        "object": "model",
        "created": 0,
        "owned_by": "lchatgpt",
        "name": "ChatGPT (chat quota: drives a logged-in chromium)",
        "context_window": CONTEXT_WINDOW,
        "max_output_tokens": MAX_OUTPUT_TOKENS,
    })];
    if cfg.allow_codex {
        data.push(json!({
            "id": MODEL_CODEX,
            "object": "model",
            "created": 0,
            "owned_by": "lchatgpt",
            "name": "ChatGPT (codex quota: POST /backend-api/codex/responses)",
            "context_window": CONTEXT_WINDOW,
            "max_output_tokens": MAX_OUTPUT_TOKENS,
        }));
    }
    Json(json!({ "object": "list", "data": data }))
}

fn model_ids(cfg: &Config) -> Vec<&'static str> {
    if cfg.allow_codex {
        vec![MODEL_CHAT, MODEL_CODEX]
    } else {
        vec![MODEL_CHAT]
    }
}
/// 这笔请求该花哪份额度
#[derive(Clone, Copy, Debug, PartialEq)]
enum Route {
    Chat,
    Codex,
}

fn pick_route(model: &str, cfg: &Config) -> Result<Route, String> {
    let bare = model.trim();
    let tail = bare
        .strip_prefix("lchatgpt:")
        .or_else(|| bare.strip_prefix("lchatgpt-"))
        .or_else(|| bare.strip_prefix("lchatgpt/"))
        .unwrap_or(bare);
    let route = match tail {
        "" | "chat" => Route::Chat,
        "codex" => Route::Codex,
        other => {
            let known = model_ids(cfg).join(" / ");
            return Err(format!(
                "不认识的模型 {other:?}；这个端只提供 {known}（id 前缀 lchatgpt: 可省）"
            ));
        }
    };
    if route == Route::Codex && !cfg.allow_codex {
        return Err(format!(
            "{MODEL_CODEX} 这个端没挂上 —— 启动时加 --allow-codex 才会露出 codex 额度。默认只用 chat 额度（{MODEL_CHAT}），绝不会偷偷换过去"
        ));
    }
    Ok(route)
}

/// OpenAI 的 content 可能是纯字符串，也可能是 [{type:text,text}] 这样的分段
fn content_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|p| match p.get("text").and_then(Value::as_str) {
                Some(t) => t.to_string(),
                None => p.as_str().unwrap_or("").to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn message_text(m: &Value) -> String {
    content_text(m.get("content"))
}

/// 把 OpenAI 的 messages 压成一段聊天转录（chat 额度那条路只有「一个输入框」）
fn transcript(messages: &[Value]) -> String {
    let mut sys: Vec<String> = Vec::new();
    let mut turns: Vec<(&'static str, String)> = Vec::new();
    for m in messages {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        let text = message_text(m);
        if text.trim().is_empty() {
            continue;
        }
        match role {
            "system" | "developer" => sys.push(text),
            "assistant" => turns.push(("Assistant", text)),
            "tool" | "function" => turns.push(("Tool", text)),
            _ => turns.push(("User", text)),
        }
    }
    let mut out = String::new();
    if !sys.is_empty() {
        out.push_str(&sys.join("\n\n"));
        out.push_str("\n\n");
    }
    // 常见情形：就一句用户消息 —— 别加标签，原样发过去最自然
    if turns.len() == 1 && turns[0].0 == "User" {
        out.push_str(&turns[0].1);
        return out;
    }
    let body = turns
        .iter()
        .map(|(role, text)| format!("{role}: {text}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    out.push_str(&body);
    out
}

/// codex responses 支持真正的多轮：逐条翻成 input 项，system 折进 instructions
fn codex_input(messages: &[Value]) -> (Value, String) {
    let mut items: Vec<Value> = Vec::new();
    let mut sys: Vec<String> = Vec::new();
    for m in messages {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        let text = message_text(m);
        if text.trim().is_empty() {
            continue;
        }
        match role {
            "system" | "developer" => sys.push(text),
            "tool" | "function" => items.push(json!({
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": format!("[工具结果]\n{text}") }],
            })),
            "assistant" => items.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "input_text", "text": text }],
            })),
            _ => items.push(json!({
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": text }],
            })),
        }
    }
    (Value::Array(items), sys.join("\n\n"))
}
/// 一次请求要干的活
enum Job {
    Chat {
        prompt: String,
    },
    Codex {
        input: Value,
        model: String,
        instructions: String,
    },
}

/// 干完之后的成品
struct Reply {
    text: String,
    usage: Option<Value>,
    /// 这一轮生成的图（只有 chat 那条路会有）
    images: Vec<Value>,
}

impl Reply {
    /// 生图那一轮没有正文。给客户端一句说明，别交一个空回答出去
    /// （dsh 的 retryPolicy 把 EMPTY_RESPONSE 当可重试错误，会白花两份额度）。
    fn text_or_image_note(&self) -> String {
        if !self.text.trim().is_empty() || self.images.is_empty() {
            return self.text.clone();
        }
        let ids = self
            .images
            .iter()
            .filter_map(|i| i["file_id"].as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "（这一轮没有文字，只生成了 {} 张图：{ids}。取图用 POST /v1/images/generations 或 lchatgpt ask --save-images <目录>）",
            self.images.len()
        )
    }
}

/// 把 codex 的 {input_tokens,output_tokens} 翻成 OpenAI 的 {prompt,completion,total}_tokens
fn openai_usage(u: Option<&Value>) -> Option<Value> {
    let u = u?;
    let input = u.get("input_tokens").and_then(Value::as_u64)?;
    let output = u
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let total = u
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(input + output);
    Some(json!({
        "prompt_tokens": input,
        "completion_tokens": output,
        "total_tokens": total,
    }))
}

/// 累计全文 → 新增量。网页那条路每次只能读到整段答案，增量得自己算。
#[derive(Default)]
struct Diff {
    sent: usize,
}

impl Diff {
    fn next(&mut self, text: &str) -> Option<String> {
        if text.len() <= self.sent {
            return None;
        }
        let mut from = self.sent;
        while from < text.len() && !text.is_char_boundary(from) {
            from += 1;
        }
        let delta = text[from..].to_string();
        self.sent = text.len();
        if delta.is_empty() {
            None
        } else {
            Some(delta)
        }
    }
}

/// 请求线程专用的 Context：进度事件丢给一个转发线程写 stderr，别让它静默堆在 channel 里
fn worker_ctx(tag: &str) -> Context {
    let (tx, rx) = std::sync::mpsc::channel::<Progress>();
    let tag = tag.to_string();
    std::thread::spawn(move || {
        while let Ok(p) = rx.recv() {
            if let Progress::Log { level, message } = p {
                if level != LogLevel::Debug {
                    eprintln!("[serve {tag}] {message}");
                }
            }
        }
    });
    Context::from_sender(tx)
}

/// 真正干活：ureq / CDP 都是同步的，所以整个 job 在 spawn_blocking 里跑
fn run_job(cfg: &Config, job: &Job, sink: &mut dyn FnMut(&str)) -> Result<Reply, AppError> {
    let mut diff = Diff::default();
    let value = match job {
        Job::Chat { prompt } => {
            let ctx = worker_ctx("chat");
            let mut on = |text: &str, _done: bool| {
                if let Some(d) = diff.next(text) {
                    sink(&d);
                }
            };
            chat::ask_streaming(prompt, &cfg.cdp, &ctx, &mut on)?
        }
        Job::Codex {
            input,
            model,
            instructions,
        } => {
            let ctx = worker_ctx("codex");
            let mut on = |text: &str| {
                if let Some(d) = diff.next(text) {
                    sink(&d);
                }
            };
            ask::stream_codex(input, model, &cfg.effort, instructions, &ctx, &mut on)?
        }
    };
    Ok(Reply {
        text: value["text"].as_str().unwrap_or("").to_string(),
        usage: openai_usage(value.get("usage")),
        images: value
            .get("images")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    })
}

fn chunk(id: &str, created: i64, model: &str, content: &str, finish: Option<&str>) -> String {
    let delta = if content.is_empty() {
        json!({})
    } else {
        json!({ "content": content })
    };
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }],
    })
    .to_string()
}

fn last_chunk(id: &str, created: i64, model: &str, usage: Option<Value>) -> String {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
        "usage": usage,
    })
    .to_string()
}

fn api_error(status: StatusCode, message: impl Into<String>, kind: &str) -> Response {
    (
        status,
        Json(json!({
            "error": { "message": message.into(), "type": kind, "code": null }
        })),
    )
        .into_response()
}

fn error_response(e: &AppError) -> Response {
    match e {
        AppError::InvalidArg(_) | AppError::InvalidInput(_) => api_error(
            StatusCode::BAD_REQUEST,
            format!("{e}"),
            "invalid_request_error",
        ),
        AppError::Cancelled => api_error(StatusCode::SERVICE_UNAVAILABLE, "已取消", "cancelled"),
        _ => api_error(StatusCode::BAD_GATEWAY, format!("{e}"), "upstream_error"),
    }
}
/// POST /v1/images/generations —— 用 **chat 额度** 生图。
///
/// 网页那边生图就是「往输入框里问一句画图的请求」：图在会话里以
/// `image_asset_pointer` 出现，正文往往是空的。所以这条路是三步：
///
/// 1. 走 `chat::ask`（花 chat 额度；codex 额度在这一层完全不参与）；
/// 2. 把结果里 `images[].file_id` 逐个 `Api::download_file` 取回字节；
/// 3. 按 OpenAI 的形状回 `{ created, data: [{ b64_json }] }`。
///
/// 只回 `b64_json`：后端给的 `download_url` 带签名、且**必须带登录态头**才能
/// 取，直接甩给客户端是取不到的，所以这里把字节编码进响应体。
async fn images_generations(State(cfg): State<Arc<Config>>, Json(body): Json<Value>) -> Response {
    let prompt = body
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if prompt.is_empty() {
        return api_error(
            StatusCode::BAD_REQUEST,
            "prompt 不能为空",
            "invalid_request_error",
        );
    }
    let format = body
        .get("response_format")
        .and_then(Value::as_str)
        .unwrap_or("b64_json")
        .trim()
        .to_string();
    if format != "b64_json" {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("这个端只回 b64_json（收到 response_format={format:?}）：后端给的图片地址带签名、且必须带登录态头才能取，直接给 URL 客户端也打不开"),
            "invalid_request_error",
        );
    }

    let cfg2 = cfg.clone();
    let joined = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let ctx = worker_ctx("images");
        let asked = chat::ask(&prompt, &cfg2.cdp, &ctx)?;
        let picked: Vec<(String, Option<String>)> = asked
            .get("images")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| {
                        let id = i.get("file_id")?.as_str()?.to_string();
                        let caption = i
                            .get("caption")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        Some((id, caption))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if picked.is_empty() {
            let text = asked.get("text").and_then(Value::as_str).unwrap_or("");
            return Err(AppError::InvalidInput(format!(
                "这一轮没有生成图片（网页回的是文字：{}）。提示词里把「画一张……」写清楚；只想要文字回答的话用 /v1/chat/completions",
                util::clip(text, 200)
            )));
        }
        // 下载图要登录态：chat 路用的是浏览器凭据（只读），而 /files/... 走的是
        // 读接口，所以 auth::load() 就够，不需要 load_for_send()
        let api = Api::new(refresh::ensure_fresh(auth::load()?));
        let mut data = Vec::with_capacity(picked.len());
        for (file_id, caption) in picked {
            let bytes = api.download_file(&file_id)?;
            data.push(json!({
                "b64_json": util::base64_encode(&bytes),
                "revised_prompt": caption,
            }));
        }
        Ok(json!({ "created": store::now_secs(), "data": data }))
    })
    .await;

    match joined {
        Ok(Ok(out)) => Json(out).into_response(),
        Ok(Err(e)) => error_response(&e),
        Err(e) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("生图线程崩了: {e}"),
            "internal_error",
        ),
    }
}

/// 流里往客户端推的东西
enum Part {
    Delta(String),
    Done(Box<Reply>),
    Failed(String),
}

async fn chat_completions(State(cfg): State<Arc<Config>>, Json(body): Json<Value>) -> Response {
    let asked = body.get("model").and_then(Value::as_str).unwrap_or("");
    let route = match pick_route(asked, &cfg) {
        Ok(r) => r,
        Err(msg) => return api_error(StatusCode::BAD_REQUEST, msg, "invalid_request_error"),
    };
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if messages.is_empty() {
        return api_error(
            StatusCode::BAD_REQUEST,
            "messages 不能为空",
            "invalid_request_error",
        );
    }
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    let job = match route {
        Route::Chat => {
            let prompt = transcript(&messages);
            if prompt.trim().is_empty() {
                return api_error(
                    StatusCode::BAD_REQUEST,
                    "messages 里没有任何文本内容",
                    "invalid_request_error",
                );
            }
            Job::Chat { prompt }
        }
        Route::Codex => {
            let (input, sys) = codex_input(&messages);
            let instructions = if sys.trim().is_empty() {
                ask::default_instructions().to_string()
            } else {
                sys
            };
            // codex responses 对 ChatGPT 账号只认一个 slug，客户端写什么都替成它
            let model = match ask::resolve_model("") {
                Ok(m) => m,
                Err(e) => return error_response(&e),
            };
            Job::Codex {
                input,
                model,
                instructions,
            }
        }
    };

    let created = store::now_secs();
    let id = format!("chatcmpl-lchatgpt-{created}");
    let echo_model = if asked.trim().is_empty() {
        MODEL_CHAT.to_string()
    } else {
        asked.to_string()
    };

    if !stream {
        // 非流式：等全文生成完再一次给出 —— 好处是出错能回真正的 HTTP 状态码
        let cfg2 = cfg.clone();
        let joined = tokio::task::spawn_blocking(move || run_job(&cfg2, &job, &mut |_| {})).await;
        return match joined {
            Ok(Ok(reply)) => Json(json!({
                "id": id,
                "object": "chat.completion",
                "created": created,
                "model": echo_model,
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": reply.text_or_image_note() },
                    "finish_reason": "stop",
                }],
                "usage": reply.usage,
            }))
            .into_response(),
            Ok(Err(e)) => error_response(&e),
            Err(e) => api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("生成线程崩了: {e}"),
                "internal_error",
            ),
        };
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Part>(64);
    let cfg2 = cfg.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let outcome = run_job(&cfg2, &job, &mut |d| {
            let _ = tx.blocking_send(Part::Delta(d.to_string()));
        });
        let tail = match outcome {
            Ok(reply) => Part::Done(Box::new(reply)),
            Err(e) => Part::Failed(format!("{e}")),
        };
        let _ = tx.blocking_send(tail);
    });

    let head_id = id.clone();
    let head_model = echo_model.clone();
    let stream_body = async_stream::stream! {
        // 先甩一个空 delta：有些客户端要等第一个 chunk 才认连接已经建立
        yield Ok::<Event, Infallible>(
            Event::default().data(chunk(&head_id, created, &head_model, "", None)),
        );
        while let Some(part) = rx.recv().await {
            match part {
                Part::Delta(d) => {
                    yield Ok(Event::default()
                        .data(chunk(&head_id, created, &head_model, &d, None)));
                }
                Part::Done(reply) => {
                    yield Ok(Event::default()
                        .data(last_chunk(&head_id, created, &head_model, reply.usage.clone())));
                    break;
                }
                Part::Failed(msg) => {
                    // OpenAI SDK 认这个形状：把失败如实交出去，不要伪造一个空回答
                    yield Ok(Event::default().data(
                        json!({ "error": { "message": msg, "type": "lchatgpt_error" } })
                            .to_string(),
                    ));
                    break;
                }
            }
        }
        yield Ok(Event::default().data("[DONE]"));
        let _ = worker.await;
    };

    // keep-alive 很重要：chat 那条路第一段文字要等十几秒，中间得有心跳，
    // 否则客户端（dsh 的 streamIdleTimeout 默认 5 分钟）会以为连接死了
    Sse::new(stream_body)
        .keep_alive(KeepAlive::default())
        .into_response()
}
#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(allow_codex: bool) -> Config {
        Config {
            cdp: String::new(),
            allow_codex,
            effort: "low".into(),
        }
    }

    #[test]
    fn the_model_ids_come_from_the_config() {
        assert_eq!(model_ids(&cfg(false)), vec![MODEL_CHAT]);
        assert_eq!(model_ids(&cfg(true)), vec![MODEL_CHAT, MODEL_CODEX]);
    }

    /// 没加 --allow-codex 时 codex 那条路必须根本请求不到 —— 额度归属不能靠「客户端乖」
    #[test]
    fn only_the_chat_route_is_open_without_the_codex_flag() {
        let c = cfg(false);
        assert_eq!(pick_route("lchatgpt:chat", &c).ok(), Some(Route::Chat));
        assert_eq!(pick_route("chat", &c).ok(), Some(Route::Chat));
        assert_eq!(pick_route("", &c).ok(), Some(Route::Chat));
        let err = pick_route("lchatgpt:codex", &c).expect_err("没挂 codex 额度就该拒绝");
        assert!(err.contains("--allow-codex"), "{err}");
        assert_eq!(
            pick_route("lchatgpt:codex", &cfg(true)).ok(),
            Some(Route::Codex)
        );
    }

    #[test]
    fn an_unknown_model_lists_what_this_endpoint_offers() {
        let err = pick_route("gpt-6.2-sol", &cfg(false)).expect_err("别的模型不该被悄悄接受");
        assert!(err.contains(MODEL_CHAT), "{err}");
    }

    #[test]
    fn a_single_user_message_travels_without_any_role_prefix() {
        let msgs = vec![json!({"role": "user", "content": "你好"})];
        assert_eq!(transcript(&msgs), "你好");
    }

    #[test]
    fn a_multi_turn_history_is_flattened_with_role_prefixes() {
        let msgs = vec![
            json!({"role": "system", "content": "只回答一个字"}),
            json!({"role": "user", "content": "几点了"}),
            json!({"role": "assistant", "content": "早"}),
            json!({"role": "user", "content": [{"type": "text", "text": "现在呢"}]}),
        ];
        let t = transcript(&msgs);
        assert!(t.starts_with("只回答一个字\n\nUser: 几点了"), "{t}");
        assert!(t.ends_with("User: 现在呢"), "{t}");
    }

    #[test]
    fn the_codex_route_keeps_the_turns_apart_and_folds_system_into_instructions() {
        let msgs = vec![
            json!({"role": "system", "content": "简洁"}),
            json!({"role": "user", "content": "一加一"}),
            json!({"role": "assistant", "content": "二"}),
        ];
        let (input, sys) = codex_input(&msgs);
        assert_eq!(sys, "简洁");
        let items = input.as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["role"], "user");
        assert_eq!(items[1]["role"], "assistant");
        assert_eq!(items[0]["content"][0]["type"], "input_text");
    }

    #[test]
    fn the_diff_only_hands_out_the_new_tail() {
        let mut d = Diff::default();
        assert_eq!(d.next("你"), Some("你".to_string()));
        assert_eq!(d.next("你好"), Some("好".to_string()));
        assert_eq!(d.next("你好"), None);
        // 多字节边界不能被切坏
        assert_eq!(d.next("你好，世界"), Some("，世界".to_string()));
    }

    #[test]
    fn codex_usage_is_renamed_to_the_openai_field_names() {
        let u = json!({"input_tokens": 3, "output_tokens": 7, "total_tokens": 10});
        let o = openai_usage(Some(&u)).unwrap();
        assert_eq!(o["prompt_tokens"], 3);
        assert_eq!(o["completion_tokens"], 7);
        assert_eq!(o["total_tokens"], 10);
        assert!(openai_usage(None).is_none());
    }

    #[test]
    fn a_streamed_chunk_carries_the_delta_and_a_finish_chunk_closes_it() {
        let c = chunk("id1", 1, MODEL_CHAT, "好", None);
        let v: Value = serde_json::from_str(&c).unwrap();
        assert_eq!(v["choices"][0]["delta"]["content"], "好");
        assert!(v["choices"][0]["finish_reason"].is_null());
        let end = last_chunk("id1", 1, MODEL_CHAT, None);
        let v: Value = serde_json::from_str(&end).unwrap();
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
    }
}
