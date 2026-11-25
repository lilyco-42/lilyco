//! 凭证从哪来 —— 三级本地回落 + 一条可选的浏览器登录态抓取。
//!
//! 解析顺序（先命中先用）：
//! 1. `LCHATGPT_ACCESS_TOKEN` 环境变量（+ 可选 `LCHATGPT_ACCOUNT_ID`）—— CI / 临时借用
//! 2. `LCHATGPT_AUTH_FILE` 指定的 JSON 文件
//! 3. **lchatgpt 自己的仓库**（`store.rs`，`lchatgpt login` 写进去的那份）
//! 4. `~/.codex/auth.json`（codex CLI 用 ChatGPT 账号登录后留下的那份）
//!
//! 两条来源的能力不一样，差别写在 `jwt::Capability` 里：
//! - codex token（含 `api.connectors.invoke`）：能读也能 `POST /backend-api/codex/responses`；
//! - 浏览器登录态 token（只有 `model.request/read`）：只能读，发消息一律 401。
//!
//! 所以 `ask` 用 `load_for_send()`：优先挑一份**能发**的凭证；只剩只读凭证时明确报错，
//! 而不是让用户看一句后端的 `{"detail":"Unauthorized"}`。

use crate::jwt::{self, Capability};
use crate::store;
use lilyco::prelude::AppError;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// codex CLI 的 OAuth client id：只有它签出来的 token 带 `api.connectors.invoke`
pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

/// 一次调用要用的凭证。token 只在这里出现，任何输出都会被打码。
#[derive(Clone, Debug)]
pub struct Credentials {
    pub access_token: String,
    pub account_id: Option<String>,
    /// 凭证出处（写进结果里，方便用户判断「它读的是哪份登录态」）
    pub source: String,
    /// 有 refresh_token 才能在过期后自己续命（浏览器登录态没有这一项）
    pub refresh_token: Option<String>,
    /// JWT 的 `exp`（epoch 秒）；不透明 token 为 None
    pub expires_at: Option<i64>,
    /// 这份凭证落在哪个文件里 —— 续期成功后写回去的就是它
    pub store: Option<PathBuf>,
}

impl Credentials {
    /// 打码后的 token，用于人读的输出
    pub fn masked_token(&self) -> String {
        let t = &self.access_token;
        if t.chars().count() <= 12 {
            "***".to_string()
        } else {
            let head: String = t.chars().take(6).collect();
            let tail: String = t.chars().skip(t.chars().count() - 4).collect();
            format!("{head}…{tail}")
        }
    }

    /// 这份 token 能读还是能发
    pub fn capability(&self) -> Capability {
        jwt::capability(&self.access_token)
    }

    /// 是否已经过期（留 60 秒余量，免得卡在边界上被后端 401）
    pub fn expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => exp <= store::now_secs() + 60,
            None => false,
        }
    }

    pub fn can_refresh(&self) -> bool {
        self.refresh_token
            .as_deref()
            .map(|r| !r.trim().is_empty())
            .unwrap_or(false)
    }
}

/// 一个候选凭证源（给 `lchatgpt auth` 展示用，不联网）
pub struct Candidate {
    pub source: String,
    pub path: Option<PathBuf>,
    pub available: bool,
    pub note: String,
}

/// 默认凭证文件：`~/.codex/auth.json`
pub fn default_auth_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex").join("auth.json"))
}

