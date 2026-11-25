//! 用 refresh_token 换一份新的 access_token —— 让「一份登录态用很久」这件事成立。
//!
//! 只有 codex 那条 OAuth 凭证带 refresh_token（浏览器登录态没有）。实测 `auth.openai.com`
//! 的 `refresh_token` 授权可用：
//!
//! `POST https://auth.openai.com/oauth/token`
//! `{"client_id":"app_EMoamEEZ73f0CkXaXp7hrann","grant_type":"refresh_token","refresh_token":"…","scope":"openid profile email"}`
//!
//! 回 `{access_token, id_token, expires_in, refresh_token, earliest_refresh_at}`。
//! 注意：返回的 refresh_token **会轮换**（旧的那份仍然可用，但新的是正统），
//! 所以换完之后必须落盘，否则下次只能拿旧的来赌 —— `ensure_fresh` 会写回仓库。
//!
//! 另一条被排除的路：自己跑 `/oauth/authorize` + PKCE 做登录。它对 codex 的
//! client_id 一律回 400 `Invalid authorize request`（curl 与真浏览器都试过），
//! 即我们**不能自己发起登录**，只能拿别人（codex CLI）已经换到手的 refresh_token 续期。

use crate::auth::{Credentials, CODEX_CLIENT_ID};
use crate::jwt;
use crate::store;
use lilyco::prelude::AppError;
use serde_json::{json, Value};
use std::time::Duration;

const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";

/// 换回来的一整套 token
#[derive(Debug)]
pub struct Refreshed {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_in: Option<i64>,
}

