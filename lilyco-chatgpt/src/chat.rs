//! `ask` 的**默认**发送路径 —— 驱动网页聊天，花 ChatGPT 的 chat 额度，不动 codex 额度。
//!
//! 为什么不能纯 HTTP：`POST /backend-api/conversation` 要求 turnstile + sentinel 的
//! 设备指纹（连真 token 也回 403 "Unusual activity"）。但**页面自己**发就不需要过这些门 ——
//! 所以这里借一个已经登录 chatgpt.com 的 chromium（ego-lite 的 CDP 端口优先），把字打进
//! 输入框、点亮发送按钮；读回复一律走 CLI 自己的 REST（带 Bearer），不碰 DOM。
//!
//! 实测时序（点发送起算）：页面 URL 立刻变成 `/c/local-chatgpt%3A<uuid>` —— 那个 id 是
//! **页面内部的**，拿它去 REST 只会 400/404（编不编码都不行）；REST 认得的那条 id 要等
//! 会话落进 `/backend-api/conversations` 列表，实测约 16 秒后才出现。所以这边认会话的办法是：
//! 发之前先记下列表里已有的 id，发之后轮询列表找**新冒出来的那条**，再核对它的最后一条
//! 用户消息确实是这次问的（防同时冒出别的会话顶替）。
//!
//! 认到 id 之后 `GET /backend-api/conversation/{id}` 3.7 秒就能读到（assistant 节点
//! `status=in_progress`），5.4 秒时 `status=finished_successfully`、`end_turn=true`。
//!
//! 所以「答完了没有」只看 REST 里的 `status / end_turn`，**绝不看 DOM 选择器**：
//! 消息节点没有任何稳定属性（CSS Module 类名全是哈希，`[data-message-author-role]` 早已失效）。
//!
//! 这条路径是可选的：没有浏览器在跑时它会明确报错，并告诉用户要么起浏览器、
//! 要么加 `--allow-codex` 显式改用 codex 额度 —— 绝不会偷偷花 codex 的配额。

use crate::api::Api;
use crate::auth;
use crate::browser::{self, Client};
use crate::refresh;
use crate::show;
use lilyco::prelude::*;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// 输入框就绪的最长等待（新标签页要加载整个 SPA）
const COMPOSER_TIMEOUT: Duration = Duration::from_secs(45);
/// 点发送后等「这次提问出现在会话列表里」的最长等待（实测约 16 秒，留足余量）
const CONVERSATION_TIMEOUT: Duration = Duration::from_secs(90);
/// 找新会话时两次列表查询之间歇多久
const LIST_INTERVAL: Duration = Duration::from_millis(2000);
/// 发消息前抓一次会话列表快照，最多试几次（这台机器上偶发 unexpected end of file）
const SNAPSHOT_ATTEMPTS: usize = 5;
/// 快照与轮询时取多少个最近的会话
const LIST_LIMIT: u64 = 20;
/// 等回答写完的最长等待
const POLL_TIMEOUT: Duration = Duration::from_secs(180);
/// 两次轮询之间歇多久（1.2 秒：实测 3 次采样就稳定，再密只是白打后端）
const POLL_INTERVAL: Duration = Duration::from_millis(1200);
/// 往输入框里写字最多试几次（CDP 的 insertText 会漏、React 的 state 会滞后，
/// 实测常在第 3~4 次才等到发送按钮变可点，这里留到 14 次）
const COMPOSE_ATTEMPTS: usize = 14;
/// insertText 之后等多久再读状态：React 收到 input 事件后要一拍才重算按钮的 disabled
const COMPOSE_SETTLE: Duration = Duration::from_millis(500);

/// 清空输入框并把焦点给它。必须走 Selection + deleteFromDocument：
/// 直接改 innerText 不会触发 React 的 state，发送按钮会一直是 disabled。
const CLEAR_JS: &str = r#"(() => {
  const el = document.querySelector('div[contenteditable="true"]');
  if (!el) return JSON.stringify({ ok: false, why: 'composer-missing' });
  el.focus();
  const sel = window.getSelection();
  if (sel) { sel.selectAllChildren(el); sel.deleteFromDocument(); }
  return JSON.stringify({ ok: true, text: el.innerText });
})()"#;

