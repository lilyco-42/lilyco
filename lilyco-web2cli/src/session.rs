//! 持久化 HTTP 会话与可选无头浏览器 DOM 渲染。
//!
//! - 使用 `ureq 3.x` 同步客户端（零 tokio 依赖，匹配 `lilyco-core` 同步 Handler）
//! - 支持按 `--session <name>` 持久化 Cookie、最近访问 URL 与最近响应正文（用于 `do <n>` / `find` / `read <n>`）
//! - 支持 `--render`（或自动 SPA 壳检测）调用本机 Chrome / Chromium / Edge `--dump-dom` 渲染 JS 页面

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use lilyco::__core::error::AppError;
use serde::{Deserialize, Serialize};
use ureq::ResponseExt;

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    pub name: String,
    pub last_url: Option<String>,
    pub last_content_type: Option<String>,
    pub last_body: Option<String>,
    /// host -> (cookie_name -> cookie_value)
    #[serde(default)]
    pub cookies: BTreeMap<String, BTreeMap<String, String>>,
    /// extra headers applied to all requests in this session
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub final_url: String,
    pub status: u16,
    pub content_type: String,
    pub body: String,
    pub rendered_with_browser: bool,
}

#[derive(Debug)]
pub struct SessionManager {
    session_dir: PathBuf,
    session_name: String,
    state: Mutex<SessionState>,
}

impl SessionManager {
    pub fn new(session_name: impl Into<String>) -> Self {
        let name = session_name.into();
        let dir = default_session_dir();
        Self::with_dir(name, dir)
    }

    pub fn with_dir(session_name: impl Into<String>, session_dir: PathBuf) -> Self {
        let name = sanitize_session_name(&session_name.into());
        let state = load_session(&session_dir, &name).unwrap_or_else(|| SessionState {
            name: name.clone(),
            ..Default::default()
        });
        Self {
            session_dir,
            session_name: name,
            state: Mutex::new(state),
        }
    }

    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    pub fn state_snapshot(&self) -> SessionState {
        self.state.lock().expect("session lock").clone()
    }

    pub fn seed_cookie(&self, domain: &str, name: &str, value: &str) -> Result<(), AppError> {
        let mut st = self.state.lock().expect("session lock");
        st.cookies
            .entry(normalize_domain(domain))
            .or_default()
            .insert(name.to_string(), value.to_string());
        save_session(&self.session_dir, &st)
    }

