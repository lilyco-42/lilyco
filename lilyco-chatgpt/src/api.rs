//! chatgpt.com 后端 API 的薄封装：一个 ureq Agent + 凭证注入 + 错误翻译。
//!
//! 只碰这几个入口：
//! - 只读 REST（`GET /backend-api/me|conversations|conversation/{id}|models|codex/usage`）
//! - `POST /backend-api/codex/responses`（SSE，codex CLI 用的那条，**花 codex 额度**）
//! - `POST /backend-api/conversation/init`（body `{}`，读 chat 侧的 feature 额度）
//!
//! **不碰** `POST /backend-api/conversation`（网页聊天发消息）：那条路要求 turnstile +
//! sentinel 的设备指纹，纯 HTTP 调用一律 403「Unusual activity has been detected from your
//! device」。网页聊天由 `chat.rs` 驱动真浏览器去发（那就是默认的 `ask` 路径），
//! 这里只负责把回复用 REST 读回来。

use crate::auth::Credentials;
use lilyco::prelude::AppError;
use serde_json::Value;
use std::time::Duration;

/// 后端根路径。所有命令都拼在这后面。
pub const BASE: &str = "https://chatgpt.com/backend-api";

/// 浏览器 UA：后端对 `ureq/3.x` 这种裸 UA 更容易判异常，带一个常见 UA 更稳。
/// （真正的反自动化判定在 sentinel / PoW 那层，这里只是别主动撞枪口。）
pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// 单张图 / 附件下载的字节上限（实测生成的 PNG 约 1 MB，留足余量）
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;

/// 一次 API 调用
pub struct Api {
    agent: ureq::Agent,
    creds: Credentials,
}

