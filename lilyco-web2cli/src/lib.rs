//! `lilyco-web2cli` — 将任意网页、OpenAPI 文档或 HAR 抓包转化为 Lilyco 四端应用（CLI / TUI / Web / MCP）。
//!
//! 两条核心能力：
//! 1. **零配置通用网页 CLI**（借鉴 `only-cli/oc`）：`open` / `do` / `find` / `read` / `raw`，带持久 Cookie 会话与可选无头浏览器 `--render`；
//! 2. **自动发现与四端编译**（借鉴 `site2cli` / `OpenCLI` + Lilyco `Registry`）：将网页 `<form>` / JS API / OpenAPI / HAR 编译为 `WebCliSpec` → `lilyco::__core::registry::Registry` → 自动获得 CLI / TUI / Web UI / MCP Server。

pub mod cdp;
pub mod codegen;
pub mod discover;
pub mod distill;
pub mod dom;
pub mod exec;
pub mod plugin;
pub mod proofread;
pub mod recipe;
pub mod review;
#[cfg(feature = "server")]
pub mod server;
pub mod session;
pub mod spec;
pub mod vault;
pub mod wizard;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use lilyco::__core::error::AppError;
use lilyco::__core::registry::Registry;
use lilyco::__core::safety::SafetyPolicy;

pub use cdp::{CdpClient, CdpCookie, CdpElement, CdpSnapshot, CdpTarget, ManagedChrome};
pub use codegen::{generate_crate, write_generated_crate, GeneratedCrate};
pub use discover::{discover_from_har, discover_from_html, discover_from_openapi};
pub use distill::{distill_response, ActionKind, DistilledPage, NumberedAction, TableData};
pub use dom::DomTree;
pub use plugin::{
    builtin_plugins, check_plugin_entitlement, current_unix_secs, issue_license, verify_license,
    EnterpriseLicense, PluginDescriptor, PLUGIN_CDP_WIZARD, PLUGIN_FORM_REVIEW, PLUGIN_PROOFREAD,
    PLUGIN_VISION,
};
pub use proofread::{
    proofread_draft, proofread_profile, validate_cn_id_card, validate_cn_phone, validate_uscc,
    IssueSeverity, ProofreadIssue, ProofreadReport,
};
pub use recipe::{FieldBinding, RecipeStore, SiteRecipe};
pub use review::{DraftStatus, DraftStore, SubmissionDraft};
#[cfg(feature = "server")]
pub use server::{build_router, serve_gateway, GatewayConfig, GatewayState};
pub use session::{find_browser, render_with_headless_browser, FetchResponse, SessionManager};
pub use spec::{
    ArgLocation, BodyEncoding, HttpMethod, ResponseExtract, WebArgKind, WebArgSpec, WebCliSpec,
    WebCommandSpec,
};
pub use vault::{auto_map_form, AttachmentEntry, AutoMapResult, VaultProfile, VaultStore};
pub use wizard::{run_wizard_cdp, WizardPlan, WizardRunReport, WizardStatus, WizardStep};

/// 抓取任意 URL 并自动发现为 `WebCliSpec`（自动识别 OpenAPI JSON 或 HTML 页面）。
pub fn discover_url(
    session: &SessionManager,
    url: &str,
    render_js: bool,
) -> Result<WebCliSpec, AppError> {
    let resp = session.request("GET", url, &BTreeMap::new(), None, render_js)?;
    let trimmed = resp.body.trim_start();
    if (resp.content_type.contains("json") || trimmed.starts_with('{'))
        && (trimmed.contains("\"openapi\"") || trimmed.contains("\"swagger\""))
    {
        if let Ok(spec) = discover_from_openapi(&resp.final_url, &resp.body) {
            return Ok(spec);
        }
    }
    Ok(discover_from_html(&resp.final_url, &resp.body))
}

/// 从本地文件（`.json` / `.toml` `WebCliSpec` 或 OpenAPI JSON）或实时 URL 加载并编译为 `WebCliSpec`。
pub fn load_or_discover_spec(
    session: &SessionManager,
    source: &str,
    render_js: bool,
) -> Result<WebCliSpec, AppError> {
    if source.starts_with("http://") || source.starts_with("https://") {
        return discover_url(session, source, render_js);
    }
    let path = Path::new(source);
    let content = fs::read_to_string(path)
        .map_err(|e| AppError::InvalidInput(format!("读取文件 `{source}` 失败: {e}")))?;
    let trimmed = content.trim_start();
    if trimmed.starts_with('{')
        && (trimmed.contains("\"openapi\"") || trimmed.contains("\"swagger\""))
    {
        return discover_from_openapi("https://example.com", &content);
    }
    WebCliSpec::from_str_auto(&content)
}

/// 一步将任意 URL 编译为可直接运行的 `lilyco` `Registry`。
pub fn compile_url_to_registry(
    url: &str,
    session: Arc<SessionManager>,
    policy: Option<Arc<dyn SafetyPolicy>>,
    render_js: bool,
) -> Result<Registry, AppError> {
    let spec = discover_url(&session, url, render_js)?;
    spec.to_registry(session, policy)
}
