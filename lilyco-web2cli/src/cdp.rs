//! 纯 Rust CDP（Chrome DevTools Protocol）实时浏览器会话控制。
//!
//! 借鉴 `vercel-labs/agent-browser`（`@e1`/`@e2` 编号引用）与 `citrolabs/ego-lite`（多层 `iframe` 快照 + 批处理）：
//! - 可附着到用户已扫码登录的 Chrome 调试端口（`--cdp http://127.0.0.1:9222`），直接复用政务网登录态
//! - 也可按需拉起临时隔离的 Chrome/Chromium 实例（`ManagedChrome`）
//! - 支持：导航、递归穿透 `iframe` 与 `ShadowRoot` 的页面快照、兼容 Vue/React/ElementUI 的填表与点击、整页截图入审、以及一键导出全部登录态 Cookies

use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use lilyco::__core::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::session::{find_browser, SessionManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpTarget {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(rename = "type", default)]
    pub target_type: String,
    #[serde(rename = "webSocketDebuggerUrl", default)]
    pub ws_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpElement {
    pub ref_id: String,
    pub frame_path: String,
    pub tag: String,
    #[serde(default)]
    pub input_type: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpSnapshot {
    pub url: String,
    pub title: String,
    pub frame_count: usize,
    pub elements: Vec<CdpElement>,
    pub compact_view: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    #[serde(default)]
    pub path: String,
}

pub struct CdpClient {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl CdpClient {
    /// 列出指定 CDP HTTP 端点（如 `http://127.0.0.1:9222`）下的全部目标页签。
    pub fn list_targets(cdp_http: &str) -> Result<Vec<CdpTarget>, AppError> {
        let base = cdp_http.trim_end_matches('/');
        let url = format!("{base}/json/list");
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .new_agent();
        let mut resp = agent
            .get(&url)
            .call()
            .map_err(|e| AppError::Runtime(format!("连接 CDP `{url}` 失败: {e}")))?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| AppError::Runtime(format!("读取 CDP 目标列表失败: {e}")))?;
        serde_json::from_str(&text)
            .map_err(|e| AppError::Runtime(format!("解析 CDP /json/list 失败: {e}")))
    }

    /// 附着到指定 CDP HTTP 端点的第一个 `page` 页签（或含 `url_filter` 的页签）。
    pub fn connect_http(cdp_http: &str, url_filter: Option<&str>) -> Result<Self, AppError> {
        let targets = Self::list_targets(cdp_http)?;
        let chosen = targets
            .iter()
            .find(|t| {
                t.target_type == "page"
                    && !t.ws_url.is_empty()
                    && url_filter.is_none_or(|f| t.url.contains(f))
            })
            .or_else(|| {
                targets
                    .iter()
                    .find(|t| t.target_type == "page" && !t.ws_url.is_empty())
            })
            .ok_or_else(|| {
                AppError::Runtime(format!(
                    "在 `{cdp_http}` 未找到可用的 page 目标（请确认 Chrome 启动时带了 --remote-debugging-port）"
                ))
            })?;
        Self::connect_ws(&chosen.ws_url)
    }

    pub fn connect_ws(ws_url: &str) -> Result<Self, AppError> {
        let (ws, _resp) = tungstenite::connect(ws_url)
            .map_err(|e| AppError::Runtime(format!("连接 CDP WebSocket `{ws_url}` 失败: {e}")))?;
        Ok(Self { ws, next_id: 1 })
    }

    /// 发送一条 CDP JSON-RPC 指令并等待对应 `id` 的返回。
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, AppError> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "id": id,
            "method": method,
            "params": params,
        });
        self.ws
            .send(Message::Text(payload.to_string().into()))
            .map_err(|e| AppError::Runtime(format!("发送 CDP `{method}` 失败: {e}")))?;

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if Instant::now() > deadline {
                return Err(AppError::Runtime(format!("CDP `{method}` 等待响应超时")));
            }
            let msg = self
                .ws
                .read()
                .map_err(|e| AppError::Runtime(format!("读取 CDP `{method}` 响应失败: {e}")))?;
            if let Message::Text(txt) = msg {
                let Ok(v) = serde_json::from_str::<Value>(&txt) else {
                    continue;
                };
                if v.get("id").and_then(Value::as_u64) == Some(id) {
                    if let Some(err) = v.get("error") {
                        return Err(AppError::Runtime(format!("CDP `{method}` 返回错误: {err}")));
                    }
                    return Ok(v.get("result").cloned().unwrap_or(Value::Null));
                }
            }
        }
    }

    /// 导航到目标 URL 并等待文档加载完成。
    pub fn navigate(&mut self, url: &str, settle_ms: u64) -> Result<(), AppError> {
        let _ = self.call("Page.enable", json!({}));
        let _ = self.call("Runtime.enable", json!({}));
        self.call("Page.navigate", json!({ "url": url }))?;

        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Ok(state) = self.eval_js("document.readyState") {
                if state.as_str() == Some("complete") {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(120));
        }
        if settle_ms > 0 {
            thread::sleep(Duration::from_millis(settle_ms));
        }
        Ok(())
    }

    /// 在当前页面执行 JS 并按值返回结果。
    pub fn eval_js(&mut self, expression: &str) -> Result<Value, AppError> {
        let res = self.call(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true,
            }),
        )?;
        if let Some(exc) = res.get("exceptionDetails") {
            return Err(AppError::Runtime(format!("页面 JS 执行异常: {exc}")));
        }
        Ok(res.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    /// 递归穿透主文档、同源多层 `iframe` 与开放 `ShadowRoot`，生成 `@e1..@eN` 交互快照。
    pub fn snapshot_all_frames(&mut self) -> Result<CdpSnapshot, AppError> {
        let js = r#"(() => {
  let idx = 0;
  let frameCount = 0;
  const elements = [];

  function labelFor(doc, el) {
    if (el.id) {
      const lbl = doc.querySelector(`label[for="${CSS.escape(el.id)}"]`);
      if (lbl && lbl.innerText.trim()) return lbl.innerText.trim();
    }
    const parentLabel = el.closest && el.closest('label');
    if (parentLabel && parentLabel.innerText.trim()) return parentLabel.innerText.trim();
    return (
      el.getAttribute('aria-label') ||
      el.getAttribute('placeholder') ||
      el.innerText ||
      el.getAttribute('name') ||
      ''
    ).trim().slice(0, 80);
  }

  function walkRoot(doc, root, framePath) {
    const selector = 'input, textarea, select, button, a[href], [role="button"], [role="combobox"], [contenteditable="true"]';
    const nodes = root.querySelectorAll(selector);
    for (const el of nodes) {
      const tag = el.tagName.toLowerCase();
      const itype = (el.getAttribute('type') || '').toLowerCase();
      if (tag === 'input' && itype === 'hidden') continue;
      idx += 1;
      const refId = `e${idx}`;
      el.setAttribute('data-lilyco-ref', refId);
      const options = tag === 'select'
        ? Array.from(el.options || []).map(o => o.value || o.text).filter(Boolean).slice(0, 30)
        : [];
      elements.push({
        ref_id: refId,
        frame_path: framePath,
        tag,
        input_type: itype,
        name: el.getAttribute('name') || '',
        label: labelFor(doc, el),
        placeholder: el.getAttribute('placeholder') || '',
        value: typeof el.value === 'string' ? el.value : '',
        required: !!el.required,
        options,
      });
    }
    // Walk open shadow roots
    const all = root.querySelectorAll('*');
    for (const el of all) {
      if (el.shadowRoot) {
        walkRoot(doc, el.shadowRoot, `${framePath}/shadow`);
      }
    }
    // Walk accessible child iframes
    const frames = root.querySelectorAll('iframe, frame');
    let fIdx = 0;
    for (const fr of frames) {
      fIdx += 1;
      try {
        const fdoc = fr.contentDocument;
        if (fdoc && fdoc.documentElement) {
          frameCount += 1;
          walkRoot(fdoc, fdoc, `${framePath}/iframe[${fIdx}]`);
        }
      } catch (_e) {}
    }
  }

  frameCount = 1;
  walkRoot(document, document, 'main');
  return {
    url: location.href,
    title: document.title || location.href,
    frame_count: frameCount,
    elements,
  };
})()"#;

        let val = self.eval_js(js)?;
        let url = val
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let title = val
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let frame_count = val.get("frame_count").and_then(Value::as_u64).unwrap_or(1) as usize;
        let elements: Vec<CdpElement> = val
            .get("elements")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        let mut lines = vec![
            format!("# {title}"),
            format!("URL: {url} (frames: {frame_count})"),
        ];
        for el in &elements {
            let req_mark = if el.required { "*" } else { "" };
            let frame_tag = if el.frame_path == "main" {
                String::new()
            } else {
                format!(" [{}]", el.frame_path)
            };
            lines.push(format!(
                "[@{}]{frame_tag} <{}{req_mark}> name='{}' label='{}' val='{}'",
                el.ref_id,
                if el.input_type.is_empty() {
                    el.tag.as_str()
                } else {
                    el.input_type.as_str()
                },
                el.name,
                el.label,
                el.value
            ));
        }

        Ok(CdpSnapshot {
            url,
            title,
            frame_count,
            elements,
            compact_view: lines.join("\n"),
        })
    }

    /// 向 `@eN` 控件填入值（穿透 `iframe` / `ShadowRoot`，兼容 Vue/React 受控组件事件）。
    pub fn fill_ref(&mut self, ref_id: &str, value: &str) -> Result<bool, AppError> {
        let clean_ref = ref_id.trim_start_matches('@');
        let ref_json = serde_json::to_string(clean_ref).unwrap();
        let val_json = serde_json::to_string(value).unwrap();
        let js = format!(
            r#"(() => {{
  const targetRef = {ref_json};
  const newVal = {val_json};
  function findIn(root) {{
    const direct = root.querySelector(`[data-lilyco-ref="${{targetRef}}"]`);
    if (direct) return direct;
    for (const el of root.querySelectorAll('*')) {{
      if (el.shadowRoot) {{
        const found = findIn(el.shadowRoot);
        if (found) return found;
      }}
    }}
    for (const fr of root.querySelectorAll('iframe, frame')) {{
      try {{
        if (fr.contentDocument) {{
          const found = findIn(fr.contentDocument);
          if (found) return found;
        }}
      }} catch (_e) {{}}
    }}
    return null;
  }}
  const el = findIn(document);
  if (!el) return false;
  el.focus && el.focus();
  const proto = el.tagName === 'TEXTAREA'
    ? HTMLTextAreaElement.prototype
    : (el.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype);
  const desc = Object.getOwnPropertyDescriptor(proto, 'value');
  if (desc && desc.set) {{
    desc.set.call(el, newVal);
  }} else if (el.isContentEditable) {{
    el.innerText = newVal;
  }} else {{
    el.value = newVal;
  }}
  el.dispatchEvent(new Event('input', {{ bubbles: true }}));
  el.dispatchEvent(new Event('change', {{ bubbles: true }}));
  el.dispatchEvent(new Event('blur', {{ bubbles: true }}));
  return true;
}})()"#
        );
        let ok = self.eval_js(&js)?.as_bool().unwrap_or(false);
        Ok(ok)
    }

    /// 单次调用整表批量填报（对标 `turbo-browser-mcp` one-call form filling），一次 CDP 往返完成全部字段填充。
    pub fn fill_batch(&mut self, pairs: &[(String, String)]) -> Result<usize, AppError> {
        if pairs.is_empty() {
            return Ok(0);
        }
        let entries: Vec<Value> = pairs
            .iter()
            .map(|(r, v)| json!([r.trim_start_matches('@'), v]))
            .collect();
        let entries_json = serde_json::to_string(&entries).unwrap();
        let js = format!(
            r#"(() => {{
  const pairs = {entries_json};
  function findIn(root, targetRef) {{
    const direct = root.querySelector(`[data-lilyco-ref="${{targetRef}}"]`);
    if (direct) return direct;
    for (const el of root.querySelectorAll('*')) {{
      if (el.shadowRoot) {{
        const found = findIn(el.shadowRoot, targetRef);
        if (found) return found;
      }}
    }}
    for (const fr of root.querySelectorAll('iframe, frame')) {{
      try {{
        if (fr.contentDocument) {{
          const found = findIn(fr.contentDocument, targetRef);
          if (found) return found;
        }}
      }} catch (_e) {{}}
    }}
    return null;
  }}
  let count = 0;
  for (const [refId, newVal] of pairs) {{
    const el = findIn(document, refId);
    if (!el) continue;
    el.focus && el.focus();
    const proto = el.tagName === 'TEXTAREA'
      ? HTMLTextAreaElement.prototype
      : (el.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype);
    const desc = Object.getOwnPropertyDescriptor(proto, 'value');
    if (desc && desc.set) {{
      desc.set.call(el, newVal);
    }} else if (el.isContentEditable) {{
      el.innerText = newVal;
    }} else {{
      el.value = newVal;
    }}
    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
    el.dispatchEvent(new Event('blur', {{ bubbles: true }}));
    count += 1;
  }}
  return count;
}})()"#
        );
        Ok(self.eval_js(&js)?.as_u64().unwrap_or(0) as usize)
    }

    /// 适配政务网高频假下拉框（ElementUI `el-select` / AntD `ant-select` / ARIA `listbox`）：点击触发器并按文本选中项。
    pub fn select_custom_dropdown(
        &mut self,
        ref_id: &str,
        option_text: &str,
    ) -> Result<bool, AppError> {
        if !self.click_ref(ref_id)? {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(180));
        let text_json = serde_json::to_string(option_text).unwrap();
        let js = format!(
            r#"(() => {{
  const target = {text_json}.trim();
  const candidates = document.querySelectorAll(
    '.el-select-dropdown__item, .ant-select-item-option, [role="option"], li.option, .dropdown-item'
  );
  for (const item of candidates) {{
    const txt = (item.innerText || item.textContent || '').trim();
    if (txt === target || txt.includes(target)) {{
      item.click();
      return true;
    }}
  }}
  return false;
}})()"#
        );
        Ok(self.eval_js(&js)?.as_bool().unwrap_or(false))
    }

    /// 通过 CDP `DOM.setFileInputFiles` 向 `<input type="file">` 注入真实本地证照/PDF 附件。
    pub fn set_file_input_files(
        &mut self,
        ref_id: &str,
        files: &[String],
    ) -> Result<bool, AppError> {
        let clean_ref = ref_id.trim_start_matches('@');
        let _ = self.call("DOM.enable", json!({}));
        let doc = self.call("DOM.getDocument", json!({ "depth": 1 }))?;
        let root_node_id = doc
            .pointer("/root/nodeId")
            .and_then(Value::as_i64)
            .ok_or_else(|| AppError::Runtime("CDP DOM.getDocument 缺少 root.nodeId".into()))?;
        let sel = format!(r#"[data-lilyco-ref="{clean_ref}"]"#);
        let q = self.call(
            "DOM.querySelector",
            json!({ "nodeId": root_node_id, "selector": sel }),
        )?;
        let node_id = q.get("nodeId").and_then(Value::as_i64).unwrap_or(0);
        if node_id == 0 {
            return Ok(false);
        }
        self.call(
            "DOM.setFileInputFiles",
            json!({ "nodeId": node_id, "files": files }),
        )?;
        Ok(true)
    }

    /// 计算两次快照之间的增量变更（对标 `turbo-browser-mcp` delta snapshots），大幅节省 token。
    pub fn delta_snapshot(prev: &CdpSnapshot, next: &CdpSnapshot) -> Vec<CdpElement> {
        next.elements
            .iter()
            .filter(|cur| {
                !prev.elements.iter().any(|old| {
                    old.frame_path == cur.frame_path
                        && old.tag == cur.tag
                        && old.name == cur.name
                        && old.label == cur.label
                        && old.value == cur.value
                })
            })
            .cloned()
            .collect()
    }

    /// 点击 `@eN` 控件（穿透 `iframe` / `ShadowRoot`）。
    pub fn click_ref(&mut self, ref_id: &str) -> Result<bool, AppError> {
        let clean_ref = ref_id.trim_start_matches('@');
        let ref_json = serde_json::to_string(clean_ref).unwrap();
        let js = format!(
            r#"(() => {{
  const targetRef = {ref_json};
  function findIn(root) {{
    const direct = root.querySelector(`[data-lilyco-ref="${{targetRef}}"]`);
    if (direct) return direct;
    for (const el of root.querySelectorAll('*')) {{
      if (el.shadowRoot) {{
        const found = findIn(el.shadowRoot);
        if (found) return found;
      }}
    }}
    for (const fr of root.querySelectorAll('iframe, frame')) {{
      try {{
        if (fr.contentDocument) {{
          const found = findIn(fr.contentDocument);
          if (found) return found;
        }}
      }} catch (_e) {{}}
    }}
    return null;
  }}
  const el = findIn(document);
  if (!el) return false;
  el.scrollIntoView && el.scrollIntoView({{ block: 'center' }});
  el.click();
  return true;
}})()"#
        );
        let ok = self.eval_js(&js)?.as_bool().unwrap_or(false);
        Ok(ok)
    }

    /// 被动检测当前页面是否出现验证码、短信验证、UKey 或人机校验提示。
    ///
    /// **安全与合规原则**：本方法绝不尝试破解、点击或绕过任何验证；
    /// 一旦检测到验证界面，立刻返回命中原因，供上层流程暂停并交由操作员在可见浏览器中亲手完成。
    pub fn detect_verification_challenge(&mut self) -> Result<Option<String>, AppError> {
        let js = r#"(() => {
  const title = (document.title || '').toLowerCase();
  const body = (document.body ? document.body.innerText : '').toLowerCase().slice(0, 4000);
  const text = title + '\n' + body;
  const markers = [
    ['captcha', '检测到验证码（CAPTCHA）界面'],
    ['verify you are human', '检测到人机校验界面'],
    ['安全验证', '检测到安全验证界面'],
    ['人机验证', '检测到人机验证界面'],
    ['滑动拼图', '检测到滑块验证界面'],
    ['短信验证码', '检测到短信验证码界面'],
    ['ukey', '检测到 UKey 硬件证书验证界面'],
    ['扫码登录', '检测到扫码登录界面'],
  ];
  for (const [kw, reason] of markers) {
    if (text.includes(kw)) return reason;
  }
  const hasCaptchaFrame = Array.from(document.querySelectorAll('iframe')).some(f => {
    const src = (f.getAttribute('src') || '').toLowerCase();
    return src.includes('captcha') || src.includes('turnstile') || src.includes('recaptcha') || src.includes('hcaptcha');
  });
  if (hasCaptchaFrame) return '检测到嵌入式人机校验 iframe';
  return null;
})()"#;
        let val = self.eval_js(js)?;
        Ok(val.as_str().map(String::from))
    }

    /// 截取当前网页 PNG 并落盘（用于人类审核草稿时查看真实页面填报效果）。
    pub fn capture_screenshot(&mut self, out_path: &Path) -> Result<usize, AppError> {
        let res = self.call(
            "Page.captureScreenshot",
            json!({ "format": "png", "fromSurface": true }),
        )?;
        let b64 = res
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::Runtime("CDP 截图响应缺少 base64 data".into()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| AppError::Runtime(format!("解码截图 base64 失败: {e}")))?;
        if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::Runtime(format!("创建截图目录失败: {e}")))?;
        }
        fs::write(out_path, &bytes).map_err(|e| {
            AppError::Runtime(format!("写入截图 `{}` 失败: {e}", out_path.display()))
        })?;
        Ok(bytes.len())
    }

    /// 导出已登录 Chrome 的全部 Cookies，并可直接写入 `SessionManager`。
    pub fn export_cookies(&mut self) -> Result<Vec<CdpCookie>, AppError> {
        let _ = self.call("Network.enable", json!({}));
        let res = self.call("Network.getAllCookies", json!({}))?;
        let list = res
            .get("cookies")
            .cloned()
            .and_then(|v| serde_json::from_value::<Vec<CdpCookie>>(v).ok())
            .unwrap_or_default();
        Ok(list)
    }

    /// 将当前 CDP 浏览器登录态 Cookies 一键同步到 `SessionManager`。
    pub fn sync_cookies_to_session(&mut self, session: &SessionManager) -> Result<usize, AppError> {
        let cookies = self.export_cookies()?;
        if cookies.is_empty() {
            return Ok(0);
        }
        let json_str = serde_json::to_string(&cookies)
            .map_err(|e| AppError::Runtime(format!("序列化 CDP Cookies 失败: {e}")))?;
        session.import_cookies_from_str(&json_str, None)
    }
}