/// 本地所有候选源，按解析优先级排列（不含网络抓取 —— 那是 `login` 的事）
pub fn candidates() -> Vec<Candidate> {
    let mut out = Vec::new();
    let env_token = std::env::var("LCHATGPT_ACCESS_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty());
    out.push(Candidate {
        source: "env:LCHATGPT_ACCESS_TOKEN".to_string(),
        path: None,
        available: env_token.is_some(),
        note: "环境变量直接给的 token（CI / 临时借用）".to_string(),
    });
    if let Ok(p) = std::env::var("LCHATGPT_AUTH_FILE") {
        if !p.trim().is_empty() {
            let path = PathBuf::from(p.trim());
            out.push(Candidate {
                source: format!("file:{}", path.display()),
                available: path.is_file(),
                path: Some(path),
                note: "LCHATGPT_AUTH_FILE 指定的凭证文件".to_string(),
            });
        }
    }
    if let Some(path) = store::store_path() {
        let available = store::read_at(&path).is_some();
        out.push(Candidate {
            source: format!("store:{}", path.display()),
            available,
            path: Some(path),
            note: "lchatgpt 自己的仓库（lchatgpt login 写入，可自动续期）".to_string(),
        });
    }
    if let Some(path) = default_auth_path() {
        let available = path.is_file();
        out.push(Candidate {
            source: format!("file:{}", path.display()),
            available,
            path: Some(path),
            note: "codex CLI 的登录态（能读也能发，最稳）".to_string(),
        });
    }
    out
}

/// 从一个 JSON 文件里抽出凭证。这是纯函数：文件长什么样、能干什么都由它决定。
///
/// 兼容两种形状：codex 的 `{tokens:{access_token,refresh_token,…}}` 与
/// lchatgpt 仓库的 `{access_token,refresh_token,…}`，省得用户记两套格式。
pub fn parse_file(path: &Path) -> Result<Credentials, AppError> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        AppError::InvalidInput(format!(
            "读凭证失败 {}: {e} —— 先跑一次 `codex login`（ChatGPT 账号登录）、`lchatgpt login`（用浏览器登录态），或设置 LCHATGPT_ACCESS_TOKEN",
            path.display()
        ))
    })?;
    let json: Value = serde_json::from_str(&raw).map_err(|e| {
        AppError::InvalidInput(format!("凭证不是合法 JSON {}: {e}", path.display()))
    })?;
    let pick = |ptrs: [&str; 2]| -> Option<String> {
        ptrs.iter()
            .find_map(|p| json.pointer(p).and_then(Value::as_str))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let access_token = pick(["/tokens/access_token", "/access_token"]).ok_or_else(|| {
        AppError::InvalidInput(format!(
            "{} 里没有 access_token —— 这份文件可能是 API key 模式或已失效，重新登录一次",
            path.display()
        ))
    })?;
    let refresh_token = pick(["/tokens/refresh_token", "/refresh_token"]);
    let account_id =
        pick(["/tokens/account_id", "/account_id"]).or_else(|| jwt::account_id(&access_token));
    Ok(Credentials {
        expires_at: jwt::expires_at(&access_token),
        access_token,
        account_id,
        refresh_token,
        source: format!("file:{}", path.display()),
        store: (path == store::store_path().unwrap_or_default()).then(|| path.to_path_buf()),
    })
}