/// 读回输入框里的内容 + 发送按钮是否可点
const STATE_JS: &str = r#"(() => {
  const el = document.querySelector('div[contenteditable="true"]');
  const btn = document.querySelector('button[aria-label="发送"], button[aria-label="Send"], button[data-testid="send-button"]');
  return JSON.stringify({
    ok: true,
    text: el ? el.innerText : null,
    button: !!btn,
    enabled: btn ? !btn.disabled : false,
  });
})()"#;

/// 点发送。注意 `Input.dispatchKeyEvent` 造 Enter **发不出去**（实测 40 秒无任何反应），
/// 只有真点一下按钮才行。
const SEND_JS: &str = r#"(() => {
  const btn = document.querySelector('button[aria-label="发送"], button[aria-label="Send"], button[data-testid="send-button"]');
  if (!btn) return JSON.stringify({ ok: false, why: 'send-button-missing' });
  if (btn.disabled) return JSON.stringify({ ok: false, why: 'send-button-disabled' });
  btn.click();
  return JSON.stringify({ ok: true });
})()"#;

/// 输入框是否已经渲染出来
const READY_JS: &str = r#"(() => {
  const el = document.querySelector('div[contenteditable="true"]');
  return JSON.stringify({ ready: !!el, href: location.href, state: document.readyState });
})()"#;

/// 一次尝试的失败：`sent` 为真表示「消息可能已经发出去了」——
/// 这种情况绝不能换一个浏览器再发一遍，否则用户会得到两条一样的消息。
/// 生成过程中每次读到更新后的全文就回调一次：`(全文, 是否已收尾)`。
///
/// 参数是**累计全文**而不是增量 —— 网页那条路每次只能读到整段回答，增量要靠调用方
/// 自己 diff（`serve` 就是把它翻成 OpenAI 的 delta 流）。
pub type OnText<'a> = &'a mut dyn FnMut(&str, bool);

struct Attempt {
    error: AppError,
    sent: bool,
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

fn eval_json(client: &mut Client, expression: &str) -> Result<Value, AppError> {
    let v = client.eval_js(expression)?;
    match v {
        Value::String(s) => serde_json::from_str(&s)
            .map_err(|e| AppError::Runtime(format!("页面返回的不是 JSON: {e}"))),
        other => Ok(other),
    }
}

/// 从 `/c/<uuid>` 里取出会话 id；不在会话页就返回 None
fn conversation_id_from_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/c/")?;
    let id = rest.split('/').next().unwrap_or("");
    let id = id.split('?').next().unwrap_or("");
    if id.len() >= 8 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        Some(id.to_string())
    } else {
        None
    }
}

fn wait_for_composer(client: &mut Client) -> Result<(), AppError> {
    let deadline = Instant::now() + COMPOSER_TIMEOUT;
    let mut last = String::from("标签页还没开始加载");
    while Instant::now() < deadline {
        let probe = eval_json(client, READY_JS).unwrap_or(Value::Null);
        if probe.get("ready").and_then(Value::as_bool).unwrap_or(false) {
            return Ok(());
        }
        last = format!(
            "href={} readyState={}",
            probe.get("href").and_then(Value::as_str).unwrap_or("?"),
            probe.get("state").and_then(Value::as_str).unwrap_or("?")
        );
        std::thread::sleep(Duration::from_millis(750));
    }
    Err(AppError::Runtime(format!(
        "等了 {} 秒网页输入框还没出来（{last}）—— 这个浏览器可能没登录 chatgpt.com",
        COMPOSER_TIMEOUT.as_secs()
    )))
}

/// 把连续空白折成一个空格（多段落提示进 ProseMirror 再读回来，空行数量不一定守恒）
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn squash_len(s: &str) -> usize {
    squash(s).chars().count()
}