impl Refreshed {
    /// id_token 里的邮箱（没有就是 None）
    pub fn email(&self) -> Option<String> {
        let payload = jwt::payload(self.id_token.as_ref()?)?;
        payload
            .get("email")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// 新 token 的账号 id（JWT claim 里读，最可靠）
    pub fn account_id(&self) -> Option<String> {
        jwt::account_id(&self.access_token)
    }

    /// 新 token 的过期时刻（epoch 秒）。JWT 里没有 exp 时用 `expires_in` 折算。
    pub fn expires_at(&self) -> Option<i64> {
        jwt::expires_at(&self.access_token)
            .or_else(|| absolute_expiry(store::now_secs(), self.expires_in))
    }
}

/// `expires_in` 是「多少秒后过期」，落盘时换成绝对时刻更好用
pub fn absolute_expiry(now_secs: i64, expires_in: Option<i64>) -> Option<i64> {
    expires_in.map(|s| now_secs + s)
}

fn parse_response(text: &str) -> Result<Refreshed, AppError> {
    let v: Value = serde_json::from_str(text).map_err(|e| {
        AppError::InvalidInput(format!(
            "oauth/token 返回的不是 JSON: {e} —— {}",
            crate::util::clip(text, 200)
        ))
    })?;
    let access_token = v
        .get("access_token")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            let hint = v
                .pointer("/error/message")
                .or_else(|| v.get("error_description"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            AppError::InvalidInput(if hint.is_empty() {
                format!(
                    "oauth/token 没给出 access_token —— {}",
                    crate::util::clip(text, 200)
                )
            } else {
                format!("oauth/token 拒绝: {hint}")
            })
        })?;
    Ok(Refreshed {
        access_token,
        refresh_token: v
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        id_token: v
            .get("id_token")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        expires_in: v.get("expires_in").and_then(Value::as_i64),
    })
}

/// 真正去换。一次网络调用，约 300 ms。
pub fn refresh(refresh_token: &str) -> Result<Refreshed, AppError> {
    let rt = refresh_token.trim();
    if rt.is_empty() {
        return Err(AppError::InvalidInput(
            "没有 refresh_token 可换（浏览器登录态没有这一项，需要先 login --from codex）"
                .to_string(),
        ));
    }
    let body = json!({
        "client_id": CODEX_CLIENT_ID,
        "grant_type": "refresh_token",
        "refresh_token": rt,
        "scope": "openid profile email",
    })
    .to_string();
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    let res = agent
        .post(TOKEN_URL)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .send(body)
        .map_err(|e| AppError::Runtime(format!("换 token 请求失败: {e}")))?;
    let status = res.status();
    let mut res = res;
    let text = res
        .body_mut()
        .read_to_string()
        .map_err(|e| AppError::Runtime(format!("读 oauth/token 响应失败: {e}")))?;
    if !status.is_success() {
        return Err(AppError::InvalidInput(format!(
            "oauth/token → HTTP {status}: {}",
            crate::util::clip(&text, 300)
        )));
    }
    parse_response(&text)
}

/// 发消息前的一步：token 快过期就顺手换一份（换完写回仓库）。
///
/// 三种情况都原样返回，让调用方照旧去发请求：
/// - 还没过期；
/// - 没有 refresh_token（浏览器登录态就是这样，只能重新抓一次）；
/// - 续期失败（网络抖动不该让一次提问直接失败 —— 让后端用 401 说话更有信息量）。
pub fn ensure_fresh(creds: Credentials) -> Credentials {
    if !creds.expired() {
        return creds;
    }
    let rt = match creds.refresh_token.clone() {
        Some(r) if !r.trim().is_empty() => r,
        _ => return creds,
    };
    let r = match refresh(&rt) {
        Ok(r) => r,
        Err(_) => return creds,
    };
    let expires_at = r.expires_at();
    let account_id = r.account_id().or_else(|| creds.account_id.clone());
    let email = r.email();
    let next = Credentials {
        access_token: r.access_token,
        account_id,
        source: format!("refresh:{}", creds.source),
        refresh_token: r.refresh_token.or(Some(rt)),
        expires_at,
        store: creds.store,
    };
    // refresh_token 会轮换：不写回就等于把新的丢掉，下次只剩旧的能赌
    let entry = store::entry(
        &next.access_token,
        next.refresh_token.as_deref(),
        next.account_id.as_deref(),
        email.as_deref(),
        expires_at,
        &next.source,
    );
    let _ = store::save(&entry);
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_response_yields_the_new_tokens() {
        let r = parse_response(
            r#"{"access_token":"new-a","refresh_token":"new-r","id_token":"new-i","expires_in":864000}"#,
        )
        .unwrap();
        assert_eq!(r.access_token, "new-a");
        assert_eq!(r.refresh_token.as_deref(), Some("new-r"));
        assert_eq!(r.expires_in, Some(864_000));
    }

    /// 后端拒绝时必须把它的原话带出来，而不是一句「解析失败」
    #[test]
    fn a_rejected_grant_surfaces_the_backend_wording() {
        let err = parse_response(r#"{"error":{"message":"invalid_grant: refresh token revoked"}}"#)
            .expect_err("没有 access_token 就该报错");
        let msg = format!("{err}");
        assert!(msg.contains("invalid_grant"), "{msg}");
    }

    /// 没有 refresh_token 时不该发请求 —— 报错要说清该去干什么
    #[test]
    fn an_empty_refresh_token_is_refused_before_any_request() {
        let err = refresh("  ").expect_err("空 refresh_token 不能发请求");
        assert!(format!("{err}").contains("login --from codex"), "{err}");
    }

    #[test]
    fn relative_expiry_becomes_an_absolute_one() {
        assert_eq!(absolute_expiry(1_000, Some(60)), Some(1_060));
        assert_eq!(absolute_expiry(1_000, None), None);
    }

    /// 没过期就别多打一次网络：ensure_fresh 必须原样返回
    #[test]
    fn a_fresh_credential_is_left_alone() {
        let c = Credentials {
            access_token: "opaque".to_string(),
            account_id: None,
            source: "test".to_string(),
            refresh_token: Some("ref".to_string()),
            expires_at: None,
            store: None,
        };
        let out = ensure_fresh(c);
        assert_eq!(out.access_token, "opaque");
        assert_eq!(out.source, "test");
    }

    /// 过期但没有 refresh_token（浏览器登录态）也不能卡住：原样返回，让后端报 401
    #[test]
    fn an_expired_browser_credential_is_passed_through() {
        let c = Credentials {
            access_token: jwt::fake(&["openid"], store::now_secs() - 10),
            account_id: None,
            source: "browser:http://127.0.0.1:9229".to_string(),
            refresh_token: None,
            expires_at: Some(store::now_secs() - 10),
            store: None,
        };
        let out = ensure_fresh(c);
        assert_eq!(out.source, "browser:http://127.0.0.1:9229");
    }
}