/// 环境变量里那份凭证
fn from_env() -> Option<Credentials> {
    let token = std::env::var("LCHATGPT_ACCESS_TOKEN")
        .ok()?
        .trim()
        .to_string();
    if token.is_empty() {
        return None;
    }
    let account_id = std::env::var("LCHATGPT_ACCOUNT_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| jwt::account_id(&token));
    Some(Credentials {
        expires_at: jwt::expires_at(&token),
        access_token: token,
        account_id,
        refresh_token: None,
        source: "env:LCHATGPT_ACCESS_TOKEN".to_string(),
        store: None,
    })
}

/// 本地凭证，按优先级取第一份可用的。不联网。
pub fn load() -> Result<Credentials, AppError> {
    if let Some(c) = from_env() {
        return Ok(c);
    }
    let mut tried: Vec<String> = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("LCHATGPT_AUTH_FILE") {
        if !p.trim().is_empty() {
            paths.push(PathBuf::from(p.trim()));
        }
    }
    if let Some(p) = store::store_path() {
        paths.push(p);
    }
    if let Some(p) = default_auth_path() {
        paths.push(p);
    }
    for path in paths {
        match parse_file(&path) {
            Ok(c) => return Ok(c),
            Err(e) => tried.push(format!("  - {e}")),
        }
    }
    let mut msg = String::from(
        "拿不到凭证。三条路随便挑一条：
  1. `codex login`（推荐：能读也能发）
  2. `lchatgpt login`（用已在浏览器里登录的账号抓登录态，只读）
  3. 设置 LCHATGPT_ACCESS_TOKEN",
    );
    if !tried.is_empty() {
        msg.push_str(
            "
已经找过的位置：
",
        );
        msg.push_str(&tried.join(
            "
",
        ));
    }
    Err(AppError::InvalidInput(msg))
}

/// 发消息专用：优先挑一份**能发**的凭证。
///
/// 只读凭证（浏览器登录态）不能用在这里 —— 与其让后端回一句 401，不如现在就说清原因。
pub fn load_for_send() -> Result<Credentials, AppError> {
    let first = load()?;
    if first.capability().send_capable() {
        return Ok(first);
    }
    // 手上这份只能读，看看别处有没有能发的（比如 codex auth.json 还在）
    for path in [store::store_path(), default_auth_path()]
        .into_iter()
        .flatten()
    {
        if let Ok(c) = parse_file(&path) {
            if c.capability().send_capable() && !c.expired() {
                return Ok(c);
            }
        }
    }
    Err(AppError::InvalidInput(format!(
        "当前凭证只能读不能发（{}，source: {}）。发消息需要 codex 那条 OAuth 凭证：
  1. `codex login` 之后跑 `lchatgpt login --from codex` 把它收进仓库（之后可自动续期）
  2. 或用 LCHATGPT_ACCESS_TOKEN 直接给一份带 api.connectors.invoke 的 token
只读命令（whoami/usage/models/list/show-conversation/search）用现在这份就够了",
        first.capability().tag(),
        first.source
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwt;

    fn creds(token: &str) -> Credentials {
        Credentials {
            access_token: token.to_string(),
            account_id: None,
            source: "test".to_string(),
            refresh_token: None,
            // 过期与否只看 token 自己的 exp —— 跟真跑时的行为保持一致
            expires_at: jwt::expires_at(token),
            store: None,
        }
    }

    #[test]
    fn a_short_token_is_fully_masked() {
        assert_eq!(creds("abc").masked_token(), "***");
    }

    #[test]
    fn a_long_token_keeps_only_the_ends() {
        let c = creds("eyJhbGciOiJSUzI1NiIsInR5cCI6.deadbeef.payload");
        let m = c.masked_token();
        assert!(m.starts_with("eyJhbG"), "{m}");
        assert!(m.ends_with("load"), "{m}");
        assert!(!m.contains("deadbeef"), "{m}");
    }

    /// 环境变量优先于文件：CI 里不该去读某台机器的登录态
    #[test]
    fn the_env_token_wins_over_the_file() {
        let _env = crate::testenv::scoped();
        std::env::set_var("LCHATGPT_ACCESS_TOKEN", "from-env");
        std::env::set_var("LCHATGPT_ACCOUNT_ID", "acct-env");
        let c = load().expect("环境变量在场就应成功");
        std::env::remove_var("LCHATGPT_ACCESS_TOKEN");
        std::env::remove_var("LCHATGPT_ACCOUNT_ID");
        assert_eq!(c.access_token, "from-env");
        assert_eq!(c.account_id.as_deref(), Some("acct-env"));
        assert_eq!(c.source, "env:LCHATGPT_ACCESS_TOKEN");
    }

    /// codex 那份文件的形状（tokens.*）必须能读，refresh_token 也要一起带出来
    #[test]
    fn a_codex_shaped_file_yields_a_refreshable_credential() {
        let path = std::env::temp_dir().join("lchatgpt-codex-shape.json");
        std::fs::write(
            &path,
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"tok-a","refresh_token":"ref-a","account_id":"acct-a"}}"#,
        )
        .unwrap();
        let c = parse_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(c.access_token, "tok-a");
        assert_eq!(c.refresh_token.as_deref(), Some("ref-a"));
        assert_eq!(c.account_id.as_deref(), Some("acct-a"));
        assert!(c.can_refresh());
    }

    /// 仓库那份文件的形状（顶层字段）也要能读 —— 用户不该背两套格式
    #[test]
    fn a_store_shaped_file_is_read_the_same_way() {
        let path = std::env::temp_dir().join("lchatgpt-store-shape.json");
        std::fs::write(&path, r#"{"access_token":"tok-s","refresh_token":"ref-s"}"#).unwrap();
        let c = parse_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(c.access_token, "tok-s");
        assert_eq!(c.refresh_token.as_deref(), Some("ref-s"));
    }

    /// 过期的凭证要能被识别出来（用 JWT 的 exp，不看文件里的备注）
    #[test]
    fn an_expired_token_is_detected() {
        let old = jwt::fake(&["openid"], store::now_secs() - 10);
        let c = creds(&old);
        assert!(c.expired());
        let fresh = jwt::fake(&["openid"], store::now_secs() + 3_600);
        assert!(!creds(&fresh).expired());
        // 不透明 token 没有 exp，不能当成过期
        assert!(!creds("opaque").expired());
    }

    /// 只读凭证走 load_for_send 必须被明确拒绝，而不是发出去等 401
    #[test]
    fn a_read_only_credential_is_refused_for_sending() {
        // 把 HOME 指到空目录：否则这台机器上真有的 ~/.codex/auth.json 会被找出来，
        // 测试就变成「验证开发机的登录态」了
        let _env = crate::testenv::no_credentials("ro-send");
        let ro = jwt::fake(&["openid", "model.read"], store::now_secs() + 3_600);
        assert!(!creds(&ro).capability().send_capable());
        std::env::set_var("LCHATGPT_ACCESS_TOKEN", &ro);
        let err = load_for_send().expect_err("只读凭证不能发消息");
        std::env::remove_var("LCHATGPT_ACCESS_TOKEN");
        let msg = format!("{err}");
        assert!(msg.contains("只能读"), "{msg}");
        assert!(msg.contains("codex login"), "{msg}");
    }
}
