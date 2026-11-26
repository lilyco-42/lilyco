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
/// 等回答写完的最长等待（生图或深度思考可能超过 3 分钟，给足 6 分钟）
const POLL_TIMEOUT: Duration = Duration::from_secs(360);
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

/// chat 路径的可选控制项：续接会话、指定网页模型、指定推理强度（Instant / Medium / High）
#[derive(Clone, Copy, Debug, Default)]
pub struct ChatOptions<'a> {
    pub conversation_id: Option<&'a str>,
    pub known_branch_len: Option<usize>,
    /// 网页模型名或 slug，如 `gpt-6-2` / `GPT-6.2` / `gpt-6` / `gpt-5-6` / `gpt-6-2-instant` / `gpt-6-2-thinking`
    pub model: Option<&'a str>,
    /// 推理强度：`instant` / `low` (0) · `medium` (1) · `high` / `xhigh` / `max` (2)
    pub effort: Option<&'a str>,
}

/// 把用户传的推理强度名映射到 ChatGPT 网页滑块的 0 / 1 / 2 档位。
///
/// 实测网页滑块 `[role="slider"]` 的三档：`0 = Instant`，`1 = Medium`，`2 = High`。
pub fn parse_chat_effort(raw: &str) -> Result<Option<u8>, AppError> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Ok(None);
    }
    match s.as_str() {
        "instant" | "low" | "none" | "off" | "minimal" | "0" => Ok(Some(0)),
        "medium" | "standard" | "normal" | "default" | "1" => Ok(Some(1)),
        "high" | "xhigh" | "max" | "ultra" | "extended" | "thinking" | "2" => Ok(Some(2)),
        other => Err(AppError::InvalidArg(format!(
            "不认识的 chat 推理强度 {other:?}。可选: instant(low) / medium / high(xhigh/max)"
        ))),
    }
}

/// 拆解网页模型名：像 `gpt-6-2-instant` / `gpt-6-2-thinking` 这类 slug 同时隐含了模型和强度。
pub fn split_chat_model_hint(raw: &str) -> (Option<String>, Option<u8>) {
    let s = raw.trim();
    if s.is_empty() {
        return (None, None);
    }
    let lower = s.to_ascii_lowercase();
    if let Some(base) = lower.strip_suffix("-instant") {
        return (Some(base.to_string()), Some(0));
    }
    if let Some(base) = lower.strip_suffix("-thinking") {
        return (Some(base.to_string()), Some(2));
    }
    (Some(lower), None)
}

/// 归一化模型标签，把 `gpt-6-2` / `GPT-6.2` / `gpt-5-6` / `GPT-5.6 Sol` 统一到同一套可比形式
fn normalize_model_key(s: &str) -> String {
    let first = s
        .lines()
        .next()
        .unwrap_or(s)
        .split('|')
        .next()
        .unwrap_or(s)
        .trim()
        .to_ascii_lowercase();
    let base = first.split_whitespace().next().unwrap_or(&first);
    base.replace('-', ".")
}

/// 查找网页顶部/输入框旁的「选择 ChatGPT 模型」按钮中心坐标
const MODEL_BTN_JS: &str = r#"(() => {
  const btn = Array.from(document.querySelectorAll('button')).find(b => {
    const a = (b.getAttribute('aria-label') || '').toLowerCase();
    return a.includes('chatgpt 模型') || a.includes('chatgpt model') || a.includes('choose chatgpt');
  });
  if (!btn) return JSON.stringify({ ok: false });
  const r = btn.getBoundingClientRect();
  return JSON.stringify({
    ok: true,
    x: r.x + r.width / 2,
    y: r.y + r.height / 2,
    text: (btn.innerText || '').trim(),
  });
})()"#;