impl Api {
    /// 普通请求用的 agent：整条请求 30s 预算；非 2xx 不抛错 —— 自己看 status 才能把
    /// 后端给的 `{"detail": …}` / `{"error": …}` 原样翻译成人话。
    pub fn new(creds: Credentials) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        Self { agent, creds }
    }

    /// 流式请求用的 agent：SSE 可能持续几分钟，所以只设**单次读**超时，
    /// 不设整体预算（整体预算会在长回答中途把连接掐掉）。
    pub fn streaming(creds: Credentials) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_body(Some(Duration::from_secs(180)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        Self { agent, creds }
    }

    /// 凭证出处，写进结果里方便用户判断读的是哪份登录态
    pub fn credential_source(&self) -> &str {
        &self.creds.source
    }

    /// 打码后的 token
    pub fn masked_token(&self) -> String {
        self.creds.masked_token()
    }

    pub fn account_id(&self) -> Option<&str> {
        self.creds.account_id.as_deref()
    }

    /// 给请求挂上认证头
    fn auth_headers<B>(&self, req: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        let req = req
            .header(
                "authorization",
                format!("Bearer {}", self.creds.access_token),
            )
            .header("user-agent", USER_AGENT)
            .header("accept", "application/json");
        match &self.creds.account_id {
            Some(acct) => req.header("chatgpt-account-id", acct.clone()),
            None => req,
        }
    }

    /// GET 一个接口，返回 (HTTP 状态, 原始响应体)。不把非 2xx 当错误 ——
    /// 重试策略要自己看状态码（429 是限流，值得等一会儿再试）。
    fn get_raw(&self, path: &str, query: &[(String, String)]) -> Result<(u16, String), AppError> {
        let url = format!("{BASE}{path}");
        let mut req = self.auth_headers(self.agent.get(&url));
        for (k, v) in query {
            req = req.query(k.as_str(), v.as_str());
        }
        let res = req
            .call()
            .map_err(|e| AppError::Runtime(format!("请求 GET {path} 失败: {e}")))?;
        let status = res.status().as_u16();
        let body = res
            .into_body()
            .read_to_string()
            .map_err(|e| AppError::Runtime(format!("读响应失败 GET {path}: {e}")))?;
        Ok((status, body))
    }

    /// GET 一个 JSON 接口。`path` 以 `/` 开头，拼在 BASE 后面。
    pub fn get_json(&self, path: &str, query: &[(String, String)]) -> Result<Value, AppError> {
        let (status, body) = self.get_raw(path, query)?;
        if !(200..300).contains(&status) {
            return Err(http_error("GET", path, status, &body));
        }
        serde_json::from_str(&body)
            .map_err(|e| AppError::Runtime(format!("后端返回的不是 JSON（GET {path}）: {e}")))
    }

    /// 同 `get_json`，但 429（限流）会退避重试几次再放弃。
    ///
    /// 轮询会话正文时最容易被限流：一次生图要盯着一两分钟，一秒钟一个 GET 很快就被
    /// 后端判成刷接口（实测连打 123 次后整个 `/conversation/{id}` 报 429 约半分钟）。
    /// 限流不是失败，等一等就过去了。
    pub fn get_json_retry(
        &self,
        path: &str,
        query: &[(String, String)],
        attempts: u32,
    ) -> Result<Value, AppError> {
        let mut backoff = Duration::from_millis(800);
        for attempt in 0..attempts.max(1) {
            let (status, body) = self.get_raw(path, query)?;
            if (200..300).contains(&status) {
                return serde_json::from_str(&body).map_err(|e| {
                    AppError::Runtime(format!("后端返回的不是 JSON（GET {path}）: {e}"))
                });
            }
            // 429 与 5xx 值得再试；401/403/404 再试也是一样的结果
            let retryable = retryable_status(status);
            if !retryable || attempt + 1 == attempts.max(1) {
                return Err(http_error("GET", path, status, &body));
            }
            std::thread::sleep(backoff);
            backoff = (backoff * 2).min(Duration::from_secs(8));
        }
        unreachable!("循环里一定会 return")
    }

    /// POST 一个 JSON 请求体并**不读完响应**：SSE 需要边到边读，所以 2xx 时把原始
    /// Response 交回去，由调用方自己按行解析；非 2xx 才在这里读 body 翻译成中文错误。
    pub fn post_stream(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<ureq::http::Response<ureq::Body>, AppError> {
        let url = format!("{BASE}{path}");
        let payload = serde_json::to_string(body)
            .map_err(|e| AppError::Runtime(format!("序列化请求体失败: {e}")))?;
        let mut res = self
            .auth_headers(self.agent.post(&url))
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .header("origin", "https://chatgpt.com")
            .header("referer", "https://chatgpt.com/")
            .send(payload)
            .map_err(|e| AppError::Runtime(format!("请求 POST {path} 失败: {e}")))?;
        let status = res.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(res);
        }
        let raw = res
            .body_mut()
            .read_to_string()
            .map_err(|e| AppError::Runtime(format!("读响应失败 POST {path}: {e}")))?;
        Err(http_error("POST", path, status, &raw))
    }

    /// POST 一个 JSON 请求体并读回 JSON 响应。
    ///
    /// 目前只有 `POST /backend-api/conversation/init` 用得上（读 chat 侧的 feature 额度）：
    /// 它要 body `{}`，返回带 `limits_progress` 的 JSON，不是流。
    pub fn post_json(&self, path: &str, body: &Value) -> Result<Value, AppError> {
        let url = format!("{BASE}{path}");
        let payload = serde_json::to_string(body)
            .map_err(|e| AppError::Runtime(format!("序列化请求体失败: {e}")))?;
        let mut res = self
            .auth_headers(self.agent.post(&url))
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .header("origin", "https://chatgpt.com")
            .header("referer", "https://chatgpt.com/")
            .send(payload)
            .map_err(|e| AppError::Runtime(format!("请求 POST {path} 失败: {e}")))?;
        let status = res.status().as_u16();
        let raw = res
            .body_mut()
            .read_to_string()
            .map_err(|e| AppError::Runtime(format!("读响应失败 POST {path}: {e}")))?;
        if !(200..300).contains(&status) {
            return Err(http_error("POST", path, status, &raw));
        }
        serde_json::from_str(&raw)
            .map_err(|e| AppError::Runtime(format!("后端返回的不是 JSON（POST {path}）: {e}")))
    }

    /// GET 一个**绝对** URL 并读回原始字节（生图下载用）。
    ///
    /// 与 `get_json` 的区别：不拼 `BASE`（`download_url` 本身就是完整地址），
    /// 返回字节而不是 JSON。认证头照挂 —— 少了 `chatgpt-account-id` 后端会回
    /// 403 `File stream access denied.`。
    pub fn get_bytes(&self, url: &str) -> Result<Vec<u8>, AppError> {
        let mut res = self
            .auth_headers(self.agent.get(url))
            .call()
            .map_err(|e| AppError::Runtime(format!("请求 GET {url} 失败: {e}")))?;
        let status = res.status().as_u16();
        if !(200..300).contains(&status) {
            let raw = res.body_mut().read_to_string().unwrap_or_default();
            return Err(http_error("GET", url, status, &raw));
        }
        res.body_mut()
            .with_config()
            .limit(MAX_DOWNLOAD_BYTES)
            .read_to_vec()
            .map_err(|e| AppError::Runtime(format!("读取 GET {url} 的响应体失败: {e}")))
    }

    /// 下载一张会话里生成的图。
    ///
    /// 两步（实测）：先 `GET /files/{id}/download` 拿 `{download_url}`（一个带签名的
    /// `/backend-api/estuary/content?...` 地址），再带同一套认证头 GET 那个地址拿 PNG。
    /// 直接用 `sediment://file_…` 里的 id 拼别的端点都不行（403 / 404）。
    pub fn download_file(&self, file_id: &str) -> Result<Vec<u8>, AppError> {
        let meta = self.get_json(&format!("/files/{file_id}/download"), &[])?;
        let url = meta
            .get("download_url")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::Runtime(format!(
                    "后端没给出 {file_id} 的 download_url（返回: {meta}）"
                ))
            })?;
        self.get_bytes(url).map_err(|e| match e {
            // 换成一句能看出这是「下载图片」失败的话
            AppError::Runtime(m) => AppError::Runtime(format!("下载图片 {file_id} 失败: {m}")),
            other => other,
        })
    }
}

