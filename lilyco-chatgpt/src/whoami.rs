//! `lchatgpt whoami` —— 这份凭证对应哪个账号、走的是哪条认证路径。

use crate::api::Api;
use crate::auth;
use lilyco::prelude::*;
use serde_json::{json, Value};

/// 问 chatgpt.com「我是谁」，并说明凭证是从哪读来的
#[derive(App)]
#[app(
    name = "whoami",
    run = "run_whoami",
    about = "Show which ChatGPT account the current credential belongs to: calls GET /backend-api/me and returns { email, name, user_id, country, region, plan_type, account_id, credential_source, token } where token is masked (first 6 + … + last 4) and credential_source names the env var or file the token came from. Also reports plan_type from the codex usage endpoint so you can tell whether sending is currently allowed. Read-only (safety T0); the access token is never printed and nothing is modified."
)]
pub struct Whoami {}

fn run_whoami(_app: &Whoami, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let creds = auth::load()?;
    let api = Api::new(creds);
    ctx.emit(Progress::Started {
        total: Some(2),
        message: Some("查询账号".to_string()),
    });

    let me = api.get_json("/me", &[])?;
    let source = api.credential_source().to_string();
    let masked = api.masked_token();
    let account_id = api.account_id().map(|s| s.to_string());
    ctx.tick(1, Some(2), "查询配额");

    // 配额接口顺带给出 plan_type：只读失败不该让 whoami 整体失败，所以这里降级成 null
    let plan_type = api.get_json("/codex/usage", &[]).ok().and_then(|u| {
        u.get("plan_type")
            .and_then(Value::as_str)
            .map(str::to_string)
    });

    let field = |k: &str| -> String { me.get(k).and_then(Value::as_str).unwrap_or("").to_string() };
    let result = json!({
        "email": field("email"),
        "name": field("name"),
        "user_id": field("id"),
        "country": field("country"),
        "region": field("region"),
        "plan_type": plan_type,
        "account_id": account_id,
        "credential_source": source,
        "token": masked,
    });
    ctx.tick(2, Some(2), "");
    ctx.log(LogLevel::Info, format!("凭证来源: {source}"));
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// 没有凭证时必须给「去登录」的指引，而不是一句 IO 错误
    #[test]
    fn a_missing_credential_points_at_codex_login() {
        let _env = crate::testenv::no_credentials("whoami-missing");
        let (tx, _rx) = mpsc::channel();
        let err = run_whoami(&Whoami {}, &Context::new_test(tx)).expect_err("没有凭证要报错");
        let msg = format!("{err}");
        assert!(msg.contains("codex login"), "{msg}");
    }
}
