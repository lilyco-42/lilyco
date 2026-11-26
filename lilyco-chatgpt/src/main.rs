//! lchatgpt —— 把 chatgpt.com 的账号后端写成 CLI。
//!
//! 一个 struct 派生四端：CLI / TUI / Web / MCP 共用同一份命令 schema，
//! 同一份 handler。产物是**一个自包含的二进制**，不需要 OpenAI API key，
//! 也不需要安装 ego-lite 或 chromium。
//!
//! # 登录态从哪来
//!
//! 三条路，先命中先用（详见 `lchatgpt auth`）：
//!
//! 1. `LCHATGPT_ACCESS_TOKEN` 环境变量 —— CI / 临时借用；
//! 2. `lchatgpt login` 收进自己的仓库（`~/.local/share/lchatgpt/auth.json`，0600）；
//! 3. `~/.codex/auth.json` —— codex CLI 的登录态。
//!
//! `lchatgpt login --from browser` 会**可选地**连一个已经登录过 chatgpt.com 的
//! chromium（ego-lite 的 CDP 端口 9229 优先）抓一份登录态。这条路只是「获取凭证
//! 的一条来源」：CDP 客户端是纯 Rust（tungstenite），不启动也不要求浏览器，
//! 端口上没人就直接回落到别的来源，二进制本身照常可用。
//!
//! 两种凭证能力不同，`lchatgpt auth` 会直说：
//! - codex 那条（带 `api.connectors.invoke`）：能读也能走 codex API，还能自动续期；
//! - 浏览器登录态（只有 `model.request/read`）：只能读（网页聊天要的正是这种登录态，
//!   因为它驱动的是真浏览器，不靠 token 发消息）。
//!
//! # 两种额度：chat 是默认，codex 要勾选
//!
//! `ask` 默认花**chat 额度**：借一个已登录 chatgpt.com 的 chromium 把问题打进网页、
//! 点发送，再用 REST 把回复读回来。**codex 额度只有显式加 `--allow-codex` 才会动**，
//! 绝不会隐式消耗。`lchatgpt usage` 把两套额度分开报。
//!
//! ```bash
//! # 先把登录态收好，看看它是什么
//! lchatgpt login --from browser
//! lchatgpt auth
//!
//! # 这份凭证是谁的
//! lchatgpt whoami --json
//!
//! # 额度还剩多少、什么时候恢复
//! lchatgpt usage
//!
//! # 最近的会话 → 读其中一条 → 按关键词搜
//! lchatgpt list --limit 5
//! lchatgpt show-conversation --conversation-id 6b068588-ab08-83ea-a7fd-49cdd76f6682 --all
//! lchatgpt search --query repo
//!
//! # 真正提问（默认花 chat 额度：需要有个登录着 chatgpt.com 的 chromium）
//! lchatgpt ask --prompt "用一句话解释 borrow checker"
//! lchatgpt ask --prompt "..." --cdp 127.0.0.1:9229
//!
//! # 明确要花 codex 额度（必须自己勾）
//! lchatgpt ask --prompt "..." --allow-codex --effort low
//! lchatgpt ask --prompt "..." --allow-codex --json-stream
//!
//! # 同一套命令的其他三个面
//! lchatgpt --tui
//! lchatgpt --gui            # 浏览器界面
//! lchatgpt --mcp            # stdio JSON-RPC，给 AI 调
//! lchatgpt --schema         # 导出 JSON Schema
//! ```
//!
//! # 安全分级
//!
//! 只读命令（T0 ReadOnly）：`whoami` / `usage` / `models` / `list` /
//! `show-conversation` / `search` / `auth` —— 只发 HTTP GET 或只读本地文件，
//! 不改远端任何状态，所以 MCP 面也放行。
//!
//! 写命令（T1 Confirm）：`ask`（向模型发送内容并消耗额度 —— 默认 chat，`--allow-codex`
//! 才动 codex）、`login`（写本地凭证文件、会去读浏览器登录态）、`serve`
//! （起一个 OpenAI 兼容的 HTTP 端，外部客户端可以通过它花额度提问）。
//! MCP / Web 这类自动化面注册时用 `DenyElevated`，因此只在本地交互面可用。
//!
//! # 给别的程序当模型提供端
//!
//! `lchatgpt serve --port 8790` 把同一套能力吐成 OpenAI 兼容的 `/v1/models` 与
//! `/v1/chat/completions`（SSE）。默认只提供 `lchatgpt:chat`（chat 额度）；
//! 启动时加 `--allow-codex` 才会多露出 `lchatgpt:codex`。这样 dsh（DeepSeek
//! Harness）之类的客户端把它当一条 `api: openai-completions` 的 route 就能用。