/// 输入框里的文字是否就是这段提示（按折叠空白后的内容比）
fn same_text(got: &str, want: &str) -> bool {
    !want.trim().is_empty() && squash(got) == squash(want)
}

/// 清空输入框，并且**确认真的空了**才返回。
///
/// 只调一次 Selection + deleteFromDocument 不够：ProseMirror 会把删空的段落重新补回来，
/// 长提示更是常常只删掉一部分 —— 残字留在里面，后面 insertText 插进去的就是残字加新字，
/// 于是永远比不出「一模一样」。
fn clear_composer(client: &mut Client) -> Result<(), AppError> {
    let mut last = String::from("清空没生效");
    for _ in 0..4 {
        let cleared = eval_json(client, CLEAR_JS)?;
        if !cleared.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            return Err(AppError::Runtime(
                "找不到 contenteditable 输入框".to_string(),
            ));
        }
        let left = cleared.get("text").and_then(Value::as_str).unwrap_or("");
        if left.trim().is_empty() {
            return Ok(());
        }
        last = format!("还剩 {} 字", squash_len(left));
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(AppError::Runtime(last))
}

fn compose(client: &mut Client, prompt: &str) -> Result<(), AppError> {
    let mut last = String::from("输入框还没就绪");
    for attempt in 0..COMPOSE_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(400));
        }
        match clear_composer(client) {
            Ok(()) => {}
            Err(e) => {
                last = format!("清空输入框失败: {e}");
                continue;
            }
        }
        // 必须走 CDP 的 Input.insertText：React 只认真实 input 事件
        client.insert_text(prompt)?;
        // 插完立刻读，React 往往还没把发送按钮从 disabled 里放出来，白读一次
        std::thread::sleep(COMPOSE_SETTLE);
        let state = eval_json(client, STATE_JS)?;
        let text = state.get("text").and_then(Value::as_str).unwrap_or("");
        let enabled = state
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        // 不能拿原文逐字比：ProseMirror 会把空行变成独立的段，innerText 读回来时空行
        // 与原文未必一字不差（多段落的长提示尤其如此）。折叠空白后再比，既容忍这种
        // 排版差异，又能挡住 insertText 漏字（漏字会让折叠后的长度对不上）。
        if same_text(text, prompt) && enabled {
            return Ok(());
        }
        last = format!(
            "输入框里是 {:?}（{} 字，期望 {} 字），发送按钮{}",
            crate::util::clip(text, 40),
            squash_len(text),
            squash_len(prompt),
            if enabled { "可点" } else { "不可点" }
        );
    }
    Err(AppError::Runtime(format!(
        "连试 {COMPOSE_ATTEMPTS} 次都没能把问题写进网页输入框（{last}）"
    )))
}

fn click_send(client: &mut Client) -> Result<(), AppError> {
    let mut last = String::from("没有发送按钮");
    for _ in 0..20 {
        let r = eval_json(client, SEND_JS)?;
        if r.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            return Ok(());
        }
        last = r
            .get("why")
            .and_then(Value::as_str)
            .unwrap_or("未知原因")
            .to_string();
        std::thread::sleep(Duration::from_millis(300));
    }
    Err(AppError::Runtime(format!("点不到发送按钮（{last}）")))
}

fn list_query() -> Vec<(String, String)> {
    vec![
        ("offset".to_string(), "0".to_string()),
        ("limit".to_string(), LIST_LIMIT.to_string()),
        ("order".to_string(), "updated".to_string()),
    ]
}

