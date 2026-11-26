//! 可选的浏览器登录态抓取 —— 从一个**已经登录 chatgpt.com 的 chromium** 里取出 access token。
//!
//! 为什么要有这条路：`/api/auth/session` 给出的 accessToken 能覆盖全部只读命令
//! （whoami / usage / models / list / show-conversation / search），而它只存在于
//! 浏览器的 cookie 里；用户既然已经在 ego-lite（或任何 chromium）里登录过，
//! 我们就不该让他再登一次。
//!
//! **不是硬依赖**：这里不调用任何外部程序，也不链接 chromium / ego-lite 的库，
//! 只用 DevTools Protocol 的两个口子（HTTP 发现端口 + WebSocket 求值，tungstenite 纯 Rust）。
//! 没有浏览器在跑的时候，`lchatgpt login --from browser` 会明确说「连不上 CDP」，
//! 其余 7 条命令完全不受影响 —— 单一二进制，装完就能用。
//!
//! 端口的找法：`--cdp` → `$LCHATGPT_CDP` → ego-lite 的 `~/.config/ego-lite/config.json`
//! 里的 `cdpPort` → 常见端口 9229 / 9222 / 9223 / 9333。

use lilyco::prelude::AppError;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// chatgpt.com 的登录态端点（同源 cookie 才有效）
const SESSION_PATH: &str = "/api/auth/session";
/// 起新标签页时访问的地址
const TARGET_URL: &str = "https://chatgpt.com/";
/// 没指定、也没有 ego-lite 配置时依次试的端口
const FALLBACK_PORTS: &[u16] = &[9229, 9222, 9223, 9333];

/// 从浏览器里抓到的登录态
#[derive(Debug)]
pub struct Session {
    // 注意：token 是敏感字段，Debug 只在测试里用；输出永远走 masked
    pub access_token: String,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub expires_at: Option<i64>,
    /// 抓到它的那个 CDP 端点（写进凭证出处，方便排查）
    pub cdp: String,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(6)))
        .http_status_as_error(false)
        .build()
        .new_agent()
}