/// 读出已打开的模型/强度菜单里的所有 radio 模型项与滑块状态
const MODEL_MENU_JS: &str = r#"(() => {
  const pop = document.querySelector('[role="menu"][data-state="open"], [data-radix-menu-content]');
  if (!pop) return JSON.stringify({ open: false });
  const radios = Array.from(pop.querySelectorAll('[role="menuitemradio"]')).map(el => {
    const r = el.getBoundingClientRect();
    return {
      text: (el.innerText || '').trim(),
      checked: el.getAttribute('aria-checked') === 'true',
      x: r.x + r.width / 2,
      y: r.y + r.height / 2,
    };
  });
  const slider = pop.querySelector('[role="slider"]');
  const label = pop.querySelector('[role="menuitem"]')?.innerText?.trim() || '';
  return JSON.stringify({
    open: true,
    radios,
    slider: slider ? {
      val: Number(slider.getAttribute('aria-valuenow') ?? 0),
      min: Number(slider.getAttribute('aria-valuemin') ?? 0),
      max: Number(slider.getAttribute('aria-valuemax') ?? 2),
      label,
    } : null,
  });
})()"#;

/// 在已就绪的 ChatGPT 网页里切换模型和/或推理强度（均为空时跳过，零开销）。
fn apply_model_and_effort(
    client: &mut Client,
    model_raw: Option<&str>,
    effort_raw: Option<&str>,
    ctx: &Context,
) -> Result<(), AppError> {
    let (model_hint, implied_effort) = match model_raw {
        Some(m) => split_chat_model_hint(m),
        None => (None, None),
    };
    let explicit_effort = match effort_raw {
        Some(e) => parse_chat_effort(e)?,
        None => None,
    };
    let target_effort = explicit_effort.or(implied_effort);
    if model_hint.is_none() && target_effort.is_none() {
        return Ok(());
    }

    let open_menu = |client: &mut Client| -> Result<Value, AppError> {
        // 刚进页面时 React 事件监听可能还没挂稳，最多试 3 次点开菜单
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(600));
            }
            let btn = eval_json(client, MODEL_BTN_JS)?;
            if !btn.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let x = btn["x"].as_f64().unwrap_or(0.0);
            let y = btn["y"].as_f64().unwrap_or(0.0);
            if x <= 0.0 || y <= 0.0 {
                continue;
            }
            client.click_point(x, y)?;
            for _ in 0..6 {
                std::thread::sleep(Duration::from_millis(200));
                let m = eval_json(client, MODEL_MENU_JS)?;
                if m.get("open").and_then(Value::as_bool).unwrap_or(false) {
                    return Ok(m);
                }
            }
        }
        Err(AppError::Runtime("点了模型按钮但下拉菜单没弹出来".to_string()))
    };

    let mut menu = open_menu(client)?;

    if let Some(want_model) = model_hint.as_deref() {
        let want_key = normalize_model_key(want_model);
        let radios = menu
            .get("radios")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let matched = radios.iter().find(|r| {
            let txt = r.get("text").and_then(Value::as_str).unwrap_or("");
            normalize_model_key(txt) == want_key
        });
        if let Some(item) = matched {
            let already = item
                .get("checked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let label = item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or(want_model)
                .lines()
                .next()
                .unwrap_or(want_model);
            if !already {
                let x = item["x"].as_f64().unwrap_or(0.0);
                let y = item["y"].as_f64().unwrap_or(0.0);
                client.click_point(x, y)?;
                std::thread::sleep(Duration::from_millis(350));
                ctx.log(LogLevel::Info, format!("已切换网页模型 → {label}"));
                // 点完模型后 Radix 菜单可能自动收起，后面还要调强度的话重新打开
                if target_effort.is_some() {
                    let check = eval_json(client, MODEL_MENU_JS)?;
                    if !check.get("open").and_then(Value::as_bool).unwrap_or(false) {
                        menu = open_menu(client)?;
                    } else {
                        menu = check;
                    }
                }
            }
        } else {
            let avail: Vec<String> = radios
                .iter()
                .filter_map(|r| {
                    r.get("text")
                        .and_then(Value::as_str)
                        .and_then(|t| t.lines().next())
                        .map(str::to_string)
                })
                .collect();
            let _ = client.press_key("Escape", "Escape", 27);
            return Err(AppError::InvalidArg(format!(
                "网页模型菜单里找不到 {want_model:?}（当前可选: {}）",
                avail.join(" / ")
            )));
        }
    }

    if let Some(target) = target_effort {
        if let Some(slider) = menu.get("slider").filter(|v| v.is_object()) {
            let max = slider.get("max").and_then(Value::as_u64).unwrap_or(2) as u8;
            let clamped = target.min(max);
            let _ = client.eval_js(
                "document.querySelector('[role=\"menu\"][data-state=\"open\"] [role=\"slider\"], [data-radix-menu-content] [role=\"slider\"]')?.focus()",
            );
            std::thread::sleep(Duration::from_millis(120));
            client.press_key("Home", "Home", 36)?;
            std::thread::sleep(Duration::from_millis(150));
            for _ in 0..clamped {
                client.press_key("ArrowRight", "ArrowRight", 39)?;
                std::thread::sleep(Duration::from_millis(150));
            }
            let after = eval_json(client, MODEL_MENU_JS).unwrap_or(Value::Null);
            let label = after
                .pointer("/slider/label")
                .and_then(Value::as_str)
                .unwrap_or("");
            ctx.log(
                LogLevel::Info,
                format!("已切换推理强度 → 档位 {clamped} ({label})"),
            );
        } else {
            ctx.log(
                LogLevel::Warn,
                "当前选中的网页模型没有推理强度滑块，已跳过强度切换".to_string(),
            );
        }
    }

    let _ = client.press_key("Escape", "Escape", 27);
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}

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