/// 抓一次最近会话的 id 快照。读不到就重试（这台机器上偶发 unexpected end of file）；
/// 全失败必须在**发消息之前**报错 —— 没有快照就分不清哪条是新会话，而那时还没花掉额度。
fn recent_ids(api: &Api) -> Result<HashSet<String>, AppError> {
    let mut last = String::new();
    for attempt in 0..SNAPSHOT_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(1000));
        }
        match api.get_json("/conversations", &list_query()) {
            Ok(v) => {
                let ids = v
                    .get("items")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c.get("id").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect();
                return Ok(ids);
            }
            Err(e) => last = first_line(&format!("{e}")).to_string(),
        }
    }
    Err(AppError::Runtime(format!(
        "发消息前读不到最近的会话列表，分不清哪条会话是这一次的（{last}）"
    )))
}

/// 活动分支上最后一条用户消息的正文
fn last_user_text(conversation: &Value) -> Option<String> {
    let mut found = None;
    for node in show::active_branch(conversation) {
        let msg = match node.get("message") {
            Some(m) if m.is_object() => m,
            _ => continue,
        };
        let role = msg
            .pointer("/author/role")
            .and_then(Value::as_str)
            .unwrap_or("");
        if role != "user" {
            continue;
        }
        found = show::visible_text(msg, false).map(|(_, t)| t);
    }
    found
}

/// 等这次提问落成一条 REST 能读的会话，返回它的 id。
///
/// 页面 URL 里的 local-chatgpt:<uuid> 认不出来 —— 它只证明「消息确实发出去了」；
/// 真正能用 REST 读的 id 只能从列表里新冒出来的那条拿，并核对最后一条用户消息。
fn wait_for_new_conversation(
    client: &mut Client,
    api: &Api,
    before: &HashSet<String>,
    prompt: &str,
    ctx: &Context,
) -> Result<String, AppError> {
    let deadline = Instant::now() + CONVERSATION_TIMEOUT;
    let mut rounds = 0u64;
    let mut page = String::from("/");
    let mut last_err = String::new();
    while Instant::now() < deadline {
        rounds += 1;
        // 顺带记下页面落点：/c/local-chatgpt%3A… 是「消息已经发出去了」的正向信号
        if let Ok(v) = client.eval_js("location.pathname") {
            if let Some(p) = v.as_str() {
                page = p.to_string();
            }
            // 个别情况下页面会直接落到 REST 认识的 id 上，那就省掉一轮等待
            if let Some(id) = conversation_id_from_path(&page) {
                return Ok(id);
            }
        }
        match api.get_json("/conversations", &list_query()) {
            Ok(v) => {
                let items = v
                    .get("items")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for item in items {
                    let id = match item.get("id").and_then(Value::as_str) {
                        Some(id) if !id.is_empty() && !before.contains(id) => id.to_string(),
                        _ => continue,
                    };
                    // 同时冒出别的会话（另一台设备、另一个标签页）时别认错
                    match api.get_json(&format!("/conversation/{id}"), &[]) {
                        Ok(conv) => {
                            let said = last_user_text(&conv).unwrap_or_default();
                            if said.trim() == prompt.trim() {
                                return Ok(id);
                            }
                        }
                        Err(e) => last_err = first_line(&format!("{e}")).to_string(),
                    }
                }
            }
            Err(e) => last_err = first_line(&format!("{e}")).to_string(),
        }
        ctx.tick(rounds, None, format!("等会话落库（页面停在 {page}）"));
        std::thread::sleep(LIST_INTERVAL);
    }
    let extra = if last_err.is_empty() {
        String::new()
    } else {
        format!("；最后一次错误: {last_err}")
    };
    Err(AppError::Runtime(format!(
        "点完发送 {} 秒都没在会话列表里看到这次提问（页面停在 {page}）—— 可能被风控拦了，也可能只是还没落库{extra}",
        CONVERSATION_TIMEOUT.as_secs()
    )))
}

struct Assist {
    text: String,
    done: bool,
}