/// ego-lite 配置里记的 CDP 端口
fn ego_lite_port() -> Option<u16> {
    let home = std::env::var_os("HOME")?;
    let path = std::path::PathBuf::from(home)
        .join(".config")
        .join("ego-lite")
        .join("config.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    v.get("cdpPort").and_then(Value::as_u64).map(|p| p as u16)
}

fn normalize(endpoint: &str) -> String {
    let e = endpoint.trim().trim_end_matches('/');
    if e.starts_with("http://") || e.starts_with("https://") {
        e.to_string()
    } else {
        format!("http://{e}")
    }
}

/// 候选 CDP 端点（去重、保序）
pub fn endpoints(explicit: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |e: String| {
        if !out.contains(&e) {
            out.push(e);
        }
    };
    if !explicit.trim().is_empty() {
        push(normalize(explicit));
    }
    if let Ok(e) = std::env::var("LCHATGPT_CDP") {
        if !e.trim().is_empty() {
            push(normalize(&e));
        }
    }
    if let Some(p) = ego_lite_port() {
        push(format!("http://127.0.0.1:{p}"));
    }
    for p in FALLBACK_PORTS {
        push(format!("http://127.0.0.1:{p}"));
    }
    out
}

/// 抓一次登录态：把候选端点挨个试一遍，第一个能给出 token 的就用它。
///
/// 例外：显式给了 `--cdp` 就**只**用那一个 —— 否则用户指名道姓要某个浏览器，
/// 我们却在别的浏览器上抓到了另一个账号的登录态，那才是真出事。
/// 真正会被试的端点：显式给了 `--cdp` 就**只**那一个，否则按 `endpoints` 的顺序挨个来。
///
/// 这条规则在抓登录态和「用网页聊天发消息」两处必须一致 —— 用户指名某个浏览器，
/// 我们却在另一个浏览器上操作，那是真事故。
pub(crate) fn candidate_endpoints(explicit: &str) -> Vec<String> {
    if explicit.trim().is_empty() {
        endpoints(explicit)
    } else {
        vec![normalize(explicit)]
    }
}

pub fn harvest(explicit_cdp: &str) -> Result<Session, AppError> {
    let mut tried: Vec<String> = Vec::new();
    for cdp in candidate_endpoints(explicit_cdp) {
        match harvest_at(&cdp) {
            Ok(s) => return Ok(s),
            Err(e) => tried.push(format!("  - {}: {}", cdp, first_line(&format!("{e}")))),
        }
    }
    let mut msg = String::from(
        "没抓到浏览器登录态。请确认：
  1. 有个 chromium 正以调试端口运行（ego-lite 默认 9229）
  2. 那个浏览器**已经登录 chatgpt.com**
  3. 或者用 --cdp 指定端点（http://127.0.0.1:9229）",
    );
    if !tried.is_empty() {
        msg.push_str("\n试过的端点：\n");
        msg.push_str(&tried.join("\n"));
    }
    Err(AppError::InvalidInput(msg))
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// 探一下这个端口是不是真的 CDP：连不上和「不是 CDP」要给不同的错。
pub(crate) fn probe(cdp: &str) -> Result<(), AppError> {
    let version = agent()
        .get(format!("{cdp}/json/version"))
        .call()
        .map_err(|e| AppError::Runtime(format!("连不上 CDP {cdp}: {e}")))?;
    if !version.status().is_success() {
        return Err(AppError::Runtime(format!(
            "{cdp}/json/version → HTTP {}（这个端口不是 CDP）",
            version.status()
        )));
    }
    Ok(())
}

fn harvest_at(cdp: &str) -> Result<Session, AppError> {
    probe(cdp)?;

    let (target_id, ws_url, owned) = pick_target(cdp)?;
    let mut client = match Client::connect(&ws_url) {
        Ok(c) => c,
        Err(e) => {
            if owned {
                close_target(cdp, &target_id);
            }
            return Err(e);
        }
    };

    let result = read_session(&mut client, cdp);
    if owned {
        close_target(cdp, &target_id);
    }
    result
}

/// 优先复用已经打开的 chatgpt 标签页；没有就自己开一个（用完关掉）
fn pick_target(cdp: &str) -> Result<(String, String, bool), AppError> {
    let list = agent()
        .get(format!("{cdp}/json/list"))
        .call()
        .map_err(|e| AppError::Runtime(format!("读 {cdp}/json/list 失败: {e}")))?;
    let raw = list
        .into_body()
        .read_to_string()
        .map_err(|e| AppError::Runtime(format!("读标签页列表失败: {e}")))?;
    let targets: Value = serde_json::from_str(&raw)
        .map_err(|e| AppError::Runtime(format!("CDP 标签页列表不是 JSON: {e}")))?;
    if let Some(items) = targets.as_array() {
        for t in items {
            let url = t.get("url").and_then(Value::as_str).unwrap_or("");
            let kind = t.get("type").and_then(Value::as_str).unwrap_or("");
            let ws = t
                .get("webSocketDebuggerUrl")
                .and_then(Value::as_str)
                .unwrap_or("");
            if kind == "page" && url.contains("chatgpt.com") && !ws.is_empty() {
                let id = t
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                return Ok((id, ws.to_string(), false));
            }
        }
    }

    // 没有现成的标签页：新开一个
    let (id, ws) = create_target(cdp)?;
    Ok((id, ws, true))
}

/// 新开一个标签页（默认打开 chatgpt.com 首页），返回 (target id, WebSocket 调试地址)。
pub(crate) fn create_target(cdp: &str) -> Result<(String, String), AppError> {
    create_target_url(cdp, TARGET_URL)
}

/// 新开一个指定 URL 的标签页（用于续接已有会话 `/c/<id>`），返回 (target id, WebSocket 调试地址)。
///
/// 注意 Chrome 只认 `PUT`，`GET /json/new` 会回 405 —— 这点坑过一次。
pub(crate) fn create_target_url(cdp: &str, url: &str) -> Result<(String, String), AppError> {
    let created = agent()
        .put(format!("{cdp}/json/new?{url}"))
        .send_empty()
        .map_err(|e| AppError::Runtime(format!("开新标签页失败: {e}")))?;
    let status = created.status();
    let body = created
        .into_body()
        .read_to_string()
        .map_err(|e| AppError::Runtime(format!("读新标签页返回失败: {e}")))?;
    if !status.is_success() {
        return Err(AppError::Runtime(format!(
            "{cdp}/json/new → HTTP {status}: {}",
            crate::util::clip(&body, 200)
        )));
    }
    let v: Value = serde_json::from_str(&body)
        .map_err(|e| AppError::Runtime(format!("新标签页返回不是 JSON: {e}")))?;
    let id = v
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Runtime("新标签页没有 id".to_string()))?
        .to_string();
    let ws = v
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Runtime("新标签页没有 webSocketDebuggerUrl".to_string()))?
        .to_string();
    Ok((id, ws))
}