/// 把连续空白折成一个空格，并剥掉 ProseMirror 偶发插入的零宽字符
/// （多段落提示进 ProseMirror 再读回来，空行数量不一定守恒，长文本还可能多出 1~2 个零宽/换行空格）
fn squash(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|&c| c != '\u{200b}' && c != '\u{feff}')
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn squash_len(s: &str) -> usize {
    squash(s).chars().count()
}

/// 输入框里的文字是否就是这段提示（按折叠空白后的内容比）。
///
/// 短提示要求逐字一致；超过 200 字的长提示（如 dsh 注入的 100KB 上下文）经 ProseMirror
/// 归一化后偶发多/少 1~2 个空白（实测 100516 vs 100515），只要首尾 80 字符完全一致且
/// 长度差不超过 1% 就认作同一段文本，既能放行这种 1 字排版差，又能挡住漏字或截断。
fn same_text(got: &str, want: &str) -> bool {
    if want.trim().is_empty() {
        return false;
    }
    let g = squash(got);
    let w = squash(want);
    if g == w {
        return true;
    }
    let gc: Vec<char> = g.chars().collect();
    let wc: Vec<char> = w.chars().collect();
    if wc.len() < 200 || gc.len() < 200 {
        return false;
    }
    let edge = 80;
    let diff = gc.len().abs_diff(wc.len());
    diff * 100 <= wc.len()
        && gc[..edge] == wc[..edge]
        && gc[gc.len() - edge..] == wc[wc.len() - edge..]
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

/// 长提示阈值：超过这个字节数时，CDP 的 `Input.insertText` 会让 ProseMirror 逐行拆 `<p>`
/// 并同步做 layout，40KB+ 的 dsh 系统提示词会卡死渲染线程超过 25s 超时。
/// 实测直接设 `el.textContent` + 派发 `InputEvent('input')` 写 49KB 只需 ~0.5s，且发送按钮立刻可点。
const FAST_INSERT_THRESHOLD: usize = 2000;

fn fast_insert_js(prompt: &str) -> String {
    let encoded = serde_json::to_string(prompt).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(() => {{
  const el = document.querySelector('div[contenteditable="true"]');
  if (!el) return JSON.stringify({{ ok: false }});
  el.focus();
  el.textContent = {encoded};
  el.dispatchEvent(new InputEvent('input', {{ bubbles: true, inputType: 'insertText' }}));
  return JSON.stringify({{ ok: true }});
}})()"#
    )
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
        if prompt.len() > FAST_INSERT_THRESHOLD {
            let r = eval_json(client, &fast_insert_js(prompt))?;
            if !r.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                last = String::from("找不到 contenteditable 输入框");
                continue;
            }
        } else {
            // 短提示走 CDP 的 Input.insertText：最贴近真实输入事件
            client.insert_text(prompt)?;
        }
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
                            if same_text(&said, prompt) {
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
    /// 这一轮生成的图（生图回复只有图、没有字）
    images: Vec<show::Image>,
}

