//! `lchatgpt auth` —— 把「凭证从哪来、能不能发消息、什么时候过期」一次说清。
//!
//! 这是只读的本地体检：不联网，只按解析优先级把候选源逐个打开看，
//! 顺便告诉你**真正会被用的那一份**是哪一份、它能不能发消息。

use crate::auth;
use crate::store;
use lilyco::prelude::*;
use serde_json::{json, Value};

/// 查看凭证来源
#[derive(App)]
#[app(
    name = "auth",
    run = "run_auth",
    about = "Explain where the credential comes from without touching the network: lists every local credential source in resolution order (LCHATGPT_ACCESS_TOKEN env var, LCHATGPT_AUTH_FILE, lchatgpt's own store, ~/.codex/auth.json), marks which file exists, then reports the one that would actually be used with its source, capability (send / read-only / unknown - derived from the JWT scp claim, since only a token carrying api.connectors.invoke may POST /backend-api/codex/responses), expiry, whether it can refresh itself, and the store path that lchatgpt login writes. Returns { resolved, candidates[], store_path, hints }. Read-only (safety T0): only local files are inspected, no token is printed."
)]
pub struct Auth {}

fn run_auth(_app: &Auth, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    ctx.emit(Progress::Started {
        total: Some(2),
        message: Some("检查本地凭证".to_string()),
    });

    let candidates: Vec<Value> = auth::candidates()
        .into_iter()
        .map(|c| {
            json!({
                "source": c.source,
                "path": c.path.map(|p| p.display().to_string()),
                "available": c.available,
                "note": c.note,
            })
        })
        .collect();
    ctx.tick(1, Some(2), "列出候选源");

    let resolved = match auth::load() {
        Ok(c) => json!({
            "ok": true,
            "source": c.source,
            "capability": c.capability().tag(),
            "can_send": c.capability().send_capable(),
            "account_id": c.account_id,
            "expires_at": c.expires_at.map(crate::util::iso8601_from_epoch_secs),
            "expired": c.expired(),
            "refreshable": c.can_refresh(),
            "token": c.masked_token(),
        }),
        Err(e) => json!({ "ok": false, "error": format!("{e}") }),
    };

    let mut hints: Vec<String> = Vec::new();
    if resolved["ok"] == json!(false) {
        hints.push(
            "没有可用凭证：跑 lchatgpt login（自动挑 codex 登录态或浏览器登录态）".to_string(),
        );
    } else if resolved["can_send"] == json!(false) {
        hints.push(
            "这份凭证只能读；要发消息请 codex login 之后跑 lchatgpt login --from codex".to_string(),
        );
    }
    if resolved["expired"] == json!(true) && resolved["refreshable"] == json!(true) {
        hints
            .push("已过期但可续期：跑 lchatgpt login --from refresh（ask 也会自动续）".to_string());
    }
    if resolved["expired"] == json!(false)
        && resolved["refreshable"] == json!(false)
        && resolved["ok"] == json!(true)
    {
        hints.push(
            "浏览器登录态无法自动续期，过期后需要重新 lchatgpt login --from browser".to_string(),
        );
    }

    let result = json!({
        "resolved": resolved,
        "candidates": candidates,
        "store_path": store::store_path().map(|p| p.display().to_string()),
        "hints": hints,
    });

    ctx.tick(2, Some(2), "");
    if let Some(src) = result["resolved"]["source"].as_str() {
        ctx.log(
            LogLevel::Info,
            format!(
                "在用: {}（{}）",
                src,
                result["resolved"]["capability"].as_str().unwrap_or("?")
            ),
        );
    }
    for h in result["hints"].as_array().into_iter().flatten() {
        if let Some(h) = h.as_str() {
            ctx.log(LogLevel::Warn, h.to_string());
        }
    }
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// 一条候选都没有时也要给出可执行的建议，而不是空结果
    #[test]
    fn a_missing_credential_yields_a_hint() {
        let _env = crate::testenv::no_credentials("auth-missing");
        let (tx, _rx) = mpsc::channel();
        let v = run_auth(&Auth {}, &Context::new_test(tx)).expect("auth 本身不该失败");
        assert_eq!(v["resolved"]["ok"], json!(false));
        let hints = v["hints"].as_array().unwrap();
        assert!(!hints.is_empty(), "没有凭证时必须给建议: {v}");
        assert!(format!("{v}").contains("lchatgpt login"), "{v}");
    }

    /// 候选源必须按优先级排列，且带 available 标记
    #[test]
    fn candidates_are_listed_with_availability() {
        let _env = crate::testenv::scoped();
        std::env::remove_var("LCHATGPT_ACCESS_TOKEN");
        std::env::set_var(
            "LCHATGPT_STORE",
            "/tmp/lchatgpt-nonexistent-store/auth.json",
        );
        let (tx, _rx) = mpsc::channel();
        let v = run_auth(&Auth {}, &Context::new_test(tx)).unwrap();
        let list = v["candidates"].as_array().unwrap();
        assert!(list.len() >= 3, "{v}");
        assert!(list.iter().all(|c| c.get("available").is_some()), "{v}");
        assert!(v["store_path"].as_str().unwrap().contains("lchatgpt"));
    }
}