    /// 多格式批量导入 Cookie：
    /// 1. JSON 数组（EditThisCookie / Cookie-Editor）或 Playwright `storageState` (`{"cookies": [...]}`)
    /// 2. Netscape `cookies.txt`（7 列 Tab 分隔，兼容 `#HttpOnly_` 前缀）
    /// 3. 原始 `Cookie:` 请求头（`a=1; b=2`，需提供 `default_domain`）
    pub fn import_cookies_from_str(
        &self,
        content: &str,
        default_domain: Option<&str>,
    ) -> Result<usize, AppError> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidInput("Cookie 内容为空".into()));
        }

        let mut imported: Vec<(String, String, String)> = Vec::new();

        if trimmed.starts_with('[') || trimmed.starts_with('{') {
            let val: serde_json::Value = serde_json::from_str(trimmed)
                .map_err(|e| AppError::InvalidInput(format!("解析 Cookie JSON 失败: {e}")))?;
            let list = if let Some(arr) = val.as_array() {
                arr.as_slice()
            } else if let Some(arr) = val.get("cookies").and_then(|v| v.as_array()) {
                arr.as_slice()
            } else {
                return Err(AppError::InvalidInput(
                    "JSON 须为 Cookie 数组或含 `cookies` 字段的 storageState 对象".into(),
                ));
            };
            for item in list {
                let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let Some(value) = item.get("value").and_then(|v| v.as_str()) else {
                    continue;
                };
                let domain = item
                    .get("domain")
                    .and_then(|v| v.as_str())
                    .or(default_domain)
                    .unwrap_or("");
                if !domain.is_empty() {
                    imported.push((
                        normalize_domain(domain),
                        name.to_string(),
                        value.to_string(),
                    ));
                }
            }
        } else if trimmed.lines().any(|l| l.split('\t').count() >= 7) {
            // Netscape cookies.txt format
            for raw_line in trimmed.lines() {
                let line = raw_line.trim();
                if line.is_empty() {
                    continue;
                }
                let line = if let Some(rest) = line.strip_prefix("#HttpOnly_") {
                    rest
                } else if line.starts_with('#') {
                    continue;
                } else {
                    line
                };
                let cols: Vec<&str> = line.split('\t').collect();
                if cols.len() >= 7 {
                    let domain = normalize_domain(cols[0]);
                    let name = cols[5].trim();
                    let value = cols[6].trim();
                    if !domain.is_empty() && !name.is_empty() {
                        imported.push((domain, name.to_string(), value.to_string()));
                    }
                }
            }
        } else {
            // Raw Cookie header string: "k1=v1; k2=v2"
            let domain = default_domain
                .map(normalize_domain)
                .filter(|d| !d.is_empty())
                .ok_or_else(|| {
                    AppError::InvalidArg("导入原始 Cookie 请求头字符串时必须指定 --domain".into())
                })?;
            let stripped = trimmed
                .strip_prefix("Cookie:")
                .or_else(|| trimmed.strip_prefix("cookie:"))
                .unwrap_or(trimmed);
            for part in stripped.split(';') {
                if let Some((k, v)) = part.trim().split_once('=') {
                    let k = k.trim();
                    let v = v.trim();
                    if !k.is_empty() {
                        imported.push((domain.clone(), k.to_string(), v.to_string()));
                    }
                }
            }
        }

        if imported.is_empty() {
            return Err(AppError::InvalidInput("未解析到任何有效 Cookie".into()));
        }

        let count = imported.len();
        let mut st = self.state.lock().expect("session lock");
        for (domain, name, value) in imported {
            st.cookies.entry(domain).or_default().insert(name, value);
        }
        save_session(&self.session_dir, &st)?;
        Ok(count)
    }

    pub fn set_header(&self, name: &str, value: &str) -> Result<(), AppError> {
        let mut st = self.state.lock().expect("session lock");
        st.headers.insert(name.to_string(), value.to_string());
        save_session(&self.session_dir, &st)
    }

    pub fn clear(&self) -> Result<(), AppError> {
        let mut st = self.state.lock().expect("session lock");
        *st = SessionState {
            name: self.session_name.clone(),
            ..Default::default()
        };
        let path = session_file(&self.session_dir, &self.session_name);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| AppError::Runtime(format!("删除会话失败: {e}")))?;
        }
        Ok(())
    }

    pub fn list_sessions(&self) -> Vec<String> {
        let Ok(rd) = fs::read_dir(&self.session_dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
        out.sort();
        out
    }

    /// 发送 HTTP 请求（支持 GET/POST/PUT/DELETE/PATCH），自动携带并更新会话 Cookie。
    pub fn request(
        &self,
        method: &str,
        url_str: &str,
        headers: &BTreeMap<String, String>,
        body: Option<(&str, &str)>, // (content_type, payload)
        render_js: bool,
    ) -> Result<FetchResponse, AppError> {
        self.request_bytes(
            method,
            url_str,
            headers,
            body.map(|(ct, s)| (ct, s.as_bytes())),
            render_js,
        )
    }

    /// 发送带任意二进制请求体（如 `multipart/form-data` 附件上传）的 HTTP 请求。
    pub fn request_bytes(
        &self,
        method: &str,
        url_str: &str,
        headers: &BTreeMap<String, String>,
        body: Option<(&str, &[u8])>, // (content_type, payload bytes)
        render_js: bool,
    ) -> Result<FetchResponse, AppError> {
        let parsed = url::Url::parse(url_str)
            .map_err(|e| AppError::InvalidArg(format!("无效 URL `{url_str}`: {e}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(AppError::InvalidArg(format!(
                "仅支持 http/https 协议，收到: {}",
                parsed.scheme()
            )));
        }

        if render_js && method.eq_ignore_ascii_case("GET") {
            if let Ok(dom_html) = render_with_headless_browser(url_str) {
                let mut st = self.state.lock().expect("session lock");
                st.last_url = Some(url_str.to_string());
                st.last_content_type = Some("text/html; charset=utf-8".to_string());
                st.last_body = Some(dom_html.clone());
                let _ = save_session(&self.session_dir, &st);
                return Ok(FetchResponse {
                    final_url: url_str.to_string(),
                    status: 200,
                    content_type: "text/html; charset=utf-8".to_string(),
                    body: dom_html,
                    rendered_with_browser: true,
                });
            }
        }

        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .new_agent();

        let cookie_header = {
            let st = self.state.lock().expect("session lock");
            build_cookie_header(&st, parsed.host_str().unwrap_or(""))
        };
        let session_headers = {
            let st = self.state.lock().expect("session lock");
            st.headers.clone()
        };

        let upper = method.to_ascii_uppercase();
        let mut resp = match upper.as_str() {
            "GET" => {
                let mut req = agent
                    .get(url_str)
                    .header("User-Agent", DEFAULT_USER_AGENT)
                    .header(
                        "Accept",
                        "text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8",
                    );
                if let Some(c) = &cookie_header {
                    req = req.header("Cookie", c);
                }
                for (k, v) in &session_headers {
                    req = req.header(k, v);
                }
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                req.call()
            }
            "DELETE" => {
                let mut req = agent
                    .delete(url_str)
                    .header("User-Agent", DEFAULT_USER_AGENT);
                if let Some(c) = &cookie_header {
                    req = req.header("Cookie", c);
                }
                for (k, v) in &session_headers {
                    req = req.header(k, v);
                }
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                req.call()
            }
            "POST" | "PUT" | "PATCH" => {
                let (ct, payload) = body.unwrap_or(("application/x-www-form-urlencoded", b""));
                let mut req = match upper.as_str() {
                    "PUT" => agent.put(url_str),
                    "PATCH" => agent.patch(url_str),
                    _ => agent.post(url_str),
                }
                .header("User-Agent", DEFAULT_USER_AGENT)
                .header("Content-Type", ct);
                if let Some(c) = &cookie_header {
                    req = req.header("Cookie", c);
                }
                for (k, v) in &session_headers {
                    req = req.header(k, v);
                }
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                req.send(payload)
            }
            other => {
                return Err(AppError::InvalidArg(format!("不支持的 HTTP 方法: {other}")));
            }
        }
        .map_err(|e| AppError::Runtime(format!("HTTP 请求失败 ({url_str}): {e}")))?;

        let status = resp.status().as_u16();
        let final_url = resp.get_uri().to_string();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/plain")
            .to_string();

        // Capture Set-Cookie headers
        let set_cookies: Vec<String> = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok().map(String::from))
            .collect();

        let body_text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| AppError::Runtime(format!("读取响应体失败: {e}")))?;

        let host = url::Url::parse(&final_url)
            .ok()
            .and_then(|u| u.host_str().map(String::from))
            .unwrap_or_else(|| parsed.host_str().unwrap_or("").to_string());

        {
            let mut st = self.state.lock().expect("session lock");
            for sc in &set_cookies {
                if let Some((nv, _rest)) = sc.split_once(';') {
                    if let Some((k, v)) = nv.split_once('=') {
                        st.cookies
                            .entry(host.to_ascii_lowercase())
                            .or_default()
                            .insert(k.trim().to_string(), v.trim().to_string());
                    }
                } else if let Some((k, v)) = sc.split_once('=') {
                    st.cookies
                        .entry(host.to_ascii_lowercase())
                        .or_default()
                        .insert(k.trim().to_string(), v.trim().to_string());
                }
            }
            st.last_url = Some(final_url.clone());
            st.last_content_type = Some(content_type.clone());
            st.last_body = Some(body_text.clone());
            let _ = save_session(&self.session_dir, &st);
        }

        Ok(FetchResponse {
            final_url,
            status,
            content_type,
            body: body_text,
            rendered_with_browser: false,
        })
    }
}