impl Assist {
    /// 「有内容」= 有字 **或** 有图。生图那一轮最后一个 assistant 节点的 parts 是空串，
    /// 只看字就会把一次成功的生图判成空会话。
    fn has_content(&self) -> bool {
        !self.text.trim().is_empty() || !self.images.is_empty()
    }
}

/// 活动分支上最后一条 assistant 消息 + 整条分支上出现过的图。
///
/// 文本只看最后一条 assistant 节点（那才是屏幕上那句话）；图要在**整条分支**上收集 ——
/// 生图时图挂在 `role: "tool"` 的 `multimodal_text` 节点上，而那些节点在收尾的
/// assistant 节点之前，且一次生成会出现两个指向同一 `asset_pointer` 的节点（`show::images` 去重）。
#[cfg(test)]
fn last_assistant(conversation: &Value) -> Option<Assist> {
    last_assistant_after(conversation, 0)
}

/// 同 `last_assistant`，但忽略活动分支前 `min_branch_len` 个旧节点 ——
/// 续接已有会话发新消息时，用发消息前的分支长度做下界，确保绝不会把上一轮的旧回答当成新回答。
fn last_assistant_after(conversation: &Value, min_branch_len: usize) -> Option<Assist> {
    let mut found: Option<Assist> = None;
    let mut images: Vec<show::Image> = Vec::new();
    let mut finished_seen = false;
    let chain = show::active_branch(conversation);
    if min_branch_len > 0 && chain.len() <= min_branch_len {
        return None;
    }
    let slice = &chain[min_branch_len..];
    // 图只算**这一轮**的：从最后一条 user 消息往后数，免得把上一轮的图算进来
    let from = match slice.iter().rposition(|n| {
        n.pointer("/message/author/role")
            .and_then(Value::as_str)
            .unwrap_or("")
            == "user"
    }) {
        Some(idx) => idx,
        None if min_branch_len > 0 => return None,
        None => 0,
    };
    for node in slice.iter().skip(from) {
        let msg = match node.get("message") {
            Some(m) if m.is_object() => m,
            _ => continue,
        };
        for img in show::images(msg) {
            if !images.iter().any(|i| i.file_id == img.file_id) {
                images.push(img);
            }
        }
        let role = msg
            .pointer("/author/role")
            .and_then(Value::as_str)
            .unwrap_or("");
        if role != "assistant" {
            continue;
        }
        let status = msg.get("status").and_then(Value::as_str).unwrap_or("");
        let end_turn = msg.get("end_turn").and_then(Value::as_bool) == Some(true);
        if status == "finished_successfully" {
            finished_seen = true;
        }
        let text = show::visible_text(msg, false)
            .map(|(_, t)| t)
            .unwrap_or_default();
        let empty_text = text.trim().is_empty();
        found = Some(Assist {
            text,
            done: end_turn || status == "finished_successfully",
            images: Vec::new(),
        });
        // 生图那一轮的收尾节点永远停在 in_progress（实测：图挂在前面的 tool 节点上，
        // 后面再挂一条空文字的 assistant 节点，status 一直是 in_progress、end_turn 从不置
        // true）。只要这一轮已经有图、末尾这条是空文字的 in_progress，就算画完了 ——
        // 否则会一路空等到 POLL_TIMEOUT，把成功生图报成 complete=false。
        if !images.is_empty() && finished_seen && empty_text && status == "in_progress" {
            found = Some(Assist {
                text: String::new(),
                done: true,
                images: Vec::new(),
            });
        }
    }
    found.map(|mut a| {
        a.images = images;
        a
    })
}