/// 托管的临时无头 Chrome/Chromium 实例（退出时自动回收进程与临时 profile）。
pub struct ManagedChrome {
    child: Child,
    pub cdp_http: String,
    profile_dir: std::path::PathBuf,
}

impl ManagedChrome {
    pub fn launch() -> Result<Self, AppError> {
        let browser = find_browser().ok_or_else(|| {
            AppError::Runtime("未找到本机 Chrome/Chromium/Edge，无法启动托管 CDP 实例".into())
        })?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| AppError::Runtime(format!("分配调试端口失败: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::Runtime(format!("读取端口失败: {e}")))?
            .port();
        drop(listener);

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let profile_dir = std::env::temp_dir().join(format!("lilyco-cdp-profile-{stamp}"));
        fs::create_dir_all(&profile_dir)
            .map_err(|e| AppError::Runtime(format!("创建临时 profile 目录失败: {e}")))?;

        let child = Command::new(&browser)
            .args([
                "--headless=new",
                "--disable-gpu",
                "--no-sandbox",
                "--no-first-run",
                "--no-default-browser-check",
                &format!("--remote-debugging-port={port}"),
                &format!("--user-data-dir={}", profile_dir.display()),
                "about:blank",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::Runtime(format!("启动浏览器 `{browser}` 失败: {e}")))?;

        let cdp_http = format!("http://127.0.0.1:{port}");
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(targets) = CdpClient::list_targets(&cdp_http) {
                if targets
                    .iter()
                    .any(|t| t.target_type == "page" && !t.ws_url.is_empty())
                {
                    return Ok(Self {
                        child,
                        cdp_http,
                        profile_dir,
                    });
                }
            }
            thread::sleep(Duration::from_millis(120));
        }

        Err(AppError::Runtime(format!(
            "托管浏览器启动超时（{cdp_http} 未就绪）"
        )))
    }
}

impl Drop for ManagedChrome {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.profile_dir);
    }
}