fn normalize_domain(domain: &str) -> String {
    domain.trim().trim_start_matches('.').to_ascii_lowercase()
}

fn build_cookie_header(st: &SessionState, host: &str) -> Option<String> {
    let host_lc = host.to_ascii_lowercase();
    let mut pairs = Vec::new();
    for (domain, kvs) in &st.cookies {
        if host_lc == *domain || host_lc.ends_with(&format!(".{domain}")) {
            for (k, v) in kvs {
                pairs.push(format!("{k}={v}"));
            }
        }
    }
    if pairs.is_empty() {
        None
    } else {
        Some(pairs.join("; "))
    }
}

fn default_session_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg)
                .join("lilyco")
                .join("web2cli")
                .join("sessions");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".config")
                .join("lilyco")
                .join("web2cli")
                .join("sessions");
        }
    }
    std::env::temp_dir().join("lilyco-web2cli-sessions")
}

fn sanitize_session_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "default".to_string()
    } else {
        cleaned
    }
}

fn session_file(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.json"))
}

fn load_session(dir: &Path, name: &str) -> Option<SessionState> {
    let p = session_file(dir, name);
    let data = fs::read_to_string(p).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_session(dir: &Path, st: &SessionState) -> Result<(), AppError> {
    fs::create_dir_all(dir).map_err(|e| AppError::Runtime(format!("创建会话目录失败: {e}")))?;
    let p = session_file(dir, &st.name);
    let json = serde_json::to_string_pretty(st)
        .map_err(|e| AppError::Runtime(format!("序列化会话失败: {e}")))?;
    fs::write(&p, json).map_err(|e| AppError::Runtime(format!("写入会话失败: {e}")))?;
    Ok(())
}

/// 查找本机 Chrome / Chromium / Edge，用于 `--render` JS 页面渲染。
pub fn find_browser() -> Option<String> {
    if let Ok(p) = std::env::var("CHROME_PATH") {
        let p = p.trim();
        if !p.is_empty() && Path::new(p).is_file() {
            return Some(p.to_string());
        }
    }
    let candidates = [
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    ];
    for c in candidates {
        if Path::new(c).is_file() {
            return Some(c.to_string());
        }
    }
    if let Ok(path_env) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for name in ["chromium", "google-chrome", "chromium-browser", "msedge"] {
            for dir in path_env.split(sep) {
                let p = Path::new(dir).join(name);
                if p.is_file() {
                    return Some(p.display().to_string());
                }
            }
        }
    }
    None
}

