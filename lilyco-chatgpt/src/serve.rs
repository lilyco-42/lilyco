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

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::convert::Infallible;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use axum::extract::Path;
use axum::http::{header, StatusCode};
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

/// 新开/滚动会话时单条文本的字符上限：超过这个长度会折叠中间历史，
/// 防止 ChatGPT 网页输入框拒收或直接把会话强制关闭。
const MAX_CHAT_CHARS: usize = 120_000;
/// 内存里最多记住多少条「客户端会话 → ChatGPT 网页会话」绑定
const MAX_BOUND_SESSIONS: usize = 64;

/// 生图在 /v1/chat/completions 里自动落盘的目录（同时通过 GET /images/<文件名> 提供访问）
const SERVE_IMAGE_DIR: &str = "/tmp/lchatgpt-images";

/// 一条已绑定的网页会话：后续轮次只往这条 `/c/<id>` 发增量消息
#[derive(Clone, Debug)]
struct BoundSession {
    conversation_id: String,
    /// 已经发进这条网页会话的非 system 轮次数（包含上一轮已生成的 assistant 回复）
    sent_turns: usize,
    /// 上一轮结束时活动分支的节点数，续聊直接复用，避免发前再 GET 一次触发 429
    branch_len: usize,
}

/// 一个端口进程共享的设置
struct Config {
    cdp: String,
    allow_codex: bool,
    effort: String,
    base_origin: String,
    sessions: Mutex<HashMap<u64, BoundSession>>,
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
        base_origin: format!("http://{bind}"),
        sessions: Mutex::new(HashMap::new()),
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
        .route("/images/{name}", get(serve_image))
        .with_state(cfg)
}

async fn serve_image(Path(name): Path<String>) -> Response {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return api_error(StatusCode::BAD_REQUEST, "非法图片文件名", "invalid_request_error");
    }
    let path = std::path::Path::new(SERVE_IMAGE_DIR).join(&name);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let (mime, _) = util::sniff_image_mime(&bytes);
            ([(header::CONTENT_TYPE, mime)], bytes).into_response()
        }
        Err(_) => api_error(StatusCode::NOT_FOUND, "图片不存在", "not_found"),
    }
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
/// 这笔请求该花哪份额度（chat 路径可附带网页模型与推理强度）
#[derive(Clone, Debug, PartialEq)]
enum Route {
    Chat {
        web_model: Option<String>,
        effort: Option<String>,
    },
    Codex,
}