mod api;
mod ask;
mod auth;
mod authinfo;
mod browser;
mod chat;
mod jwt;
mod list;
mod login;
mod models;
mod refresh;
mod search;
#[cfg(feature = "server")]
mod serve;
mod show;
mod store;
mod usage;
mod util;
mod whoami;

use lilyco::prelude::*;
use std::sync::Arc;

pub fn build_registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);
    #[allow(unused_mut)]
    let mut cmds: Vec<RegisteredCommand> = vec![
        RegisteredCommand::from_app::<whoami::Whoami>(),
        RegisteredCommand::from_app::<usage::Usage>(),
        RegisteredCommand::from_app::<models::Models>(),
        RegisteredCommand::from_app::<list::List>(),
        RegisteredCommand::from_app::<show::ShowConversation>(),
        RegisteredCommand::from_app::<search::Search>(),
        RegisteredCommand::from_app::<authinfo::Auth>(),
        RegisteredCommand::from_app::<ask::Ask>(),
        RegisteredCommand::from_app::<login::Login>(),
    ];
    // `serve` 只在编了 server feature 时存在：它是把同一批能力吐成 OpenAI 兼容
    // HTTP 端的那一层，给 dsh 之类的外部客户端当模型提供端用。
    #[cfg(feature = "server")]
    cmds.push(RegisteredCommand::from_app::<serve::Serve>());
    for c in cmds {
        let name = c.schema.name.clone();
        reg.register(c)
            .unwrap_or_else(|e| panic!("注册命令 `{name}` 失败: {e}"));
    }
    reg
}

pub fn policy_for(backend: lilyco::Backend) -> Arc<dyn SafetyPolicy> {
    match backend {
        // 自动化调用面（MCP）只放行只读命令
        lilyco::Backend::Mcp => Arc::new(DenyElevated),
        #[allow(unreachable_patterns)]
        _ => Arc::new(Interactive),
    }
}

fn main() {
    let backend = lilyco::detect_registry_backend();
    let reg = build_registry_with_policy(policy_for(backend));
    lilyco::run_registry_with("lchatgpt", reg, backend);
}

/// 进程环境变量是全局状态：动 env 的测试要排队，并且离开时把值还原。
///
/// 这不是在防「并发 bug」—— cargo test 默认就在一个进程里多线程跑，
/// 不排队的话谁先跑谁决定结果（实测过：某台机器上真有一份 ~/.codex/auth.json，
/// 「没有凭证」的断言就会读到它）。
#[cfg(test)]
mod testenv {
    use std::collections::BTreeMap;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    const WATCHED: [&str; 6] = [
        "HOME",
        "LCHATGPT_ACCESS_TOKEN",
        "LCHATGPT_ACCOUNT_ID",
        "LCHATGPT_AUTH_FILE",
        "LCHATGPT_STORE",
        "XDG_DATA_HOME",
    ];

    fn lock() -> MutexGuard<'static, ()> {
        static CELL: OnceLock<Mutex<()>> = OnceLock::new();
        // 别的测试 panic 了也不该把锁毒死，后面照常跑
        CELL.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// 拿到锁 + 出作用域时把环境还原
    pub struct Env {
        _guard: MutexGuard<'static, ()>,
        saved: BTreeMap<&'static str, Option<String>>,
    }