/// 使用无头浏览器 `--dump-dom` 获取 SPA / JS 渲染后的完整 DOM。
pub fn render_with_headless_browser(url_str: &str) -> Result<String, AppError> {
    let browser = find_browser().ok_or_else(|| {
        AppError::Runtime(
            "未找到 Chrome/Chromium/Edge，无法执行 --render；可设置 CHROME_PATH".into(),
        )
    })?;
    let out = Command::new(&browser)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--no-sandbox",
            "--dump-dom",
            url_str,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| AppError::Runtime(format!("启动浏览器失败: {e}")))?;
    if !out.status.success() {
        return Err(AppError::Runtime(format!(
            "无头浏览器退出码非零: {:?}",
            out.status.code()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_json_netscape_and_raw_cookie_formats() {
        let dir = tempfile::tempdir().unwrap();
        let sess = SessionManager::with_dir("cookie-test", dir.path().to_path_buf());

        // 1. Playwright storageState JSON
        let n1 = sess
            .import_cookies_from_str(
                r#"{"cookies":[{"domain":".gov.cn","name":"sso","value":"v1"}]}"#,
                None,
            )
            .unwrap();
        assert_eq!(n1, 1);

        // 2. Netscape cookies.txt
        let netscape =
            "# Netscape HTTP Cookie File\n#HttpOnly_.zwfw.gov.cn\tTRUE\t/\tTRUE\t0\tsid\tv2\n";
        let n2 = sess.import_cookies_from_str(netscape, None).unwrap();
        assert_eq!(n2, 1);

        // 3. Raw Cookie header
        let n3 = sess
            .import_cookies_from_str("Cookie: a=1; b=2", Some("example.gov.cn"))
            .unwrap();
        assert_eq!(n3, 2);

        let snap = sess.state_snapshot();
        assert_eq!(snap.cookies["gov.cn"]["sso"], "v1");
        assert_eq!(snap.cookies["zwfw.gov.cn"]["sid"], "v2");
        assert_eq!(snap.cookies["example.gov.cn"]["a"], "1");
        assert_eq!(snap.cookies["example.gov.cn"]["b"], "2");
    }
}