fn pick_route(model: &str, cfg: &Config) -> Result<Route, String> {
    let bare = model.trim();
    let (has_prefix, tail) = if let Some(t) = bare
        .strip_prefix("lchatgpt:")
        .or_else(|| bare.strip_prefix("lchatgpt-"))
        .or_else(|| bare.strip_prefix("lchatgpt/"))
    {
        (true, t)
    } else {
        (false, bare)
    };
    let route = match tail {
        "" | "chat" => Route::Chat {
            web_model: None,
            effort: None,
        },
        "codex" => Route::Codex,
        other => {
            if let Some(eff) = other.strip_prefix("chat:") {
                chat::parse_chat_effort(eff).map_err(|e| format!("{e}"))?;
                Route::Chat {
                    web_model: None,
                    effort: Some(eff.trim().to_string()),
                }
            } else if has_prefix {
                // 支持 lchatgpt:<网页模型>[:<强度>]，如 lchatgpt:gpt-6-2:high / lchatgpt:gpt-6:instant
                let (m, eff) = match other.split_once(':') {
                    Some((m, e)) => {
                        chat::parse_chat_effort(e).map_err(|err| format!("{err}"))?;
                        (m.trim(), Some(e.trim().to_string()))
                    }
                    None => (other.trim(), None),
                };
                if m.is_empty() {
                    return Err("模型名不能为空".to_string());
                }
                Route::Chat {
                    web_model: Some(m.to_string()),
                    effort: eff,
                }
            } else {
                let known = model_ids(cfg).join(" / ");
                return Err(format!(
                    "不认识的模型 {other:?}；这个端只提供 {known}（也支持 lchatgpt:<模型>:<强度> 如 lchatgpt:gpt-6-2:high）"
                ));
            }
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
/// 按字符数估算 token（CJK 约 1.5 token/字，ASCII 约 0.25 token/字）。
/// 网页端不返回真实 usage，不填的话 dsh / zerostack 会一直显示 0% 且永不自动压缩。
fn estimate_tokens(text: &str) -> u64 {
    if text.trim().is_empty() {
        return 0;
    }
    let mut cjk = 0u64;
    let mut other = 0u64;
    for c in text.chars() {
        if ('\u{4E00}'..='\u{9FFF}').contains(&c)
            || ('\u{3400}'..='\u{4DBF}').contains(&c)
            || ('\u{3000}'..='\u{303F}').contains(&c)
            || ('\u{FF00}'..='\u{FFEF}').contains(&c)
        {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    ((cjk * 3).div_ceil(2) + other.div_ceil(4)).max(1)
}

fn estimated_usage(prompt: &str, completion: &str) -> Value {
    let p = estimate_tokens(prompt);
    let c = estimate_tokens(completion);
    json!({
        "prompt_tokens": p,
        "completion_tokens": c,
        "total_tokens": p + c,
    })
}

/// 拆分 system 与非 system 轮次，供会话锚点、增量发送与完整转录共用
fn split_chat_messages(messages: &[Value]) -> (Vec<String>, Vec<(&'static str, String)>) {
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
    (sys, turns)
}

/// 生成会话锚点：取 system 前 200 字符（不含尾部动态时间戳）+ 第一条 User 消息。
/// 这样 dsh 每轮刷新 time-context 时，依然能稳定命中同一条 ChatGPT 网页会话。
fn session_anchor(sys: &[String], turns: &[(&'static str, String)]) -> Option<u64> {
    let first_user = turns.iter().find(|(r, _)| *r == "User").map(|(_, t)| t)?;
    let mut h = DefaultHasher::new();
    if let Some(s) = sys.first() {
        let head: String = s.chars().take(200).collect();
        head.hash(&mut h);
    }
    first_user.hash(&mut h);
    Some(h.finish())
}

/// 把增量轮次压成发往已有会话的一段文本（单条 User 直接发原文，多条带标签）
fn format_delta_turns(turns: &[(&'static str, String)]) -> String {
    if turns.len() == 1 && turns[0].0 == "User" {
        return turns[0].1.clone();
    }
    turns
        .iter()
        .map(|(r, t)| format!("{r}: {t}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 超长兜底：新开或滚动会话时如果整段转录超过 `MAX_CHAT_CHARS`，
/// 保留头部（system 身份）与尾部（最近几轮），折叠中间过老内容，防止网页强关。
fn cap_transcript(prompt: &str) -> String {
    let total = prompt.chars().count();
    if total <= MAX_CHAT_CHARS {
        return prompt.to_string();
    }
    let head_keep = 36_000;
    let tail_keep = MAX_CHAT_CHARS.saturating_sub(head_keep + 200);
    let head: String = prompt.chars().take(head_keep).collect();
    let tail: String = prompt
        .chars()
        .skip(total.saturating_sub(tail_keep))
        .collect();
    let omitted = total.saturating_sub(head_keep + tail_keep);
    format!("{head}\n\n[... 已折叠中间约 {omitted} 字符的较早历史，以防超出 ChatGPT 网页上下文上限 ...]\n\n{tail}")
}

/// 判断最新一条用户消息是否属于「直接请求生图」：
///
/// 像 dsh 这样的宿主会在 `messages` 前面注入 10 万字编程 Agent 系统提示词，
/// 直接一起发给 ChatGPT 会干扰内置画图工具（实测常回「生成未成功，请上传原图」）。
/// 识别到明确的生图意图后，自动剥离宿主系统提示词，只把纯净的画图指令新开会话发送。
/// 同时排除「修复无法生成图片」「为什么生图失败」「Create a concise title」这类非画图请求。
fn is_image_gen_request(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 800 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    // 排除宿主标题生成、排查/修复/提问类文本，防止误伤
    if lower.starts_with("create a concise title")
        || lower.starts_with("/goal")
        || compact.contains("无法生成图片")
        || compact.contains("生成图片失败")
        || compact.contains("修复")
        || compact.contains("报错")
        || compact.contains("为什么")
        || compact.contains("代码")
        || lower.contains("bug")
    {
        return false;
    }
    const ZH_PREFIXES: &[&str] = &[
        "画一",
        "画个",
        "画幅",
        "帮我画",
        "请画",
        "给我画",
        "画图",
        "作图",
        "生图",
        "生成一",
        "生成图",
        "帮我生成",
        "请生成",
        "给我生成",
        "来一张",
        "来一只",
    ];
    if ZH_PREFIXES.iter().any(|p| compact.starts_with(p)) {
        return true;
    }
    const ZH_KEYWORDS: &[&str] = &[
        "画一张",
        "画一只",
        "画一个",
        "画一幅",
        "生成图片",
        "生成一张",
        "生成一只",
        "生成照片",
        "生成插画",
        "生成头像",
        "生成壁纸",
        "生成海报",
    ];
    if ZH_KEYWORDS.iter().any(|k| compact.contains(k)) {
        return true;
    }
    const EN_PREFIXES: &[&str] = &[
        "draw ",
        "paint ",
        "illustrate ",
        "generate an image",
        "generate a picture",
        "generate a photo",
        "create an image",
        "create a picture",
        "make an image",
    ];
    EN_PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// 一次请求要干的活
enum Job {
    Chat {
        /// 完整转录（用于新开会话、超限滚动兜底、以及估算总 prompt_tokens）
        full_prompt: String,
        /// 客户端会话指纹
        anchor: Option<u64>,
        /// 如果命中已有会话，只发这些新增轮次
        delta_prompt: Option<String>,
        /// 本次请求里的非 system 轮次数
        total_turns: usize,
        /// 命中的已有 ChatGPT 网页会话 id
        bound_conversation_id: Option<String>,
        /// 命中会话已知的分支节点数（避免续聊前多发一次 GET 撞 429）
        bound_branch_len: Option<usize>,
        /// 可选切换网页模型（如 `gpt-6-2` / `gpt-6` / `gpt-5-6`）
        web_model: Option<String>,
        /// 可选切换网页推理强度（`instant` / `medium` / `high`）
        effort: Option<String>,
    },
    Codex {
        input: Value,
        model: String,
        instructions: String,
        effort: String,
    },
}

/// 已自动下载落盘并挂到 `/images/<name>` 的生成图
struct SavedImage {
    file_id: String,
    caption: Option<String>,
    local_path: String,
    url: String,
}

/// 干完之后的成品
struct Reply {
    text: String,
    usage: Option<Value>,
    /// 这一轮生成的图（只有 chat 那条路会有）
    images: Vec<Value>,
    /// 已自动落盘并提供本地 HTTP 访问的图片
    saved_images: Vec<SavedImage>,
}

impl Reply {
    /// 生图那一轮没有正文。自动附上已落盘图片链接与路径，别交一个空回答出去
    /// （dsh 的 retryPolicy 把 EMPTY_RESPONSE 当可重试错误，会白花两份额度）。
    fn text_or_image_note(&self) -> String {
        if !self.text.trim().is_empty() {
            return self.text.clone();
        }
        if !self.saved_images.is_empty() {
            let mut lines = Vec::with_capacity(self.saved_images.len() + 1);
            for img in &self.saved_images {
                let alt = img
                    .caption
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&img.file_id);
                lines.push(format!("![{alt}]({})", img.url));
                lines.push(format!("- 已保存到本地：`{}`", img.local_path));
            }
            return lines.join("\n");
        }
        if self.images.is_empty() {
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

/// 把 chat 生图结果里的 file_id 自动下载到 `SERVE_IMAGE_DIR`，供 `/images/<name>` 直接展示
fn save_chat_images(base_origin: &str, images: &[Value], ctx: &Context) -> Vec<SavedImage> {
    if images.is_empty() {
        return Vec::new();
    }
    if std::fs::create_dir_all(SERVE_IMAGE_DIR).is_err() {
        return Vec::new();
    }
    let creds = match auth::load() {
        Ok(c) => refresh::ensure_fresh(c),
        Err(e) => {
            ctx.log(LogLevel::Warn, format!("下载生图跳过（读凭证失败: {e}）"));
            return Vec::new();
        }
    };
    let api = Api::new(creds);
    let mut out = Vec::new();
    for img in images {
        let Some(file_id) = img.get("file_id").and_then(Value::as_str) else {
            continue;
        };
        let caption = img
            .get("caption")
            .and_then(Value::as_str)
            .map(str::to_string);
        match api.download_file(file_id) {
            Ok(bytes) => {
                let (_, ext) = util::sniff_image_mime(&bytes);
                let filename = format!("{file_id}.{ext}");
                let path = std::path::Path::new(SERVE_IMAGE_DIR).join(&filename);
                if std::fs::write(&path, &bytes).is_ok() {
                    out.push(SavedImage {
                        file_id: file_id.to_string(),
                        caption,
                        local_path: path.display().to_string(),
                        url: format!("{base_origin}/images/{filename}"),
                    });
                }
            }
            Err(e) => {
                ctx.log(
                    LogLevel::Warn,
                    format!("自动下载生图 {file_id} 失败: {e}"),
                );
            }
        }
    }
    out
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
    let (value, chat_prompt_for_usage) = match job {
        Job::Chat {
            full_prompt,
            anchor,
            delta_prompt,
            total_turns,
            bound_conversation_id,
            bound_branch_len,
            web_model,
            effort,
        } => {
            let ctx = worker_ctx("chat");
            let mut on = |text: &str, _done: bool| {
                if let Some(d) = diff.next(text) {
                    sink(&d);
                }
            };
            let res = match (bound_conversation_id.as_deref(), delta_prompt.as_deref()) {
                (Some(cid), Some(delta)) if !delta.trim().is_empty() => {
                    let cont_opts = chat::ChatOptions {
                        conversation_id: Some(cid),
                        known_branch_len: *bound_branch_len,
                        model: web_model.as_deref(),
                        effort: effort.as_deref(),
                    };
                    match chat::ask_with_options_streaming(
                        cont_opts, delta, &cfg.cdp, &ctx, &mut on,
                    ) {
                        Ok(v) => Ok(v),
                        Err(e) => {
                            ctx.log(
                                LogLevel::Warn,
                                format!("续接会话 {cid} 失败（{e}），自动滚动新开会话继续"),
                            );
                            if let Some(key) = anchor {
                                if let Ok(mut map) = cfg.sessions.lock() {
                                    map.remove(key);
                                }
                            }
                            let capped = cap_transcript(full_prompt);
                            let fresh_opts = chat::ChatOptions {
                                model: web_model.as_deref(),
                                effort: effort.as_deref(),
                                ..Default::default()
                            };
                            chat::ask_with_options_streaming(
                                fresh_opts, &capped, &cfg.cdp, &ctx, &mut on,
                            )
                        }
                    }
                }
                _ => {
                    let capped = cap_transcript(full_prompt);
                    let fresh_opts = chat::ChatOptions {
                        model: web_model.as_deref(),
                        effort: effort.as_deref(),
                        ..Default::default()
                    };
                    chat::ask_with_options_streaming(
                        fresh_opts, &capped, &cfg.cdp, &ctx, &mut on,
                    )
                }
            }?;
            if let (Some(key), Some(cid)) = (
                anchor,
                res.get("conversation_id").and_then(Value::as_str),
            ) {
                if !cid.is_empty() {
                    let branch_len = res
                        .get("branch_len")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    if let Ok(mut map) = cfg.sessions.lock() {
                        if map.len() >= MAX_BOUND_SESSIONS && !map.contains_key(key) {
                            map.clear();
                        }
                        map.insert(
                            *key,
                            BoundSession {
                                conversation_id: cid.to_string(),
                                // +1 代表本轮即将回给客户端的 assistant 回复
                                sent_turns: total_turns + 1,
                                branch_len,
                            },
                        );
                    }
                }
            }
            (res, Some(full_prompt.as_str()))
        }
        Job::Codex {
            input,
            model,
            instructions,
            effort,
        } => {
            let ctx = worker_ctx("codex");
            let mut on = |text: &str| {
                if let Some(d) = diff.next(text) {
                    sink(&d);
                }
            };
            (
                ask::stream_codex(input, model, effort, instructions, &ctx, &mut on)?,
                None,
            )
        }
    };
    let text = value["text"].as_str().unwrap_or("").to_string();
    let images = value
        .get("images")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let saved_images = if images.is_empty() {
        Vec::new()
    } else {
        let ctx = worker_ctx("images");
        save_chat_images(&cfg.base_origin, &images, &ctx)
    };
    let usage = openai_usage(value.get("usage"))
        .or_else(|| chat_prompt_for_usage.map(|p| estimated_usage(p, &text)));
    Ok(Reply {
        text,
        usage,
        images,
        saved_images,
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
    // 支持 OpenAI 标准字段 reasoning_effort 或 reasoning.effort（dsh / zerostack 切推理强度时发这个）
    let req_effort = body
        .get("reasoning_effort")
        .or_else(|| body.pointer("/reasoning/effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let job = match route {
        Route::Chat {
            web_model,
            effort: route_effort,
        } => {
            let effort = req_effort.or(route_effort);
            if let Some(e) = effort.as_deref() {
                if let Err(err) = chat::parse_chat_effort(e) {
                    return error_response(&err);
                }
            }
            let full_prompt = transcript(&messages);
            if full_prompt.trim().is_empty() {
                return api_error(
                    StatusCode::BAD_REQUEST,
                    "messages 里没有任何文本内容",
                    "invalid_request_error",
                );
            }
            let (sys, turns) = split_chat_messages(&messages);
            let last_user = turns
                .iter()
                .rev()
                .find(|(r, _)| *r == "User")
                .map(|(_, t)| t.as_str());
            if let Some(img_prompt) = last_user.filter(|t| is_image_gen_request(t)) {
                eprintln!(
                    "[serve chat] 识别为生图请求，已剥离宿主系统提示词，使用纯净提示词新开会话生图"
                );
                Job::Chat {
                    full_prompt: img_prompt.to_string(),
                    anchor: None,
                    delta_prompt: None,
                    total_turns: 1,
                    bound_conversation_id: None,
                    bound_branch_len: None,
                    web_model,
                    effort,
                }
            } else {
                let anchor = session_anchor(&sys, &turns);
                let (bound_conversation_id, bound_branch_len, delta_prompt) = anchor
                    .and_then(|key| cfg.sessions.lock().ok()?.get(&key).cloned())
                    .filter(|b| turns.len() > b.sent_turns)
                    .map(|b| {
                        let delta = format_delta_turns(&turns[b.sent_turns..]);
                        (Some(b.conversation_id), Some(b.branch_len), Some(delta))
                    })
                    .unwrap_or((None, None, None));
                Job::Chat {
                    full_prompt,
                    anchor,
                    delta_prompt,
                    total_turns: turns.len(),
                    bound_conversation_id,
                    bound_branch_len,
                    web_model,
                    effort,
                }
            }
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
            let effort = match req_effort.as_deref() {
                Some(e) => match ask::resolve_effort(e) {
                    Ok(v) => v,
                    Err(err) => return error_response(&err),
                },
                None => cfg.effort.clone(),
            };
            Job::Codex {
                input,
                model,
                instructions,
                effort,
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
        let mut emitted_any = false;
        while let Some(part) = rx.recv().await {
            match part {
                Part::Delta(d) => {
                    if !d.is_empty() {
                        emitted_any = true;
                    }
                    yield Ok(Event::default()
                        .data(chunk(&head_id, created, &head_model, &d, None)));
                }
                Part::Done(reply) => {
                    // 生图那一轮正文是空的，流式期间没有发出任何 delta；
                    // 这里必须补发一条图片 Markdown / 说明，否则 dsh 会报 completed response with no content 并重试！
                    if !emitted_any {
                        let fallback = reply.text_or_image_note();
                        if !fallback.is_empty() {
                            yield Ok(Event::default()
                                .data(chunk(&head_id, created, &head_model, &fallback, None)));
                        }
                    }
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
            base_origin: "http://127.0.0.1:8790".into(),
            sessions: Mutex::new(HashMap::new()),
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
        let default_chat = Route::Chat {
            web_model: None,
            effort: None,
        };
        assert_eq!(pick_route("lchatgpt:chat", &c).ok(), Some(default_chat.clone()));
        assert_eq!(pick_route("chat", &c).ok(), Some(default_chat.clone()));
        assert_eq!(pick_route("", &c).ok(), Some(default_chat));
        assert_eq!(
            pick_route("lchatgpt:chat:high", &c).ok(),
            Some(Route::Chat {
                web_model: None,
                effort: Some("high".into()),
            })
        );
        assert_eq!(
            pick_route("lchatgpt:gpt-6-2:instant", &c).ok(),
            Some(Route::Chat {
                web_model: Some("gpt-6-2".into()),
                effort: Some("instant".into()),
            })
        );
        assert_eq!(
            pick_route("lchatgpt:gpt-6", &c).ok(),
            Some(Route::Chat {
                web_model: Some("gpt-6".into()),
                effort: None,
            })
        );
        assert!(pick_route("lchatgpt:chat:invalid_effort", &c).is_err());
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

    /// system 尾部动态时间戳变化时，会话锚点必须稳定不变
    #[test]
    fn session_anchor_ignores_dynamic_tail_in_system_prompt() {
        let sys_prefix = "You are an AI agent. ".repeat(15); // > 200 chars
        let t1_msgs = vec![
            json!({"role": "system", "content": format!("{sys_prefix}\nTime: 18:10")}),
            json!({"role": "user", "content": "帮我分析架构"}),
        ];
        let t2_msgs = vec![
            json!({"role": "system", "content": format!("{sys_prefix}\nTime: 18:15")}),
            json!({"role": "user", "content": "帮我分析架构"}),
            json!({"role": "assistant", "content": "好的"}),
            json!({"role": "user", "content": "继续"}),
        ];
        let (s1, u1) = split_chat_messages(&t1_msgs);
        let (s2, u2) = split_chat_messages(&t2_msgs);
        assert_eq!(session_anchor(&s1, &u1), session_anchor(&s2, &u2));
        assert_eq!(format_delta_turns(&u2[2..]), "继续");
    }

    #[test]
    fn estimated_usage_reports_nonzero_tokens() {
        let u = estimated_usage("你好世界 hello world", "ok");
        assert!(u["prompt_tokens"].as_u64().unwrap() >= 6);
        assert!(u["completion_tokens"].as_u64().unwrap() >= 1);
        assert_eq!(
            u["total_tokens"].as_u64().unwrap(),
            u["prompt_tokens"].as_u64().unwrap() + u["completion_tokens"].as_u64().unwrap()
        );
    }

    #[test]
    fn cap_transcript_folds_middle_when_over_limit() {
        let big = format!(
            "HEAD_START_{}_TAIL_END",
            "x".repeat(MAX_CHAT_CHARS + 5000)
        );
        let capped = cap_transcript(&big);
        assert!(capped.starts_with("HEAD_START_"));
        assert!(capped.ends_with("_TAIL_END"));
        assert!(capped.contains("已折叠中间"));
        assert!(capped.chars().count() <= MAX_CHAT_CHARS);
    }

    /// 生图请求要准确识别并剥离宿主系统提示词，但排查/修复/标题生成不能误伤
    #[test]
    fn image_gen_detector_matches_drawing_and_ignores_coding_prompts() {
        assert!(is_image_gen_request("画一只戴墨镜的橘猫"));
        assert!(is_image_gen_request("生成 一张 小狗图片"));
        assert!(is_image_gen_request("帮我画一个赛博朋克风格的城市海报"));
        assert!(is_image_gen_request("draw a cat in sunglasses"));
        assert!(is_image_gen_request("Generate an image of a Shiba Inu"));
        assert!(!is_image_gen_request(
            "/goal 修复本机 lchatgpt 无法生成图片的问题"
        ));
        assert!(!is_image_gen_request("为什么生成图片失败了？"));
        assert!(!is_image_gen_request(
            "Create a concise title for: 画一只戴墨镜的橘猫"
        ));
        assert!(!is_image_gen_request("hi"));
        assert!(!is_image_gen_request("帮我写一段 Rust 代码"));
    }
}