/// 轮询到的一次结果
struct Polled {
    text: String,
    complete: bool,
    model: Option<String>,
    polls: u64,
    images: Vec<show::Image>,
    /// 本轮结束后活动分支的节点数，续聊下一轮直接复用，不必再额外 GET 一次撞 429
    branch_len: usize,
}

/// 轮询 REST 直到回答写完。
///
/// 判据是「**有字或图** + 收尾标志」：生图那一轮只有图没有字，最后一个 assistant 节点的
/// parts 是空串，所以不能只看文本（v0.3 之前就在这里把成功生图误报成「会话里还是空的」）。
fn poll_reply(
    api: &Api,
    conversation_id: &str,
    min_branch_len: usize,
    ctx: &Context,
    on: OnText<'_>,
) -> Result<Polled, AppError> {
    let deadline = Instant::now() + POLL_TIMEOUT;
    let mut best = String::new();
    let mut images: Vec<show::Image> = Vec::new();
    let mut rounds = 0u64;
    let mut last_err = String::new();
    let mut last_branch_len = min_branch_len;
    // 刚点完发送时先缓 2.5 秒再首次查询，减少无效请求与 429
    if min_branch_len > 0 {
        std::thread::sleep(Duration::from_millis(2500));
    }
    while Instant::now() < deadline {
        rounds += 1;
        // 429 是限流不是失败：CLI 自己会退避重试几次（见 Api::get_json_retry）
        match api.get_json_retry(&format!("/conversation/{conversation_id}"), &[], 5) {
            Ok(conv) => {
                last_branch_len = show::active_branch(&conv).len();
                let model = conv
                    .get("default_model_slug")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if let Some(a) = last_assistant_after(&conv, min_branch_len) {
                    if a.has_content() {
                        best = a.text;
                        images = a.images;
                        // 有图时把「图生成了」当作正文交给回调，serve 那边才不会以为没输出
                        on(&best, a.done);
                        if a.done {
                            return Ok(Polled {
                                text: best,
                                complete: true,
                                model,
                                polls: rounds,
                                images,
                                branch_len: last_branch_len,
                            });
                        }
                    }
                    ctx.tick(
                        rounds,
                        None,
                        format!(
                            "{} 字{} 图{}",
                            best.chars().count(),
                            if images.is_empty() { "" } else { "，" },
                            if images.is_empty() {
                                String::new()
                            } else {
                                images.len().to_string()
                            },
                        ),
                    );
                }
            }
            // 会话还没落库时后端回 404：这是正常的，继续等
            Err(e) => last_err = first_line(&format!("{e}")).to_string(),
        }
        std::thread::sleep(poll_interval(rounds));
    }
    if best.trim().is_empty() && images.is_empty() {
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
        Ok(Polled {
            text: best,
            complete: false,
            model: None,
            polls: rounds,
            images,
            branch_len: last_branch_len,
        })
    }
}

