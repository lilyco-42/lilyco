//! `lchatgpt login` —— 把一份登录态收进自己的仓库（`store.rs`），之后 7 条命令都直接用它。
//!
//! 三种来源：
//! - `--from browser`：连上正在跑的 chromium（ego-lite 默认 9229），用它的 cookie 请求
//!   `/api/auth/session` 拿到 accessToken。好处是用户不用重新登录；代价是这份 token
//!   **只能读**（缺 `api.connectors.invoke`，发消息会被 401）。
//! - `--from codex`：把 `~/.codex/auth.json`（codex CLI 的登录态）收进仓库。
//!   这是唯一能既读又发、还能自动续期的一份。
//! - `--from refresh`：拿仓库里的 refresh_token 换一份新的 access_token 并写回
//!   （token 快过期时 `ask` 也会自动做这件事，这个子命令用于手动强制续期）。
//!
//! 仓库位置与权限见 `store.rs`（0600，只写自己的文件，绝不改 codex 的 auth.json）。

use crate::auth;
use crate::browser;
use crate::refresh;
use crate::store;
use lilyco::prelude::*;
use serde_json::{json, Value};

/// 登录 / 导入登录态
#[derive(App)]
#[app(
    name = "login",
    run = "run_login",
    safety = "t1",
    about = "Store a ChatGPT login state into lchatgpt's own credential store so the other commands stop depending on a hand-set env var: --from browser harvests the access token of an already-logged-in chromium over the DevTools Protocol (ego-lite's port 9229 by default; no hard dependency, it is the same pure-Rust websocket client the rest of the tool links), --from codex imports ~/.codex/auth.json (the only credential that can both read and send, and the only one that can refresh itself), --from refresh forces a refresh-token grant right now. Writes only lchatgpt's own store file (mode 0600) and returns { action, source, capability, can_send, account_id, email, expires_at, store_path, token, refreshable } with the token masked. Safety T1 because a browser login state is a real credential; MCP surfaces refuse it."
)]
pub struct Login {
    #[arg(
        about = "Where the login state comes from: browser / codex / refresh (empty means: the codex file if present, otherwise the browser)",
        default = ""
    )]
    from: String,
    #[arg(
        about = "CDP endpoint of the browser to talk to (default: $LCHATGPT_CDP, then ego-lite config, then 9229/9222/9223/9333)",
        default = ""
    )]
    cdp: String,
    #[arg(about = "Print the full access token instead of the masked form")]
    reveal: bool,
    #[arg(
        about = "Overwrite even when the new login state is weaker: a read-only browser token replacing a stored credential that can also send. Without this the store keeps the stronger one and the command warns."
    )]
    force: bool,
}

fn pick_from(from: &str) -> Result<String, AppError> {
    match from.trim() {
        "" => Ok("auto".to_string()),
        "browser" | "chrome" | "chromium" | "cdp" => Ok("browser".to_string()),
        "codex" => Ok("codex".to_string()),
        "refresh" => Ok("refresh".to_string()),
        other => Err(AppError::InvalidArg(format!(
            "不认识的 --from 值 `{other}` —— 只支持 browser / codex / refresh（留空 = 自动）"
        ))),
    }
}

fn credential_json(
    c: &auth::Credentials,
    email: Option<&str>,
    store_path: &str,
    action: &str,
    reveal: bool,
) -> Value {
    json!({
        "action": action,
        "source": c.source,
        "capability": c.capability().tag(),
        "can_send": c.capability().send_capable(),
        "account_id": c.account_id,
        "email": email,
        "expires_at": c.expires_at.map(crate::util::iso8601_from_epoch_secs),
        "store_path": store_path,
        "token": if reveal { json!(c.access_token) } else { json!(c.masked_token()) },
        "refreshable": c.can_refresh(),
    })
}