    impl Drop for Env {
        fn drop(&mut self) {
            for (k, v) in self.saved.iter() {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    fn snapshot() -> Env {
        let guard = lock();
        let saved = WATCHED
            .iter()
            .map(|k| (*k, std::env::var(k).ok()))
            .collect();
        Env {
            _guard: guard,
            saved,
        }
    }

    /// 「这台机器上什么都没登录」：HOME 指到空目录，其它来源指到不存在的文件。
    /// 用来把真机上那份 ~/.codex/auth.json 挡在门外。
    pub fn no_credentials(tag: &str) -> Env {
        let env = snapshot();
        let home = std::env::temp_dir().join(format!("lchatgpt-nohome-{tag}"));
        let _ = std::fs::remove_dir_all(&home);
        std::env::set_var("HOME", &home);
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("LCHATGPT_ACCESS_TOKEN");
        std::env::remove_var("LCHATGPT_ACCOUNT_ID");
        std::env::set_var("LCHATGPT_AUTH_FILE", home.join("no-such-auth.json"));
        std::env::set_var("LCHATGPT_STORE", home.join("no-such-store.json"));
        env
    }

    /// 先拿锁、再自己设 env（值在离开时还原）
    pub fn scoped() -> Env {
        snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        let mut v: Vec<String> = build_registry_with_policy(Arc::new(Unrestricted))
            .visible()
            .map(|c| c.schema.name.clone())
            .collect();
        v.sort();
        v
    }

    fn tier(name: &str) -> SafetyTier {
        build_registry_with_policy(Arc::new(Unrestricted))
            .get(name)
            .unwrap_or_else(|| panic!("缺少命令 `{name}`"))
            .schema
            .safety
    }

    /// 命令集合是契约：加删都必须是显式的改动
    #[test]
    fn registry_lists_the_commands_this_domain_answers() {
        // `serve` 挂在 server feature 后面：关掉它就只有九条命令
        let mut want: Vec<String> = vec![
            "ask".to_string(),
            "auth".to_string(),
            "list".to_string(),
            "login".to_string(),
            "models".to_string(),
            "search".to_string(),
        ];
        #[cfg(feature = "server")]
        want.push("serve".to_string());
        want.push("show-conversation".to_string());
        want.push("usage".to_string());
        want.push("whoami".to_string());
        want.sort();
        // Registry 内部是 HashMap，顺序不保证 —— 比排序后的集合
        assert_eq!(names(), want);
    }

    /// GET 只读 = T0，改动远端或写本地凭证 = T1；分级错了 MCP 面就会放行写操作
    #[test]
    fn every_command_carries_the_declared_safety_tier() {
        for name in [
            "whoami",
            "usage",
            "models",
            "list",
            "show-conversation",
            "search",
            "auth",
        ] {
            assert_eq!(tier(name), SafetyTier::ReadOnly, "`{name}` 应当是只读");
        }
        assert_eq!(
            tier("ask"),
            SafetyTier::Confirm,
            "`ask` 会消耗额度，必须是 T1"
        );
        assert_eq!(
            tier("login"),
            SafetyTier::Confirm,
            "`login` 会写本地凭证并读浏览器登录态，必须是 T1"
        );
        #[cfg(feature = "server")]
        assert_eq!(
            tier("serve"),
            SafetyTier::Confirm,
            "`serve` 会让外部进程花额度提问，必须是 T1"
        );
    }

    /// MCP 面必须拒绝 ask 与 login —— 否则 AI 可以在你不知情时烧配额 / 换掉你的凭证
    #[test]
    fn the_mcp_surface_refuses_the_elevated_commands() {
        let reg = build_registry_with_policy(policy_for(lilyco::Backend::Mcp));
        let args = serde_json::json!({});
        let verdict = |name: &str| {
            let cmd = reg.get(name).unwrap_or_else(|| panic!("缺少命令 `{name}`"));
            reg.policy().check(GateRequest {
                command: &cmd.schema.name,
                tier: cmd.schema.safety,
                args: &args,
            })
        };
        assert!(
            matches!(verdict("ask"), GateDecision::Deny(_)),
            "MCP 面必须拦住 ask"
        );
        assert!(
            matches!(verdict("login"), GateDecision::Deny(_)),
            "MCP 面必须拦住 login"
        );
        #[cfg(feature = "server")]
        assert!(
            matches!(verdict("serve"), GateDecision::Deny(_)),
            "MCP 面必须拦住 serve —— 它会让外部进程花额度"
        );
        assert!(
            matches!(verdict("whoami"), GateDecision::Allow),
            "MCP 面应当放行 whoami"
        );
    }

    /// about 要能独立回答「这个命令干什么」：太短等于没写
    #[test]
    fn every_command_documents_its_side_effects_and_result() {
        for c in build_registry_with_policy(Arc::new(Unrestricted)).visible() {
            assert!(
                c.schema.about.len() > 120,
                "`{}` 的 about 太短，没写清返回什么/改动了什么",
                c.schema.name
            );
        }
    }
}