/// 这个状态码值不值得等一会儿再试：429（限流）与 5xx 值得，401/403/404 再试也一样。
pub fn retryable_status(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// 把后端的非 2xx 响应翻译成一句能照着做的中文错误。
///
/// 后端两种错误体：`{"detail": "…"}`（REST）与 `{"error": {"type","message",…}}`（codex）。
/// 429 的 `usage_limit_reached` 会把重置时刻算成人话 —— 这是用户最常撞到的一种。
pub fn http_error(method: &str, path: &str, status: u16, body: &str) -> AppError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let detail = parsed
        .as_ref()
        .and_then(|v| v.get("detail"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let err_obj = parsed.as_ref().and_then(|v| v.get("error"));
    let err_type = err_obj
        .and_then(|e| e.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let err_msg = err_obj
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    let hint = match status {
        401 => Some(
            "凭证无效或已过期：重新跑一次 `codex login`，或设置 LCHATGPT_ACCESS_TOKEN"
                .to_string(),
        ),
        403 => Some(
            "被 chatgpt.com 的反自动化拦住了。网页聊天（POST /backend-api/conversation）必须由真浏览器过 turnstile —— 那正是 ask 的默认路径（chat 额度）；纯 HTTP 打这条接口一定会 403"
                .to_string(),
        ),
        429 if err_type == "usage_limit_reached" => {
            let reset_at = err_obj
                .and_then(|e| e.get("resets_at"))
                .and_then(Value::as_i64)
                .map(crate::util::iso8601_from_epoch_secs);
            let in_secs = err_obj
                .and_then(|e| e.get("resets_in_seconds"))
                .and_then(Value::as_i64);
            let plan = err_obj
                .and_then(|e| e.get("plan_type"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let when = match (reset_at, in_secs) {
                (Some(iso), Some(s)) => format!("约 {s} 秒后（{iso}）恢复"),
                (Some(iso), None) => format!("{iso} 恢复"),
                _ => "稍后恢复".to_string(),
            };
            Some(format!(
                "ChatGPT 额度已用尽（plan_type: {plan}），{when}。只读命令（whoami/usage/models/list/show-conversation/search）不受影响；用 `lchatgpt usage` 看配额详情"
            ))
        }
        _ => None,
    };

    let what = detail.or(err_msg).unwrap_or_else(|| {
        let (snippet, _) = crate::util::take_chars(body.trim(), 300);
        if snippet.is_empty() {
            "（空响应体）".to_string()
        } else {
            snippet
        }
    });
    let mut msg = format!("{method} {path} → HTTP {status}: {what}");
    if let Some(h) = hint {
        msg.push_str("\n  ");
        msg.push_str(&h);
    }
    AppError::Runtime(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 后端的原话要透出，同时要带「怎么办」—— 401 只报一句英文，用户只能去翻源码
    #[test]
    fn a_plain_detail_error_surfaces_the_backend_wording_and_a_hint() {
        let e = http_error(
            "GET",
            "/me",
            401,
            &json!({"detail": "Could not parse your authentication token."}).to_string(),
        );
        let msg = format!("{e}");
        assert!(msg.contains("HTTP 401"), "{msg}");
        assert!(
            msg.contains("Could not parse your authentication token"),
            "{msg}"
        );
        assert!(msg.contains("codex login"), "{msg}");
    }

    /// 额度用尽是最常撞的失败：要给出计划类型、重置时刻，并说明只读命令还能用
    #[test]
    fn an_exhausted_quota_is_explained_with_the_reset_time() {
        let body = json!({"error": {
            "type": "usage_limit_reached",
            "message": "The usage limit has been reached",
            "plan_type": "plus",
            "resets_at": 1_795_608_075i64,
            "resets_in_seconds": 2168i64
        }});
        let msg = format!(
            "{}",
            http_error("POST", "/codex/responses", 429, &body.to_string())
        );
        assert!(msg.contains("额度已用尽"), "{msg}");
        assert!(msg.contains("plus"), "{msg}");
        assert!(msg.contains("2026-11-25T12:01:15Z"), "{msg}");
        assert!(msg.contains("2168"), "{msg}");
        assert!(msg.contains("lchatgpt usage"), "{msg}");
    }

    /// 认不出的错误体要回显，但不能把几 MB 的 body 全打进终端
    #[test]
    fn an_unknown_error_body_is_quoted_but_bounded() {
        let long = "x".repeat(2000);
        let msg = format!("{}", http_error("GET", "/me", 500, &long));
        assert!(msg.contains("HTTP 500"), "{msg}");
        assert!(
            msg.len() < 1200,
            "错误信息不该把整个 body 打印出来: {}",
            msg.len()
        );
    }

    /// 只有限流与服务端抖动值得重试；认证/权限/不存在再试一百次也一样
    #[test]
    fn only_rate_limits_and_server_hiccups_are_retried() {
        assert!(retryable_status(429));
        assert!(retryable_status(500));
        assert!(retryable_status(503));
        assert!(!retryable_status(401));
        assert!(!retryable_status(403));
        assert!(!retryable_status(404));
        assert!(!retryable_status(200));
    }
}