/// 活动分支上最后一条 assistant 消息（工具调用节点由 `visible_text` 过滤掉）
fn last_assistant(conversation: &Value) -> Option<Assist> {
    let mut found = None;
    for node in show::active_branch(conversation) {
        let msg = match node.get("message") {
            Some(m) if m.is_object() => m,
            _ => continue,
        };
        let role = msg
            .pointer("/author/role")
            .and_then(Value::as_str)
            .unwrap_or("");
        if role != "assistant" {
            continue;
        }
        let status = msg.get("status").and_then(Value::as_str).unwrap_or("");
        let end_turn = msg.get("end_turn").and_then(Value::as_bool) == Some(true);
        let text = show::visible_text(msg, false)
            .map(|(_, t)| t)
            .unwrap_or_default();
        found = Some(Assist {
            text,
            done: end_turn || status == "finished_successfully",
        });
    }
    found
}

/// 轮询 REST 直到回答写完。返回 (正文, 是否确认写完, 会话用的模型, 轮询次数)
fn poll_reply(
    api: &Api,
    conversation_id: &str,
    ctx: &Context,
    on: OnText<'_>,
) -> Result<(String, bool, Option<String>, u64), AppError> {
    let deadline = Instant::now() + POLL_TIMEOUT;
    let mut best = String::new();
    let mut rounds = 0u64;
    let mut last_err = String::new();
    while Instant::now() < deadline {
        rounds += 1;
        match api.get_json(&format!("/conversation/{conversation_id}"), &[]) {
            Ok(conv) => {
                let model = conv
                    .get("default_model_slug")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if let Some(a) = last_assistant(&conv) {
                    if !a.text.trim().is_empty() {
                        best = a.text;
                        on(&best, a.done);
                        if a.done {
                            return Ok((best, true, model, rounds));
                        }
                    }
                    ctx.tick(
                        rounds,
                        None,
                        format!(
                            "{} 字{}",
                            best.chars().count(),
                            if a.done {
                                "（已收尾）"
                            } else {
                                "（生成中）"
                            }
                        ),
                    );
                }
            }
            // 会话还没落库时后端回 404：这是正常的，继续等
            Err(e) => last_err = first_line(&format!("{e}")).to_string(),
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    if best.trim().is_empty() {
        let extra = if last_err.is_empty() {
            String::new()
        } else {
            format!("；最后一次错误: {last_err}")
        };
        Err(AppError::Runtime(format!(
            "等了 {} 秒会话 {conversation_id} 里还是空的{extra}",
            POLL_TIMEOUT.as_secs()
        )))
    } else {
        // 有正文但没等到收尾：照样交出去，由调用方标 complete=false
        Ok((best, false, None, rounds))
    }
}

fn ask_at(cdp: &str, prompt: &str, ctx: &Context, on: OnText<'_>) -> Result<Value, Attempt> {
    let started = Instant::now();
    browser::probe(cdp).map_err(|error| Attempt { error, sent: false })?;
    // 自己开一个标签页，不碰用户正在看的那些；用完（无论成败）都关掉
    let (target_id, ws_url) =
        browser::create_target(cdp).map_err(|error| Attempt { error, sent: false })?;
    let outcome = run_in_tab(&ws_url, prompt, ctx, on);
    browser::close_target(cdp, &target_id);
    match outcome {
        Ok(done) => {
            ctx.log(
                LogLevel::Info,
                format!(
                    "chat 额度 · 会话里的模型是 {}",
                    done.model.as_deref().unwrap_or("(未知)")
                ),
            );
            Ok(json!({
                "quota": "chat",
                "model": done.model,
                "conversation_id": done.conversation_id,
                "text": done.text,
                "complete": done.complete,
                "cdp": cdp,
                "polls": done.polls,
                "elapsed_ms": started.elapsed().as_millis() as u64,
            }))
        }
        Err((sent, error)) => Err(Attempt { error, sent }),
    }
}

/// 一条走完的 chat 提问
struct Done {
    conversation_id: String,
    text: String,
    complete: bool,
    model: Option<String>,
    polls: u64,
}

/// 在一个标签页里走完整条链路。返回 (正文, 是否确认写完, 模型, 轮询次数)
fn run_in_tab(
    ws_url: &str,
    prompt: &str,
    ctx: &Context,
    on: OnText<'_>,
) -> Result<Done, (bool, AppError)> {
    let mut client = Client::connect(ws_url).map_err(|e| (false, e))?;
    wait_for_composer(&mut client).map_err(|e| (false, e))?;
    // 凭证与「发之前有哪些会话」都要在**发消息之前**备好：这时候失败还没花掉额度。
    let creds = refresh::ensure_fresh(auth::load().map_err(|e| (false, e))?);
    let api = Api::new(creds);
    let before = recent_ids(&api).map_err(|e| (false, e))?;
    ctx.log(LogLevel::Info, "网页已就绪，把问题写进输入框".to_string());
    compose(&mut client, prompt).map_err(|e| (false, e))?;
    ctx.log(LogLevel::Info, "点发送".to_string());
    click_send(&mut client).map_err(|e| (false, e))?;
    // 从这里起消息可能已经发出去了：后面任何失败都不能换端点重试
    let conversation_id = wait_for_new_conversation(&mut client, &api, &before, prompt, ctx)
        .map_err(|e| (true, e))?;
    drop(client);
    ctx.log(
        LogLevel::Info,
        format!("落成会话 {conversation_id}，等它写完"),
    );
    let (text, complete, model, polls) =
        poll_reply(&api, &conversation_id, ctx, on).map_err(|e| (true, e))?;
    Ok(Done {
        conversation_id,
        text,
        complete,
        model,
        polls,
    })
}

/// 用 chat 额度发一条消息（默认路径）。返回结果 JSON。
pub fn ask(prompt: &str, cdp: &str, ctx: &Context) -> Result<Value, AppError> {
    ask_streaming(prompt, cdp, ctx, &mut |_, _| {})
}

/// 同上，但每次轮询读到新的全文就回调一次 —— `serve` 靠它把网页那条路翻成 SSE。
///
/// 注意回调给的是**累计全文**：网页只有一个「当前答案」，没有 token 级增量。
pub fn ask_streaming(
    prompt: &str,
    cdp: &str,
    ctx: &Context,
    on: OnText<'_>,
) -> Result<Value, AppError> {
    let mut tried: Vec<String> = Vec::new();
    for endpoint in browser::candidate_endpoints(cdp) {
        match ask_at(&endpoint, prompt, ctx, &mut *on) {
            Ok(v) => return Ok(v),
            Err(attempt) => {
                if attempt.sent {
                    // 消息已经发出去了，换浏览器再发一遍会让用户看到两条一样的提问
                    return Err(attempt.error);
                }
                tried.push(format!(
                    "  - {endpoint}: {}",
                    first_line(&format!("{}", attempt.error))
                ));
            }
        }
    }
    let mut msg = String::from(
        "chat 额度这条路要驱动一个已经登录 chatgpt.com 的 chromium：把问题打进网页、点发送。\n\
  1. 起 ego-lite（或其他带调试端口的 chromium），并在里面登录 chatgpt.com\n\
  2. 或用 --cdp 指定端点（http://127.0.0.1:9229）\n\
  3. 只想用 codex 额度的话，显式加 --allow-codex",
    );
    if !tried.is_empty() {
        msg.push_str("\n试过的端点：\n");
        msg.push_str(&tried.join("\n"));
    }
    Err(AppError::InvalidInput(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 多段落提示过一遍 ProseMirror 再读回来，空行数量不一定守恒（实测长提示会多出
    /// 几十个空行），所以比对必须折叠空白；但漏字仍然要能挡住。
    #[test]
    fn the_composer_check_tolerates_whitespace_but_not_missing_text() {
        let want = "第一段\n\n第二段 有 空格\n第三段";
        // 页面把它渲染成每段一个 <p>，读回来空行更多、行尾多一个空格
        let got = "第一段\n\n\n\n第二段 有 空格 \n\n第三段";
        assert!(same_text(got, want), "只差空白就不该拦");
        // 少了半个字必须拦（否则会把残句发出去）
        assert!(!same_text("第一段\n\n第二段 有 空\n第三段", want));
        assert!(!same_text("", want));
    }

    #[test]
    fn the_whitespace_measure_matches_between_shapes() {
        assert_eq!(squash_len("a\n\nb"), 3);
        assert_eq!(squash_len("a   b"), 3);
        assert_eq!(squash_len("   "), 0);
    }

    #[test]
    fn a_conversation_id_is_read_out_of_the_path() {
        assert_eq!(
            conversation_id_from_path("/c/6b06de0a-249c-83ea-ac09-8ee8c2727f4f").unwrap(),
            "6b06de0a-249c-83ea-ac09-8ee8c2727f4f"
        );
        assert_eq!(
            conversation_id_from_path("/c/6b06de0a-249c-83ea-ac09-8ee8c2727f4f/tab").unwrap(),
            "6b06de0a-249c-83ea-ac09-8ee8c2727f4f"
        );
        assert!(conversation_id_from_path("/").is_none());
        assert!(conversation_id_from_path("/c/abc").is_none());
        // 网页自己停在的会话地址带 local-chatgpt 前缀，那个 id REST 不认（会 400/404），
        // 所以这里必须认不出来，交给列表轮询去拿真 id
        assert!(conversation_id_from_path(
            "/c/local-chatgpt%3A2370930c-7b70-4fe0-abfe-54738605ff57"
        )
        .is_none());
    }

    /// 认出「这次问的是哪条会话」靠的就是这条用户消息原文
    #[test]
    fn the_last_user_turn_carries_the_prompt() {
        let conv = json!({
            "current_node": "u2",
            "mapping": {
                "u1": {"id": "u1", "parent": null, "message": {
                    "author": {"role": "user"}, "recipient": "all",
                    "content": {"content_type": "text", "parts": ["上一轮"]}}},
                "a1": {"id": "a1", "parent": "u1", "message": {
                    "author": {"role": "assistant"}, "recipient": "all",
                    "status": "finished_successfully", "end_turn": true,
                    "content": {"content_type": "text", "parts": ["上一轮的回答"]}}},
                "u2": {"id": "u2", "parent": "a1", "message": {
                    "author": {"role": "user"}, "recipient": "all",
                    "content": {"content_type": "text", "parts": ["这一轮"]}}},
            }
        });
        assert_eq!(last_user_text(&conv).as_deref(), Some("这一轮"));
        assert!(last_user_text(&json!({})).is_none());
    }

    /// 只有「收尾了」才算答完，生成中的节点不能当成答案
    #[test]
    fn the_last_assistant_turn_reports_whether_it_finished() {
        let conv = json!({
            "current_node": "a2",
            "mapping": {
                "u1": {"id": "u1", "parent": null, "message": {
                    "author": {"role": "user"}, "recipient": "all",
                    "content": {"content_type": "text", "parts": ["问"]}}},
                "t1": {"id": "t1", "parent": "u1", "message": {
                    "author": {"role": "assistant"}, "recipient": "web.run",
                    "content": {"content_type": "text", "parts": ["{\"q\":1}"]}}},
                "a2": {"id": "a2", "parent": "t1", "message": {
                    "author": {"role": "assistant"}, "recipient": "all",
                    "status": "finished_successfully", "end_turn": true,
                    "content": {"content_type": "text", "parts": ["答"]}}},
            }
        });
        let a = last_assistant(&conv).expect("应当有一条 assistant 消息");
        assert_eq!(a.text, "答");
        assert!(a.done);

        let conv = json!({
            "current_node": "a1",
            "mapping": {"a1": {"id": "a1", "parent": null, "message": {
                "author": {"role": "assistant"}, "recipient": "all",
                "status": "in_progress", "end_turn": null,
                "content": {"content_type": "text", "parts": ["答到一半"]}}}}
        });
        let a = last_assistant(&conv).expect("生成中也算有消息");
        assert_eq!(a.text, "答到一半");
        assert!(!a.done, "in_progress 不能被当成答完");
    }
}