/// 关掉自己开的标签页。
///
/// Linux 上的 Chromium 关掉最后一个 `page` 标签页时会把整个浏览器进程一起退出，
/// 下一次请求就会 `Connection refused`。所以关之前先数一下还剩几个 `page`，
/// 只剩 1 个时宁可留着标签页，绝不把浏览器关掉。
pub(crate) fn close_target(cdp: &str, id: &str) {
    if page_count(cdp).unwrap_or(0) <= 1 {
        return;
    }
    let _ = agent().get(format!("{cdp}/json/close/{id}")).call();
}

/// 统计浏览器当前打开的 `page` 标签页数量（读不到返回 None，宁可不关也不误杀浏览器）。
fn page_count(cdp: &str) -> Option<usize> {
    let raw = agent()
        .get(format!("{cdp}/json/list"))
        .call()
        .ok()?
        .into_body()
        .read_to_string()
        .ok()?;
    let items: Value = serde_json::from_str(&raw).ok()?;
    let n = items
        .as_array()?
        .iter()
        .filter(|t| t.get("type").and_then(Value::as_str) == Some("page"))
        .count();
    Some(n)
}

/// 新开的标签页要等几百毫秒到几秒才拿到 cookie，所以这里重试几次再放弃
fn read_session(client: &mut Client, cdp: &str) -> Result<Session, AppError> {
    let mut last = String::from("页面还没就绪");
    for attempt in 0..6 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(900));
        }
        let href = client.eval_js("location.href").unwrap_or_default();
        let href = href.as_str().unwrap_or("").to_string();
        if !href.contains("chatgpt.com") {
            last = format!("页面停在 {href}（这个账号可能还没登录 chatgpt.com）");
            continue;
        }
        let raw = match client.eval_js(FETCH_SESSION) {
            Ok(v) => v.as_str().unwrap_or("").to_string(),
            Err(e) => {
                last = format!("调用 {SESSION_PATH} 失败: {}", first_line(&format!("{e}")));
                continue;
            }
        };
        let outer: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => {
                last = format!(
                    "{SESSION_PATH} 返回无法解析: {}",
                    crate::util::clip(&raw, 120)
                );
                continue;
            }
        };
        let status = outer.get("status").and_then(Value::as_u64).unwrap_or(0);
        let body = outer.get("body").and_then(Value::as_str).unwrap_or("");
        if status != 200 {
            last = format!(
                "{SESSION_PATH} → HTTP {status}: {}",
                crate::util::clip(body, 160)
            );
            continue;
        }
        let session: Value = match serde_json::from_str(body) {
            Ok(v) => v,
            Err(_) => {
                last = format!("{SESSION_PATH} 不是 JSON: {}", crate::util::clip(body, 160));
                continue;
            }
        };
        let token = session
            .get("accessToken")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if token.is_empty() {
            last = format!("{SESSION_PATH} 里没有 accessToken（没登录）");
            continue;
        }
        return Ok(Session {
            expires_at: crate::jwt::expires_at(&token),
            account_id: session
                .pointer("/account/id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| crate::jwt::account_id(&token)),
            email: session
                .pointer("/user/email")
                .and_then(Value::as_str)
                .map(str::to_string),
            access_token: token,
            cdp: cdp.to_string(),
        });
    }
    Err(AppError::Runtime(format!(
        "连续 6 次都没拿到登录态：{last}"
    )))
}

/// 在页面里同源请求一次 `/api/auth/session`，把状态码和正文原样带回来。
/// 用字符串返回而不是对象：CDP 的 returnByValue 对超大对象会截断。
pub(crate) const FETCH_SESSION: &str = r#"(async () => {
  const r = await fetch('/api/auth/session', { credentials: 'include', headers: { accept: 'application/json' } });
  const body = await r.text();
  return JSON.stringify({ status: r.status, body: body.slice(0, 40000) });
})()"#;

/// 极简 CDP 客户端：只做「发一条命令、等同 id 的回包」
pub(crate) struct Client {
    ws: WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    next_id: u64,
}

