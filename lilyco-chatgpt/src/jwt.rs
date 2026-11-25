//! 只解开 JWT 的 payload，不验签 —— 目的是判断「这份 token 能干什么」。
//!
//! 实测两种 token 的差别就在 `scp` 上：
//! - codex 客户端 token（`codex login` 的产物）：含 `api.connectors.invoke`，
//!   能 `POST /backend-api/codex/responses`；
//! - 浏览器登录态 token（`/api/auth/session` 给的）：只有 `model.request` / `model.read`，
//!   读接口全通，但发消息一律 401 `{"detail":"Unauthorized"}`。
//!
//! 所以「只读凭证」不是猜的，是从 claim 里读出来的。

use crate::util::base64url_decode;
use serde_json::Value;

/// 只有带这个 scope 的 token 才能调 codex responses
pub const SEND_SCOPE: &str = "api.connectors.invoke";

/// 一份 token 的使用能力
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// 能读也能发（codex 客户端 token）
    Send,
    /// 只能读（浏览器登录态 token）
    ReadOnly,
    /// 解不开 claim（比如用户自己塞进来的不透明 token）——按「能发」处理，
    /// 免得把用户明确给的凭证擅自降级，真不能发时后端会自己报 401
    Unknown,
}

impl Capability {
    pub fn send_capable(self) -> bool {
        !matches!(self, Capability::ReadOnly)
    }

    pub fn tag(self) -> &'static str {
        match self {
            Capability::Send => "send",
            Capability::ReadOnly => "read-only",
            Capability::Unknown => "unknown",
        }
    }
}

/// 取出 JWT 的 payload（第二段）。解不开返回 None。
pub fn payload(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    parts.next()?;
    let body = parts.next()?;
    let bytes = base64url_decode(body)?;
    serde_json::from_slice(&bytes).ok()
}

/// `scp` claim：可能是数组，也可能是空格分隔的字符串
pub fn scopes(token: &str) -> Vec<String> {
    let Some(p) = payload(token) else {
        return Vec::new();
    };
    match p.get("scp") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => s.split_whitespace().map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

/// 这份 token 能不能发消息
pub fn capability(token: &str) -> Capability {
    let scopes = scopes(token);
    if scopes.is_empty() {
        return Capability::Unknown;
    }
    if scopes.iter().any(|s| s == SEND_SCOPE) {
        Capability::Send
    } else {
        Capability::ReadOnly
    }
}

/// `exp` claim（epoch 秒）
pub fn expires_at(token: &str) -> Option<i64> {
    payload(token)?.get("exp")?.as_i64()
}

/// ChatGPT 账号 id 藏在 `https://api.openai.com/auth` 这个命名 claim 里
pub fn account_id(token: &str) -> Option<String> {
    let p = payload(token)?;
    p.get("https://api.openai.com/auth")?
        .get("chatgpt_account_id")?
        .as_str()
        .map(str::to_string)
}

/// 测试 / 文档用：造一份只有 claim 的假 JWT
#[cfg(test)]
pub fn fake(scopes: &[&str], exp: i64) -> String {
    let header = "eyJhbGciOiJSUzI1NiJ9";
    let payload = serde_json::json!({"scp": scopes, "exp": exp});
    format!(
        "{header}.{}.sig",
        base64url_encode(payload.to_string().as_bytes())
    )
}

#[cfg(test)]
fn base64url_encode(raw: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in raw.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(T[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(T[n as usize & 63] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 实测的两种 scope 组合：codex 能发，浏览器只能读
    #[test]
    fn the_connectors_scope_is_what_grants_sending() {
        let codex = fake(
            &[
                "openid",
                "profile",
                "email",
                "offline_access",
                "api.connectors.invoke",
            ],
            1_796_383_114,
        );
        assert_eq!(capability(&codex), Capability::Send);
        assert!(capability(&codex).send_capable());

        let browser = fake(
            &["openid", "email", "model.request", "model.read"],
            1_796_383_114,
        );
        assert_eq!(capability(&browser), Capability::ReadOnly);
        assert!(!capability(&browser).send_capable());
    }

    /// 不透明 token（解不开 claim）不能被当成只读，否则用户自己给的凭证会被拦住
    #[test]
    fn an_opaque_token_is_assumed_to_be_usable() {
        assert_eq!(capability("not-a-jwt"), Capability::Unknown);
        assert!(capability("not-a-jwt").send_capable());
    }

    #[test]
    fn exp_and_account_id_are_readable() {
        let t = fake(&["openid"], 1_796_383_114);
        assert_eq!(expires_at(&t), Some(1_796_383_114));
        let p =
            serde_json::json!({"https://api.openai.com/auth": {"chatgpt_account_id": "acct-1"}});
        let t2 = format!("e30.{}.sig", base64url_encode(p.to_string().as_bytes()));
        assert_eq!(account_id(&t2).as_deref(), Some("acct-1"));
    }

    #[test]
    fn scopes_accept_both_array_and_space_separated_string() {
        let p = serde_json::json!({"scp": "openid api.connectors.invoke"});
        let t = format!("e30.{}.sig", base64url_encode(p.to_string().as_bytes()));
        assert_eq!(scopes(&t), vec!["openid", SEND_SCOPE]);
    }
}
