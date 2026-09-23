//! 安全闸门：Web 端只能被本机页面驱动。
//!
//! 防御 DNS rebinding / CSRF：
//! 1. 所有请求的 Host 必须是回环地址（rebinding 时 Host 仍是攻击者域名，会被拒绝）
//! 2. 受保护 POST（/run /upload /cancel /pick）的 Origin 若非回环地址则拒绝
//! 3. 受保护 POST 必须携带本次启动随机生成的 `X-Lilyco-Token`

use std::sync::Arc;

use axum::extract::{Request as AxumRequest, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::state::AppState;

/// 受保护 POST 必须带的请求头（页面从 `<meta name="lilyco-token">` 取值）
pub const TOKEN_HEADER: &str = "X-Lilyco-Token";

/// 需要令牌 + 回环 Origin 校验的 POST 端点
const PROTECTED_POST: &[&str] = &["/run", "/upload", "/cancel", "/pick"];

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    let host = if let Some(rest) = host.strip_prefix('[') {
        // IPv6 bracket notation: "[::1]:8080"
        rest.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))?;
    let host = rest.split(['/', ':', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

pub(crate) async fn security_mw(
    State(state): State<Arc<AppState>>,
    request: AxumRequest,
    next: Next,
) -> Response {
    let headers = request.headers();

    let host = header_str(headers, "host").unwrap_or_default();
    if !is_loopback_host(host) {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    let path = request.uri().path();
    if request.method() == Method::POST && PROTECTED_POST.contains(&path) {
        if let Some(origin) = header_str(headers, "origin") {
            let ok = origin_host(origin).map(is_loopback_host).unwrap_or(false);
            if !ok {
                return (StatusCode::FORBIDDEN, "Forbidden").into_response();
            }
        }
        let token = header_str(headers, TOKEN_HEADER).unwrap_or_default();
        if !constant_time_eq(token, &state.token) {
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/pick`（原生文件选择器）必须和 /run /upload /cancel 同一道闸：
    /// 它会开一个窗口，不能被别的页面远程刷
    #[test]
    fn pick_endpoint_is_token_gated() {
        assert!(
            PROTECTED_POST.contains(&"/pick"),
            "/pick 漏在闸外：{PROTECTED_POST:?}"
        );
    }
}