fn run_login(app: &Login, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let from = pick_from(&app.from)?;
    ctx.emit(Progress::Started {
        total: Some(3),
        message: Some(format!("从 {from} 取登录态")),
    });

    // auto：有 codex 的登录态就优先用它（唯一能发的），否则去问浏览器
    let from = if from == "auto" {
        match auth::default_auth_path() {
            Some(p) if p.is_file() => "codex".to_string(),
            _ => "browser".to_string(),
        }
    } else {
        from
    };
    ctx.log(LogLevel::Info, format!("登录态来源: {from}"));

    let (creds, email) = match from.as_str() {
        "browser" => {
            let s = browser::harvest(&app.cdp)?;
            ctx.tick(1, Some(3), "已从浏览器取到 token");
            let c = auth::Credentials {
                expires_at: s.expires_at,
                account_id: s.account_id.clone(),
                access_token: s.access_token.clone(),
                refresh_token: None,
                source: format!("browser:{}", s.cdp),
                store: None,
            };
            (c, s.email.clone())
        }
        "codex" => {
            let path = auth::default_auth_path().ok_or_else(|| {
                AppError::InvalidInput("拿不到 HOME，无法定位 ~/.codex/auth.json".to_string())
            })?;
            if !path.is_file() {
                return Err(AppError::InvalidInput(format!(
                    "没找到 {} —— 先跑一次 codex login，或用 --from browser 从浏览器抓",
                    path.display()
                )));
            }
            let c = auth::parse_file(&path)?;
            ctx.tick(1, Some(3), "已读取 codex 登录态");
            let email = codex_email(&path);
            (c, email)
        }
        _ => {
            // refresh：手动续期。用本地那份能续的
            let c = auth::load()?;
            let rt = c.refresh_token.clone().ok_or_else(|| {
                AppError::InvalidInput(
                    "这份凭证没有 refresh_token（浏览器登录态都是这样）。要能续期请先 lchatgpt login --from codex"
                        .to_string(),
                )
            })?;
            let r = refresh::refresh(&rt)?;
            ctx.tick(1, Some(3), "续期成功");
            let expires_at = r.expires_at();
            let account_id = r.account_id().or_else(|| c.account_id.clone());
            let email = r.email();
            let creds = auth::Credentials {
                access_token: r.access_token,
                account_id,
                source: format!("refresh:{}", c.source),
                refresh_token: r.refresh_token.or(Some(rt)),
                expires_at,
                store: None,
            };
            (creds, email)
        }
    };

    // 别用弱凭证盖掉强凭证：浏览器登录态只能读，如果仓库里已经躺着一份
    // 能发消息的（codex / refresh 来的），盖上去等于把「能提问」这件事弄坏了。
    {
        let next_can_send = creds.capability().send_capable();
        let stored_can_send = store::store_path()
            .and_then(|p| auth::parse_file(&p).ok())
            .map(|c| c.capability().send_capable())
            .unwrap_or(false);
        if !next_can_send && stored_can_send && !app.force {
            let kept = store::store_path()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            ctx.log(
                LogLevel::Warn,
                format!(
                    "新的登录态只能读，仓库里那份能发消息 —— 保留仓库里那份（{kept}）。\n  真要替换请加 --force"
                ),
            );
            let stored = auth::load()?;
            let result = credential_json(&stored, None, &kept, "kept", app.reveal);
            ctx.tick(3, Some(3), "");
            ctx.done(result.clone(), start.elapsed().as_millis() as u64);
            return Ok(result);
        }
    }

    // 落盘：形状兼容 store::entry，refresh_token 为 None 时不覆盖旧的
    let entry = store::entry(
        &creds.access_token,
        creds.refresh_token.as_deref(),
        creds.account_id.as_deref(),
        email.as_deref(),
        creds.expires_at,
        &creds.source,
    );
    let saved = store::save(&entry)?;
    ctx.tick(2, Some(3), "已写入凭证仓库");
    let saved_str = saved.display().to_string();

    let cap = creds.capability();
    let result = credential_json(&creds, email.as_deref(), &saved_str, &from, app.reveal);
    ctx.tick(3, Some(3), "");
    ctx.log(LogLevel::Info, format!("凭证已存入 {saved_str}"));
    ctx.log(
        LogLevel::Info,
        format!(
            "能力: {}（{}）",
            cap.tag(),
            if cap.send_capable() {
                "能读也能发消息"
            } else {
                "只能读；发消息请 codex login 之后跑 lchatgpt login --from codex"
            }
        ),
    );
    if let Some(e) = creds.expires_at {
        ctx.log(
            LogLevel::Info,
            format!("到期: {}", crate::util::iso8601_from_epoch_secs(e)),
        );
    }
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// codex 文件里没有单独存邮箱，但 id_token 里有；读不到就算了
fn codex_email(path: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let id = v.pointer("/tokens/id_token").and_then(Value::as_str)?;
    let payload = crate::jwt::payload(id)?;
    payload
        .get("email")
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn an_unknown_source_is_rejected_with_the_allowed_values() {
        let err = pick_from("twitter").expect_err("不认识的来源要报错");
        let msg = format!("{err}");
        assert!(msg.contains("browser"), "{msg}");
        assert!(msg.contains("codex"), "{msg}");
    }

    #[test]
    fn the_source_aliases_are_accepted() {
        assert_eq!(pick_from("").unwrap(), "auto");
        assert_eq!(pick_from("chrome").unwrap(), "browser");
        assert_eq!(pick_from(" refresh ").unwrap(), "refresh");
    }

    /// refresh 但没有 refresh_token 时，必须说清「先 login --from codex」
    #[test]
    fn refreshing_without_a_refresh_token_explains_the_fix() {
        let _env = crate::testenv::no_credentials("login-refresh");
        std::env::set_var("LCHATGPT_ACCESS_TOKEN", "opaque-token");
        let (tx, _rx) = mpsc::channel();
        let app = Login {
            from: "refresh".to_string(),
            cdp: String::new(),
            reveal: false,
            force: false,
        };
        let err = run_login(&app, &Context::new_test(tx)).expect_err("没有 refresh_token 不能续期");
        assert!(format!("{err}").contains("login --from codex"), "{err}");
    }
}