/// 轮询间隔：越等越松。
///
/// 一次生图要盯着一两分钟，固定 1.2 秒一个 GET 会把 `/conversation/{id}` 打到限流
/// （实测连打 123 次之后整个端点报 429 约半分钟）。前几轮密一点好让文字回答尽快收工，
/// 之后就拉开间隔，整段时间里的请求数从 ~150 降到 ~60。
fn poll_interval(rounds: u64) -> Duration {
    match rounds {
        0..=8 => POLL_INTERVAL,
        9..=20 => Duration::from_millis(2000),
        _ => Duration::from_millis(3500),
    }
}
fn ask_at(
    cdp: &str,
    opts: ChatOptions<'_>,
    prompt: &str,
    ctx: &Context,
    on: OnText<'_>,
) -> Result<Value, Attempt> {
    let started = Instant::now();
    browser::probe(cdp).map_err(|error| Attempt { error, sent: false })?;
    // 自己开一个标签页，不碰用户正在看的那些
    let (target_id, ws_url) = match opts.conversation_id {
        Some(id) => browser::create_target_url(cdp, &format!("https://chatgpt.com/c/{id}")),
        None => browser::create_target(cdp),
    }
    .map_err(|error| Attempt { error, sent: false })?;
    let outcome = run_in_tab(&ws_url, opts, prompt, ctx, on);
    // 只有在「明确完成」或「消息根本没发出去」时才关临时标签页；
    // 如果消息已发出但超时/未完，保留标签页，避免把仍在进行的生图任务硬掐断。
    let should_close = match &outcome {
        Ok(done) => done.complete,
        Err((sent, _)) => !sent,
    };
    if should_close {
        browser::close_target(cdp, &target_id);
    }
    match outcome {
        Ok(done) => {
            ctx.log(
                LogLevel::Info,
                format!(
                    "chat 额度 · 会话里的模型是 {}",
                    done.model.as_deref().unwrap_or("(未知)")
                ),
            );
            if !done.images.is_empty() {
                ctx.log(
                    LogLevel::Info,
                    format!(
                        "生成了 {} 张图（file_id 见 --json 的 images）",
                        done.images.len()
                    ),
                );
            }
            Ok(json!({
                "quota": "chat",
                "model": done.model,
                "conversation_id": done.conversation_id,
                "text": done.text,
                "images": done.images.iter().map(show::Image::to_json).collect::<Vec<_>>(),
                "complete": done.complete,
                "branch_len": done.branch_len,
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
    images: Vec<show::Image>,
    branch_len: usize,
}

/// 在一个标签页里走完整条链路。返回 (正文, 是否确认写完, 模型, 轮询次数)
fn run_in_tab(
    ws_url: &str,
    opts: ChatOptions<'_>,
    prompt: &str,
    ctx: &Context,
    on: OnText<'_>,
) -> Result<Done, (bool, AppError)> {
    let mut client = Client::connect(ws_url).map_err(|e| (false, e))?;
    wait_for_composer(&mut client).map_err(|e| (false, e))?;
    // 走到这里说明这个浏览器已经登录了 chatgpt.com，后面的失败直接报出来，
    // 不要再回退去试别的未登录端口白等 45 秒。
    // 切换模型/推理强度也在发消息之前完成，失败不会花掉额度。
    apply_model_and_effort(&mut client, opts.model, opts.effort, ctx)
        .map_err(|e| (true, e))?;
    // 凭证与「发之前的状态快照」都要在**发消息之前**备好：这时候失败还没花掉额度。
    let creds = refresh::ensure_fresh(auth::load().map_err(|e| (true, e))?);
    let api = Api::new(creds);

    let (conversation_id, min_branch_len) = if let Some(cid) = opts.conversation_id {
        let before_len = match opts.known_branch_len {
            Some(n) if n > 0 => n,
            _ => {
                let before_conv = api
                    .get_json_retry(&format!("/conversation/{cid}"), &[], 5)
                    .map_err(|e| (true, e))?;
                show::active_branch(&before_conv).len()
            }
        };
        ctx.log(
            LogLevel::Info,
            format!("续接会话 {cid}（已有 {before_len} 节点），把增量消息写进输入框"),
        );
        compose(&mut client, prompt).map_err(|e| (true, e))?;
        ctx.log(LogLevel::Info, "点发送".to_string());
        click_send(&mut client).map_err(|e| (true, e))?;
        drop(client);
        (cid.to_string(), before_len)
    } else {
        let before = recent_ids(&api).map_err(|e| (true, e))?;
        ctx.log(LogLevel::Info, "网页已就绪，把问题写进输入框".to_string());
        compose(&mut client, prompt).map_err(|e| (true, e))?;
        ctx.log(LogLevel::Info, "点发送".to_string());
        click_send(&mut client).map_err(|e| (true, e))?;
        // 从这里起消息可能已经发出去了：后面任何失败都不能换端点重试
        let cid = wait_for_new_conversation(&mut client, &api, &before, prompt, ctx)
            .map_err(|e| (true, e))?;
        drop(client);
        ctx.log(LogLevel::Info, format!("落成会话 {cid}，等它写完"));
        (cid, 0)
    };

    let polled = poll_reply(&api, &conversation_id, min_branch_len, ctx, on)
        .map_err(|e| (true, e))?;
    Ok(Done {
        conversation_id,
        text: polled.text,
        complete: polled.complete,
        model: polled.model,
        polls: polled.polls,
        images: polled.images,
        branch_len: polled.branch_len,
    })
}

/// 用 chat 额度发一条消息（默认路径）。返回结果 JSON。
pub fn ask(prompt: &str, cdp: &str, ctx: &Context) -> Result<Value, AppError> {
    ask_with_options(prompt, cdp, ChatOptions::default(), ctx)
}

/// 同 `ask`，但可指定网页模型与推理强度。
pub fn ask_with_options(
    prompt: &str,
    cdp: &str,
    opts: ChatOptions<'_>,
    ctx: &Context,
) -> Result<Value, AppError> {
    ask_with_options_streaming(opts, prompt, cdp, ctx, &mut |_, _| {})
}

/// 完整可控的 chat 路径：支持续接会话、指定网页模型、指定推理强度与流式回调。
pub fn ask_with_options_streaming(
    opts: ChatOptions<'_>,
    prompt: &str,
    cdp: &str,
    ctx: &Context,
    on: OnText<'_>,
) -> Result<Value, AppError> {
    // 提前校验强度参数：拼错了不用开浏览器就报错
    if let Some(e) = opts.effort {
        parse_chat_effort(e)?;
    }
    let mut tried: Vec<String> = Vec::new();
    for endpoint in browser::candidate_endpoints(cdp) {
        match ask_at(&endpoint, opts, prompt, ctx, &mut *on) {
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
    /// 生图那一轮的收尾节点永远是 in_progress（实测的会话形状）：图挂在 role=tool 的
    /// multimodal_text 节点上，末尾再挂一条空文字的 assistant。这条链以前被判成「没答完」，
    /// 一路空等到超时。现在「有图 + 前面有 finished_successfully + 末尾空文字 in_progress」
    /// 就算画完了。
    #[test]
    fn an_image_round_counts_as_finished_even_though_the_tail_stays_in_progress() {
        let conv = json!({
            "current_node": "a3",
            "mapping": {
                "u1": {"id": "u1", "parent": null, "children": ["a1"], "message": {
                    "author": {"role": "user"}, "recipient": "all",
                    "content": {"content_type": "text", "parts": ["画一只猫"]}}},
                "a1": {"id": "a1", "parent": "u1", "children": ["t1"], "message": {
                    "author": {"role": "assistant"}, "recipient": "all",
                    "status": "finished_successfully", "end_turn": false,
                    "content": {"content_type": "code", "text": "{}"}}},
                "t1": {"id": "t1", "parent": "a1", "children": ["a2"], "message": {
                    "author": {"role": "tool"}, "recipient": "all",
                    "status": "finished_successfully",
                    "content": {"content_type": "multimodal_text", "parts": [{
                        "content_type": "image_asset_pointer",
                        "asset_pointer": "sediment://file_abc",
                        "width": 1024, "height": 1024}]}}},
                "a2": {"id": "a2", "parent": "t1", "children": ["a3"], "message": {
                    "author": {"role": "assistant"}, "recipient": "all",
                    "status": "finished_successfully", "end_turn": false,
                    "content": {"content_type": "reasoning_recap", "content": "想了想"}}},
                "a3": {"id": "a3", "parent": "a2", "children": [], "message": {
                    "author": {"role": "assistant"}, "recipient": "all",
                    "status": "in_progress", "end_turn": false,
                    "content": {"content_type": "text", "parts": [""]}}},
            }
        });
        let a = last_assistant(&conv).expect("应当认出这一轮");
        assert!(a.has_content(), "有图就算有内容");
        assert_eq!(a.images.len(), 1);
        assert_eq!(a.images[0].file_id, "file_abc");
        assert!(a.done, "图已经出来了就不该再等收尾标志");
    }

    /// 光有 in_progress、没有图的时候绝不能判完（文字还在流式写）
    #[test]
    fn an_in_progress_tail_without_images_is_not_done() {
        let conv = json!({
            "current_node": "a1",
            "mapping": {
                "u1": {"id": "u1", "parent": null, "children": ["a1"], "message": {
                    "author": {"role": "user"}, "recipient": "all",
                    "content": {"content_type": "text", "parts": ["问"]}}},
                "a1": {"id": "a1", "parent": "u1", "children": [], "message": {
                    "author": {"role": "assistant"}, "recipient": "all",
                    "status": "in_progress", "end_turn": false,
                    "content": {"content_type": "text", "parts": ["答到一半"]}}},
            }
        });
        let a = last_assistant(&conv).expect("生成中也算有消息");
        assert!(!a.done, "没图的 in_progress 不能当成答完");
    }

    /// 越等越松：前 9 轮密、之后拉开，整段时间的请求数才不会被后端判成刷接口
    #[test]
    fn the_poll_interval_relaxes_as_the_wait_drags_on() {
        assert_eq!(poll_interval(1), POLL_INTERVAL);
        assert_eq!(poll_interval(9), Duration::from_millis(2000));
        assert_eq!(poll_interval(30), Duration::from_millis(3500));
        assert!(poll_interval(30) > poll_interval(1));
    }

    /// 长提示走 fast_insert_js 时，引号、换行和中文都要按 JSON 字符串安全转义
    #[test]
    fn fast_insert_js_safely_embeds_special_characters() {
        let js = fast_insert_js("第一行\n\"引号\" \\ 反斜杠 '单引号'");
        assert!(js.contains("el.textContent = \"第一行\\n\\\"引号\\\" \\\\ 反斜杠 '单引号'\";"));
        assert!(js.contains("new InputEvent('input'"));
    }

    /// 长提示经 ProseMirror 归一化后多 1 个空格或零宽字符应放行，但截断或尾部不符必须拦住
    #[test]
    fn long_prompts_tolerate_minor_prosemirror_drift_but_not_truncation() {
        let head = "You are an AI agent powered by DeepSeek. ".repeat(3);
        let mid = "context block line with details. ".repeat(10);
        let tail = "User: hi there, please answer cleanly. ".repeat(3);
        let want = format!("{head}{mid}{tail}");
        // 中间多一个空格 + 零宽字符（实测 100516 vs 100515）
        let got_drift = format!("{head}{mid}x\u{200b}{tail}");
        assert!(same_text(&got_drift, &want));
        // 截断或尾部不同必须拦住
        let got_truncated = format!("{head}{mid}");
        assert!(!same_text(&got_truncated, &want));
        let got_wrong_tail = format!("{head}{mid}{tail}EXTRA");
        assert!(!same_text(&got_wrong_tail, &want));
    }

    #[test]
    fn chat_effort_and_model_hints_parse_cleanly() {
        assert_eq!(parse_chat_effort("").unwrap(), None);
        assert_eq!(parse_chat_effort("instant").unwrap(), Some(0));
        assert_eq!(parse_chat_effort("low").unwrap(), Some(0));
        assert_eq!(parse_chat_effort("medium").unwrap(), Some(1));
        assert_eq!(parse_chat_effort("high").unwrap(), Some(2));
        assert_eq!(parse_chat_effort("max").unwrap(), Some(2));
        assert!(parse_chat_effort("bogus").is_err());

        assert_eq!(
            split_chat_model_hint("gpt-6-2-instant"),
            (Some("gpt-6-2".into()), Some(0))
        );
        assert_eq!(
            split_chat_model_hint("gpt-6-2-thinking"),
            (Some("gpt-6-2".into()), Some(2))
        );
        assert_eq!(normalize_model_key("GPT-6.2"), normalize_model_key("gpt-6-2"));
        assert_eq!(
            normalize_model_key("GPT-5.6 Sol | 将于 12月23日 下线"),
            normalize_model_key("gpt-5-6")
        );
    }
}