impl Client {
    pub(crate) fn connect(ws_url: &str) -> Result<Self, AppError> {
        let (ws, _resp) = tungstenite::connect(ws_url)
            .map_err(|e| AppError::Runtime(format!("连不上 CDP WebSocket: {e}")))?;
        let mut client = Self { ws, next_id: 1 };
        if let MaybeTlsStream::Plain(stream) = client.ws.get_mut() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(25)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(25)));
        }
        Ok(client)
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, AppError> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({ "id": id, "method": method, "params": params }).to_string();
        self.ws
            .send(Message::Text(payload.into()))
            .map_err(|e| AppError::Runtime(format!("CDP 发送 {method} 失败: {e}")))?;
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            if Instant::now() > deadline {
                return Err(AppError::Runtime(format!("CDP {method} 超时")));
            }
            let msg = self
                .ws
                .read()
                .map_err(|e| AppError::Runtime(format!("CDP 读包失败: {e}")))?;
            let text = match msg {
                Message::Text(t) => t.as_str().to_string(),
                Message::Binary(b) => String::from_utf8_lossy(&b).to_string(),
                _ => continue,
            };
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("id").and_then(Value::as_u64) != Some(id) {
                continue; // 事件通知，不是我们的回包
            }
            if let Some(err) = v.get("error") {
                let msg = err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("未知错误");
                return Err(AppError::Runtime(format!("CDP {method} 报错: {msg}")));
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// 往当前聚焦的可编辑元素里插入文本。
    /// 走 CDP 的 Input 域而不是改 DOM —— 只有走 input 事件 React 的 state 才会跟着变，
    /// 发送按钮也才会从 disabled 变回可点。
    pub(crate) fn insert_text(&mut self, text: &str) -> Result<(), AppError> {
        self.call("Input.insertText", json!({ "text": text }))?;
        Ok(())
    }

    /// 用 CDP 在指定视口坐标点一下左键（Radix UI 下拉菜单只认真实 pointer/mouse 事件）
    pub(crate) fn click_point(&mut self, x: f64, y: f64) -> Result<(), AppError> {
        self.call(
            "Input.dispatchMouseEvent",
            json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1 }),
        )?;
        self.call(
            "Input.dispatchMouseEvent",
            json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1 }),
        )?;
        Ok(())
    }

    /// 用 CDP 发送一次按键（用于操作 Radix `[role="slider"]` 推理强度滑块与 Escape 关菜单）
    pub(crate) fn press_key(&mut self, key: &str, code: &str, vk: u32) -> Result<(), AppError> {
        self.call(
            "Input.dispatchKeyEvent",
            json!({ "type": "keyDown", "key": key, "code": code, "windowsVirtualKeyCode": vk }),
        )?;
        self.call(
            "Input.dispatchKeyEvent",
            json!({ "type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": vk }),
        )?;
        Ok(())
    }

    /// 在页面里求值，拿回 JSON 值（字符串 / 数字 / 布尔）
    pub(crate) fn eval_js(&mut self, expression: &str) -> Result<Value, AppError> {
        let res = self.call(
            "Runtime.evaluate",
            json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
        )?;
        if res.get("exceptionDetails").is_some() {
            let msg = res
                .pointer("/exceptionDetails/exception/description")
                .or_else(|| res.pointer("/exceptionDetails/text"))
                .and_then(Value::as_str)
                .unwrap_or("页面脚本抛错");
            return Err(AppError::Runtime(format!("页面求值出错: {msg}")));
        }
        Ok(res.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 显式指定的端点排第一，去重，且补上 http:// 前缀
    #[test]
    fn an_explicit_endpoint_comes_first_and_is_normalized() {
        let list = endpoints("127.0.0.1:9229");
        assert_eq!(list[0], "http://127.0.0.1:9229");
        let dup = endpoints("http://127.0.0.1:9229");
        assert_eq!(
            dup.iter().filter(|e| *e == "http://127.0.0.1:9229").count(),
            1
        );
        assert!(list.iter().any(|e| e == "http://127.0.0.1:9222"));
    }

    /// 连不上时必须说清「怎么办」，而不是抛一句 IO 错误
    #[test]
    fn nothing_listening_produces_an_actionable_error() {
        let err = harvest_at("http://127.0.0.1:1").expect_err("1 号端口不该有 CDP");
        let msg = format!("{err}");
        assert!(msg.contains("连不上 CDP"), "{msg}");
    }
}
