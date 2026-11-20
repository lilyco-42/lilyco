//! `lweb2cli` — 将任意网页转化为 CLI / TUI / Web / MCP 的统一工具。
//!
//! 两种使用方式：
//! 1. **通用网页模式**（标准 Lilyco Registry）：
//!    - `lweb2cli open --url https://news.ycombinator.com`
//!    - `lweb2cli do --index 1`
//!    - `lweb2cli find --query rust`
//!    - `lweb2cli read --index 1`
//!    - `lweb2cli inspect --url https://example.com --save site.webcli.json`
//!    - `lweb2cli from-har --file trace.har --save site.webcli.json`
//!    - `lweb2cli codegen --source site.webcli.json --out ./my-site-cli`
//!    - `lweb2cli --mcp`（让 AI Agent 直接把任意网页当工具浏览与发现）
//! 2. **实时网站直通模式**（`serve <url-or-spec> ...`）：
//!    - `lweb2cli serve https://example.com --schema`
//!    - `lweb2cli serve https://example.com read-page --json`
//!    - `lweb2cli serve site.webcli.json --mcp` / `--tui` / `--gui`

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lilyco::prelude::*;
use lilyco_web2cli::{
    builtin_plugins, current_unix_secs, discover_from_har, distill_response, generate_crate,
    issue_license, load_or_discover_spec, proofread_draft, proofread_profile, run_wizard_cdp,
    verify_license, write_generated_crate, CdpClient, DraftStore, EnterpriseLicense, ManagedChrome,
    RecipeStore, SessionManager, VaultStore, WizardPlan,
};
use serde_json::{json, Map, Value};

#[derive(App)]
#[app(
    name = "open",
    run = "run_open",
    about = "Fetch any webpage and render a compact, numbered CLI view"
)]
struct OpenCmd {
    #[arg(about = "Target HTTP/HTTPS URL")]
    url: String,
    #[arg(about = "Render JS/SPA page using local headless Chrome/Edge")]
    render: bool,
    #[arg(default = 4000, range = 200..=100000, about = "Character budget for compact view")]
    budget: usize,
    #[arg(
        default = "default",
        about = "Persistent session name (cookies & history)"
    )]
    session: String,
}

fn run_open(app: &OpenCmd, ctx: &Context) -> Result<Value, AppError> {
    let sess = SessionManager::new(&app.session);
    ctx.log(LogLevel::Info, format!("GET {}", app.url));
    let resp = sess.request("GET", &app.url, &BTreeMap::new(), None, app.render)?;
    let distilled = distill_response(&resp.final_url, &resp.content_type, &resp.body, app.budget);
    Ok(json!({
        "url": distilled.url,
        "status": resp.status,
        "title": distilled.title,
        "rendered_with_browser": resp.rendered_with_browser,
        "actions_count": distilled.actions.len(),
        "tables_count": distilled.tables.len(),
        "compact_view": distilled.compact_view,
    }))
}

#[derive(App)]
#[app(
    name = "do-action",
    run = "run_do_action",
    about = "Follow numbered link [n] or inspect control [n] from the last opened page"
)]
struct DoCmd {
    #[arg(range = 1..=10000, about = "1-based action index from `open` output")]
    index: usize,
    #[arg(about = "Render JS/SPA page using local headless Chrome/Edge")]
    render: bool,
    #[arg(default = 4000, range = 200..=100000)]
    budget: usize,
    #[arg(default = "default", about = "Session name")]
    session: String,
}

fn run_do_action(app: &DoCmd, ctx: &Context) -> Result<Value, AppError> {
    let sess = SessionManager::new(&app.session);
    let snap = sess.state_snapshot();
    let last_url = snap.last_url.ok_or_else(|| {
        AppError::InvalidInput("当前会话尚未打开任何网页，请先执行 `open`".into())
    })?;
    let last_body = snap
        .last_body
        .ok_or_else(|| AppError::InvalidInput("当前会话无缓存页面内容".into()))?;
    let last_ct = snap.last_content_type.unwrap_or_else(|| "text/html".into());
    let distilled = distill_response(&last_url, &last_ct, &last_body, 0);
    let action = distilled
        .actions
        .get(app.index - 1)
        .ok_or_else(|| {
            AppError::InvalidArg(format!(
                "编号 [{}] 超出范围 (1..={})",
                app.index,
                distilled.actions.len()
            ))
        })?
        .clone();

    if let Some(href) = &action.href {
        ctx.log(LogLevel::Info, format!("Follow [{}] -> {href}", app.index));
        let resp = sess.request("GET", href, &BTreeMap::new(), None, app.render)?;
        let next_page =
            distill_response(&resp.final_url, &resp.content_type, &resp.body, app.budget);
        Ok(json!({
            "followed": action,
            "url": next_page.url,
            "status": resp.status,
            "title": next_page.title,
            "compact_view": next_page.compact_view,
        }))
    } else {
        Ok(json!({
            "action": action,
            "hint": "Non-link control; use `inspect` + `serve` to submit forms as typed CLI subcommands.",
        }))
    }
}

#[derive(App)]
#[app(
    name = "find",
    run = "run_find",
    about = "Search matching actions and sections in the last opened page"
)]
struct FindCmd {
    #[arg(about = "Case-insensitive search query")]
    query: String,
    #[arg(default = "default", about = "Session name")]
    session: String,
}

fn run_find(app: &FindCmd, _ctx: &Context) -> Result<Value, AppError> {
    let sess = SessionManager::new(&app.session);
    let snap = sess.state_snapshot();
    let last_url = snap.last_url.ok_or_else(|| {
        AppError::InvalidInput("当前会话尚未打开任何网页，请先执行 `open`".into())
    })?;
    let last_body = snap.last_body.unwrap_or_default();
    let last_ct = snap.last_content_type.unwrap_or_else(|| "text/html".into());
    let distilled = distill_response(&last_url, &last_ct, &last_body, 0);
    let q = app.query.to_ascii_lowercase();

    let matching_actions: Vec<_> = distilled
        .actions
        .into_iter()
        .filter(|a| {
            a.label.to_ascii_lowercase().contains(&q)
                || a.href
                    .as_deref()
                    .is_some_and(|h| h.to_ascii_lowercase().contains(&q))
        })
        .collect();
    let matching_sections: Vec<_> = distilled
        .sections
        .into_iter()
        .enumerate()
        .filter(|(_, s)| s.to_ascii_lowercase().contains(&q))
        .map(|(i, s)| json!({ "section": i + 1, "text": s }))
        .collect();

    Ok(json!({
        "url": last_url,
        "query": app.query,
        "actions": matching_actions,
        "sections": matching_sections,
    }))
}

#[derive(App)]
#[app(
    name = "read",
    run = "run_read",
    about = "Read full text of a numbered content section from the last opened page"
)]
struct ReadCmd {
    #[arg(range = 1..=10000, about = "1-based section index")]
    index: usize,
    #[arg(default = "default", about = "Session name")]
    session: String,
}

fn run_read(app: &ReadCmd, _ctx: &Context) -> Result<Value, AppError> {
    let sess = SessionManager::new(&app.session);
    let snap = sess.state_snapshot();
    let last_url = snap.last_url.ok_or_else(|| {
        AppError::InvalidInput("当前会话尚未打开任何网页，请先执行 `open`".into())
    })?;
    let last_body = snap.last_body.unwrap_or_default();
    let last_ct = snap.last_content_type.unwrap_or_else(|| "text/html".into());
    let distilled = distill_response(&last_url, &last_ct, &last_body, 0);
    let text = distilled.sections.get(app.index - 1).ok_or_else(|| {
        AppError::InvalidArg(format!(
            "段落 ({}) 超出范围 (1..={})",
            app.index,
            distilled.sections.len()
        ))
    })?;
    Ok(json!({
        "url": last_url,
        "section": app.index,
        "text": text,
    }))
}

#[derive(App)]
#[app(
    name = "raw",
    run = "run_raw",
    about = "Fetch a webpage and output distilled Markdown"
)]
struct RawCmd {
    #[arg(about = "Target HTTP/HTTPS URL")]
    url: String,
    #[arg(about = "Render JS/SPA page using local headless Chrome/Edge")]
    render: bool,
    #[arg(default = "default", about = "Session name")]
    session: String,
}

fn run_raw(app: &RawCmd, _ctx: &Context) -> Result<Value, AppError> {
    let sess = SessionManager::new(&app.session);
    let resp = sess.request("GET", &app.url, &BTreeMap::new(), None, app.render)?;
    let distilled = distill_response(&resp.final_url, &resp.content_type, &resp.body, 0);
    Ok(json!({
        "url": distilled.url,
        "title": distilled.title,
        "markdown": distilled.markdown,
    }))
}

#[derive(App)]
#[app(
    name = "inspect",
    run = "run_inspect",
    safety = "T1",
    about = "Discover forms, APIs, and schemas on any webpage and emit/save a WebCliSpec"
)]
struct InspectCmd {
    #[arg(about = "Webpage URL or OpenAPI JSON URL/path")]
    url: String,
    #[arg(about = "Render JS/SPA page using local headless Chrome/Edge")]
    render: bool,
    #[arg(default = "json", about = "Output spec format")]
    format: SpecFormat,
    #[arg(about = "Optional output file path to save the WebCliSpec")]
    save: Option<String>,
    #[arg(default = "default", about = "Session name")]
    session: String,
}

#[derive(Clone, Debug, ValueEnum)]
enum SpecFormat {
    Json,
    Toml,
}

fn run_inspect(app: &InspectCmd, ctx: &Context) -> Result<Value, AppError> {
    let sess = SessionManager::new(&app.session);
    ctx.log(
        LogLevel::Info,
        format!("Discovering capabilities from {}", app.url),
    );
    let spec = load_or_discover_spec(&sess, &app.url, app.render)?;
    let serialized = match app.format {
        SpecFormat::Json => spec.to_json_pretty()?,
        SpecFormat::Toml => spec.to_toml_pretty()?,
    };
    if let Some(save_path) = &app.save {
        if let Some(parent) = Path::new(save_path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::Runtime(format!("创建目录失败: {e}")))?;
        }
        fs::write(save_path, &serialized)
            .map_err(|e| AppError::Runtime(format!("写入 `{save_path}` 失败: {e}")))?;
    }
    Ok(json!({
        "name": spec.name,
        "base_url": spec.base_url,
        "commands_count": spec.commands.len(),
        "saved_to": app.save,
        "spec": spec,
    }))
}

#[derive(App)]
#[app(
    name = "from-har",
    run = "run_from_har",
    safety = "T1",
    about = "Convert a browser HAR network capture into a WebCliSpec"
)]
struct FromHarCmd {
    #[arg(must_exist = true, about = "Path to .har JSON file")]
    file: PathBuf,
    #[arg(about = "Optional host filter (e.g. api.example.com)")]
    host: Option<String>,
    #[arg(about = "Optional output file path to save the WebCliSpec JSON")]
    save: Option<String>,
}

fn run_from_har(app: &FromHarCmd, _ctx: &Context) -> Result<Value, AppError> {
    let raw = fs::read_to_string(&app.file)
        .map_err(|e| AppError::InvalidInput(format!("读取 HAR 文件失败: {e}")))?;
    let spec = discover_from_har(&raw, app.host.as_deref())?;
    if let Some(save_path) = &app.save {
        let json = spec.to_json_pretty()?;
        fs::write(save_path, json)
            .map_err(|e| AppError::Runtime(format!("写入 `{save_path}` 失败: {e}")))?;
    }
    Ok(json!({
        "name": spec.name,
        "base_url": spec.base_url,
        "commands_count": spec.commands.len(),
        "saved_to": app.save,
        "spec": spec,
    }))
}

#[derive(App)]
#[app(
    name = "codegen",
    run = "run_codegen",
    safety = "T1",
    about = "Generate a standalone Rust Lilyco crate (CLI/TUI/Web/MCP) from any URL or WebCliSpec"
)]
struct CodegenCmd {
    #[arg(about = "Webpage URL or WebCliSpec file path")]
    source: String,
    #[arg(about = "Output directory for the generated Rust crate")]
    out: PathBuf,
    #[arg(about = "Optional crate/binary name override")]
    name: Option<String>,
    #[arg(default = "default", about = "Session name")]
    session: String,
}

fn run_codegen(app: &CodegenCmd, _ctx: &Context) -> Result<Value, AppError> {
    let sess = SessionManager::new(&app.session);
    let spec = load_or_discover_spec(&sess, &app.source, false)?;
    let generated = generate_crate(&spec, app.name.as_deref())?;
    write_generated_crate(&generated, &app.out)?;
    Ok(json!({
        "crate_name": generated.crate_name,
        "out_dir": app.out,
        "commands_count": spec.commands.len(),
        "files": ["Cargo.toml", "spec.webcli.json", "src/main.rs"],
    }))
}

#[derive(Clone, Debug, ValueEnum)]
enum SessionOp {
    List,
    Clear,
    SeedCookie,
}

#[derive(App)]
#[app(
    name = "session",
    run = "run_session",
    safety = "T1",
    about = "Manage persistent web2cli sessions and cookies"
)]
struct SessionCmd {
    #[arg(default = "list", about = "Operation: list | clear | seed-cookie")]
    op: SessionOp,
    #[arg(default = "default", about = "Session name")]
    name: String,
    #[arg(about = "Cookie domain (for seed-cookie)")]
    domain: Option<String>,
    #[arg(about = "Cookie key=value pair (for seed-cookie)")]
    cookie: Option<String>,
}

fn run_session(app: &SessionCmd, _ctx: &Context) -> Result<Value, AppError> {
    let sess = SessionManager::new(&app.name);
    match app.op {
        SessionOp::List => Ok(json!({
            "session_dir": sess.session_dir(),
            "sessions": sess.list_sessions(),
        })),
        SessionOp::Clear => {
            sess.clear()?;
            Ok(json!({ "cleared": app.name }))
        }
        SessionOp::SeedCookie => {
            let domain = app
                .domain
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("seed-cookie 需要 --domain".into()))?;
            let cookie = app.cookie.as_deref().ok_or_else(|| {
                AppError::InvalidArg("seed-cookie 需要 --cookie name=value".into())
            })?;
            let (k, v) = cookie
                .split_once('=')
                .ok_or_else(|| AppError::InvalidArg("cookie 格式应为 name=value".into()))?;
            sess.seed_cookie(domain, k.trim(), v.trim())?;
            Ok(json!({ "seeded": true, "session": app.name, "domain": domain, "cookie": k.trim() }))
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
enum VaultOp {
    List,
    Show,
    Set,
    Attach,
    Import,
}

#[derive(App)]
#[app(
    name = "vault",
    run = "run_vault",
    safety = "T1",
    about = "Manage categorized enterprise/personal profiles and attachment materials for form auto-fill"
)]
struct VaultCmd {
    #[arg(
        default = "list",
        about = "Operation: list | show | set | attach | import"
    )]
    op: VaultOp,
    #[arg(default = "default", about = "Profile name (e.g. acme-corp)")]
    profile: String,
    #[arg(
        default = "company",
        about = "Category (company | legal_rep | finance | contact | custom)"
    )]
    category: String,
    #[arg(about = "Field or attachment key")]
    key: Option<String>,
    #[arg(about = "Field value (for set)")]
    value: Option<String>,
    #[arg(about = "Human-readable label (for attach)")]
    label: Option<String>,
    #[arg(about = "Local file path (for attach or import JSON)")]
    file: Option<PathBuf>,
}

fn run_vault(app: &VaultCmd, _ctx: &Context) -> Result<Value, AppError> {
    let store = VaultStore::new();
    match app.op {
        VaultOp::List => Ok(json!({
            "vault_dir": store.root_dir(),
            "profiles": store.list_profiles(),
        })),
        VaultOp::Show => {
            let profile = store.load(&app.profile)?;
            Ok(serde_json::to_value(profile).unwrap_or_default())
        }
        VaultOp::Set => {
            let key = app
                .key
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("vault set 需要 --key".into()))?;
            let value = app
                .value
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("vault set 需要 --value".into()))?;
            let profile = store.set_field(&app.profile, &app.category, key, value)?;
            Ok(json!({
                "updated": true,
                "profile": profile.name,
                "category": app.category,
                "key": key,
            }))
        }
        VaultOp::Attach => {
            let key = app
                .key
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("vault attach 需要 --key".into()))?;
            let file = app
                .file
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("vault attach 需要 --file".into()))?;
            let label = app.label.as_deref().unwrap_or(key);
            let profile = store.add_attachment(&app.profile, &app.category, key, label, file)?;
            Ok(json!({
                "attached": true,
                "profile": profile.name,
                "key": key,
                "attachment": profile.attachments.get(key),
            }))
        }
        VaultOp::Import => {
            let file = app
                .file
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("vault import 需要 --file <json>".into()))?;
            let raw = fs::read_to_string(file)
                .map_err(|e| AppError::InvalidInput(format!("读取资料 JSON 失败: {e}")))?;
            let profile = store.import_json(&app.profile, &raw)?;
            Ok(json!({
                "imported": true,
                "profile": profile.name,
                "categories": profile.categories.keys().collect::<Vec<_>>(),
                "attachments_count": profile.attachments.len(),
            }))
        }
    }
}

#[derive(App)]
#[app(
    name = "import-cookies",
    run = "run_import_cookies",
    safety = "T1",
    about = "Import browser cookies (EditThisCookie/Playwright JSON, Netscape cookies.txt, or raw Cookie header) into a session"
)]
struct ImportCookiesCmd {
    #[arg(about = "Path to cookies.json or cookies.txt")]
    file: Option<PathBuf>,
    #[arg(about = "Raw Cookie header string (e.g. 'sid=abc; token=xyz')")]
    raw: Option<String>,
    #[arg(about = "Default domain when importing raw Cookie header or domain-less JSON")]
    domain: Option<String>,
    #[arg(default = "default", about = "Target session name")]
    session: String,
}

fn run_import_cookies(app: &ImportCookiesCmd, _ctx: &Context) -> Result<Value, AppError> {
    let content = if let Some(path) = &app.file {
        fs::read_to_string(path)
            .map_err(|e| AppError::InvalidInput(format!("读取 Cookie 文件失败: {e}")))?
    } else if let Some(raw) = &app.raw {
        raw.clone()
    } else {
        return Err(AppError::InvalidArg(
            "请提供 --file <cookies.json|cookies.txt> 或 --raw <cookie_header>".into(),
        ));
    };
    let sess = SessionManager::new(&app.session);
    let count = sess.import_cookies_from_str(&content, app.domain.as_deref())?;
    Ok(json!({
        "imported_count": count,
        "session": app.session,
    }))
}

#[derive(App)]
#[app(
    name = "prepare-submit",
    run = "run_prepare_submit",
    about = "Discover a target form and auto-fill a PendingReview draft from a categorized Vault profile (never submits without human approval)"
)]
struct PrepareSubmitCmd {
    #[arg(about = "Target webpage URL or WebCliSpec file path")]
    source: String,
    #[arg(about = "Optional form/command name (defaults to first POST/form command)")]
    form: Option<String>,
    #[arg(default = "default", about = "Vault profile name")]
    profile: String,
    #[arg(default = "default", about = "Session name (with imported cookies)")]
    session: String,
    #[arg(about = "Render JS/SPA page using local headless Chrome/Edge")]
    render: bool,
    #[arg(about = "Optional JSON object of manual field overrides")]
    override_json: Option<String>,
}

fn run_prepare_submit(app: &PrepareSubmitCmd, ctx: &Context) -> Result<Value, AppError> {
    let sess = SessionManager::new(&app.session);
    ctx.log(
        LogLevel::Info,
        format!("Discovering form from {}", app.source),
    );
    let spec = load_or_discover_spec(&sess, &app.source, app.render)?;

    let cmd = if let Some(name) = &app.form {
        spec.commands
            .iter()
            .find(|c| c.name == *name)
            .cloned()
            .ok_or_else(|| AppError::InvalidArg(format!("未找到表单/命令 `{name}`")))?
    } else {
        spec.commands
            .iter()
            .find(|c| !matches!(c.name.as_str(), "read-page" | "links" | "tables"))
            .cloned()
            .ok_or_else(|| AppError::InvalidInput("该页面未发现可填报的表单或 API 端点".into()))?
    };

    let overrides: Map<String, Value> = if let Some(raw) = &app.override_json {
        serde_json::from_str::<Value>(raw)
            .map_err(|e| AppError::InvalidArg(format!("override-json 解析失败: {e}")))?
            .as_object()
            .cloned()
            .ok_or_else(|| AppError::InvalidArg("override-json 须为 JSON 对象".into()))?
    } else {
        Map::new()
    };

    let vault = VaultStore::new();
    let profile = vault.load(&app.profile)?;
    let recipes = RecipeStore::new();
    let matched_recipe = recipes.find_matching(&cmd.url, &cmd.name);
    let drafts = DraftStore::new();
    let draft = drafts.prepare_draft_with_recipe(
        &profile,
        &app.session,
        &spec.base_url,
        cmd,
        matched_recipe.as_ref(),
        &overrides,
    )?;

    Ok(json!({
        "draft_id": draft.id,
        "status": draft.status,
        "used_recipe": draft.recipe_id,
        "target_url": draft.command.url,
        "method": draft.command.method,
        "encoding": draft.command.encoding,
        "filled_args": draft.args,
        "mapping_notes": draft.mapping_notes,
        "missing_required": draft.missing_required,
        "next_step": format!("人类核对后请运行: lweb2cli review --op approve --id {} && lweb2cli commit-submit --id {}", draft.id, draft.id),
    }))
}

#[derive(Clone, Debug, ValueEnum)]
enum ReviewOp {
    List,
    Show,
    Patch,
    Approve,
    Reject,
}

#[derive(App)]
#[app(
    name = "review",
    run = "run_review",
    safety = "T1",
    about = "Human-in-the-loop review gate: list, inspect, patch, approve, or reject a prepared submission draft"
)]
struct ReviewCmd {
    #[arg(
        default = "list",
        about = "Operation: list | show | patch | approve | reject"
    )]
    op: ReviewOp,
    #[arg(about = "Draft ID")]
    id: Option<String>,
    #[arg(about = "Field name to patch (for op=patch)")]
    field: Option<String>,
    #[arg(about = "New field value (for op=patch)")]
    value: Option<String>,
    #[arg(about = "Optional reviewer audit note (for approve/reject)")]
    note: Option<String>,
}

fn run_review(app: &ReviewCmd, _ctx: &Context) -> Result<Value, AppError> {
    let store = DraftStore::new();
    match app.op {
        ReviewOp::List => {
            let items: Vec<Value> = store
                .list()
                .into_iter()
                .map(|d| {
                    json!({
                        "id": d.id,
                        "status": d.status,
                        "profile": d.profile,
                        "target_url": d.command.url,
                        "missing_required": d.missing_required.len(),
                    })
                })
                .collect();
            Ok(json!({ "drafts_dir": store.root_dir(), "drafts": items }))
        }
        ReviewOp::Show => {
            let id = app
                .id
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("review show 需要 --id".into()))?;
            let draft = store.load(id)?;
            Ok(serde_json::to_value(draft).unwrap_or_default())
        }
        ReviewOp::Patch => {
            let id = app
                .id
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("review patch 需要 --id".into()))?;
            let field = app
                .field
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("review patch 需要 --field".into()))?;
            let val_str = app
                .value
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("review patch 需要 --value".into()))?;
            let draft = store.patch_arg(id, field, Value::String(val_str.to_string()))?;
            Ok(json!({
                "patched": true,
                "id": draft.id,
                "status": draft.status,
                "missing_required": draft.missing_required,
                "args": draft.args,
            }))
        }
        ReviewOp::Approve => {
            let id = app
                .id
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("review approve 需要 --id".into()))?;
            let draft = store.approve(id, app.note.clone())?;
            Ok(json!({
                "approved": true,
                "id": draft.id,
                "status": draft.status,
                "reviewer_note": draft.reviewer_note,
                "next_step": format!("lweb2cli commit-submit --id {}", draft.id),
            }))
        }
        ReviewOp::Reject => {
            let id = app
                .id
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("review reject 需要 --id".into()))?;
            let draft = store.reject(id, app.note.clone())?;
            Ok(json!({
                "rejected": true,
                "id": draft.id,
                "status": draft.status,
                "reviewer_note": draft.reviewer_note,
            }))
        }
    }
}

#[derive(App)]
#[app(
    name = "commit-submit",
    run = "run_commit_submit",
    safety = "T1",
    about = "Commit an Approved submission draft (with attachments & session cookies) and store the server receipt"
)]
struct CommitSubmitCmd {
    #[arg(about = "Approved Draft ID")]
    id: String,
}

fn run_commit_submit(app: &CommitSubmitCmd, ctx: &Context) -> Result<Value, AppError> {
    let store = DraftStore::new();
    let preview = store.load(&app.id)?;
    let sess = SessionManager::new(&preview.session);
    let submitted = store.commit_draft(&app.id, ctx, &sess)?;
    let learned_recipe = VaultStore::new()
        .load(&submitted.profile)
        .ok()
        .and_then(|prof| RecipeStore::new().learn_from_draft(&submitted, &prof).ok())
        .map(|r| r.id);
    Ok(json!({
        "submitted": true,
        "id": submitted.id,
        "status": submitted.status,
        "learned_recipe": learned_recipe,
        "receipt": submitted.receipt,
    }))
}

#[derive(Clone, Debug, ValueEnum)]
enum CdpOp {
    Targets,
    Navigate,
    Snapshot,
    Fill,
    Click,
    Screenshot,
    SyncCookies,
}

#[derive(App)]
#[app(
    name = "cdp",
    run = "run_cdp",
    safety = "T1",
    about = "Live Chrome DevTools Protocol control: attach to logged-in Chrome (--endpoint http://127.0.0.1:9222), snapshot all iframes (@e1..@eN), fill Vue/React forms, capture screenshots, and sync cookies"
)]
struct CdpCmd {
    #[arg(
        default = "http://127.0.0.1:9222",
        about = "Chrome remote debugging HTTP endpoint"
    )]
    endpoint: String,
    #[arg(
        default = "snapshot",
        about = "Operation: targets | navigate | snapshot | fill | click | screenshot | sync-cookies"
    )]
    op: CdpOp,
    #[arg(about = "Target URL (for navigate) or tab URL filter")]
    url: Option<String>,
    #[arg(about = "Element ref from snapshot (e.g. @e1, for fill/click)")]
    ref_id: Option<String>,
    #[arg(about = "Value to fill (for fill)")]
    value: Option<String>,
    #[arg(about = "Output PNG path (for screenshot)")]
    out: Option<PathBuf>,
    #[arg(default = "default", about = "Session name (for sync-cookies)")]
    session: String,
}

fn run_cdp(app: &CdpCmd, _ctx: &Context) -> Result<Value, AppError> {
    if matches!(app.op, CdpOp::Targets) {
        let targets = CdpClient::list_targets(&app.endpoint)?;
        return Ok(json!({ "endpoint": app.endpoint, "targets": targets }));
    }
    let mut client = CdpClient::connect_http(&app.endpoint, None)?;
    match app.op {
        CdpOp::Targets => unreachable!(),
        CdpOp::Navigate => {
            let url = app
                .url
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("cdp navigate 需要 --url".into()))?;
            client.navigate(url, 300)?;
            let snap = client.snapshot_all_frames()?;
            Ok(json!({
                "url": snap.url,
                "title": snap.title,
                "frame_count": snap.frame_count,
                "elements_count": snap.elements.len(),
                "compact_view": snap.compact_view,
            }))
        }
        CdpOp::Snapshot => {
            let snap = client.snapshot_all_frames()?;
            Ok(serde_json::to_value(snap).unwrap_or_default())
        }
        CdpOp::Fill => {
            let ref_id = app
                .ref_id
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("cdp fill 需要 --ref-id @eN".into()))?;
            let val = app
                .value
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("cdp fill 需要 --value".into()))?;
            let ok = client.fill_ref(ref_id, val)?;
            Ok(json!({ "filled": ok, "ref_id": ref_id }))
        }
        CdpOp::Click => {
            let ref_id = app
                .ref_id
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("cdp click 需要 --ref-id @eN".into()))?;
            let ok = client.click_ref(ref_id)?;
            Ok(json!({ "clicked": ok, "ref_id": ref_id }))
        }
        CdpOp::Screenshot => {
            let out = app
                .out
                .as_deref()
                .unwrap_or_else(|| Path::new("cdp-review.png"));
            let bytes = client.capture_screenshot(out)?;
            Ok(json!({ "saved": out, "bytes": bytes }))
        }
        CdpOp::SyncCookies => {
            let sess = SessionManager::new(&app.session);
            let count = client.sync_cookies_to_session(&sess)?;
            Ok(json!({ "synced_cookies": count, "session": app.session }))
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
enum RecipeOp {
    List,
    Show,
}

#[derive(App)]
#[app(
    name = "recipe",
    run = "run_recipe",
    about = "Inspect learned deterministic site recipes precipitated from approved/submitted drafts"
)]
struct RecipeCmd {
    #[arg(default = "list", about = "Operation: list | show")]
    op: RecipeOp,
    #[arg(about = "Recipe ID (for show)")]
    id: Option<String>,
}

fn run_recipe(app: &RecipeCmd, _ctx: &Context) -> Result<Value, AppError> {
    let store = RecipeStore::new();
    match app.op {
        RecipeOp::List => {
            let list: Vec<Value> = store
                .list()
                .into_iter()
                .map(|r| {
                    json!({
                        "id": r.id,
                        "host": r.host,
                        "command": r.command_name,
                        "bindings": r.bindings.len(),
                        "success_count": r.success_count,
                    })
                })
                .collect();
            Ok(json!({ "recipes_dir": store.root_dir(), "recipes": list }))
        }
        RecipeOp::Show => {
            let id = app
                .id
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("recipe show 需要 --id".into()))?;
            let rec = store.load(id)?;
            Ok(serde_json::to_value(rec).unwrap_or_default())
        }
    }
}

#[derive(App)]
#[app(
    name = "wizard",
    run = "run_wizard",
    safety = "T1",
    about = "Run a multi-step CDP form wizard plan with @vault: placeholders and automatic HumanReviewGate pause + screenshot"
)]
struct WizardCmd {
    #[arg(must_exist = true, about = "Path to WizardPlan JSON or TOML file")]
    plan: PathBuf,
    #[arg(default = "default", about = "Vault profile name")]
    profile: String,
    #[arg(
        about = "Optional Chrome CDP HTTP endpoint (auto-launches headless Chromium if omitted)"
    )]
    endpoint: Option<String>,
    #[arg(default = 0, about = "0-based step index to resume from")]
    start_step: usize,
    #[arg(about = "Explicitly approve passing HumanReviewGate steps")]
    approve_gate: bool,
}

fn run_wizard(app: &WizardCmd, _ctx: &Context) -> Result<Value, AppError> {
    let raw = fs::read_to_string(&app.plan)
        .map_err(|e| AppError::InvalidInput(format!("读取向导计划失败: {e}")))?;
    let plan = WizardPlan::from_str_auto(&raw)?;
    let profile = VaultStore::new().load(&app.profile)?;

    let _managed: Option<ManagedChrome>;
    let mut client = if let Some(ep) = &app.endpoint {
        _managed = None;
        CdpClient::connect_http(ep, None)?
    } else {
        let chrome = ManagedChrome::launch()?;
        let c = CdpClient::connect_http(&chrome.cdp_http, None)?;
        _managed = Some(chrome);
        c
    };

    let report = run_wizard_cdp(
        &mut client,
        &plan,
        &profile,
        app.start_step,
        app.approve_gate,
    )?;
    Ok(serde_json::to_value(report).unwrap_or_default())
}

#[derive(App)]
#[app(
    name = "proofread",
    run = "run_proofread",
    about = "Proofread a Vault profile or SubmissionDraft using GB 32100-2015 (USCC) and GB 11643-1999 (ID card) validators"
)]
struct ProofreadCmd {
    #[arg(default = "default", about = "Vault profile name to proofread")]
    profile: String,
    #[arg(about = "Optional draft ID to proofread instead of a vault profile")]
    draft_id: Option<String>,
    #[arg(default = 2048, range = 0..=102400, about = "Max attachment size in KB (0 disables check)")]
    max_kb: u64,
}

fn run_proofread(app: &ProofreadCmd, _ctx: &Context) -> Result<Value, AppError> {
    if let Some(id) = &app.draft_id {
        let draft = DraftStore::new().load(id)?;
        let report = proofread_draft(&draft);
        return Ok(serde_json::to_value(report).unwrap_or_default());
    }
    let profile = VaultStore::new().load(&app.profile)?;
    let report = proofread_profile(&profile, app.max_kb);
    Ok(serde_json::to_value(report).unwrap_or_default())
}

#[derive(Clone, Debug, ValueEnum)]
enum LicenseOp {
    Plugins,
    Issue,
    Verify,
}

#[derive(App)]
#[app(
    name = "license",
    run = "run_license",
    safety = "T1",
    about = "Manage enterprise plugin catalog and HMAC-SHA256 paid license keys (plugins | issue | verify)"
)]
struct LicenseCmd {
    #[arg(default = "plugins", about = "Operation: plugins | issue | verify")]
    op: LicenseOp,
    #[arg(default = "ent-001", about = "Tenant ID (for issue)")]
    tenant: String,
    #[arg(default = "Demo Enterprise", about = "Company name (for issue)")]
    company: String,
    #[arg(
        default = "vision,proofread,form-review,cdp-wizard",
        about = "Comma-separated enabled plugin IDs or '*' (for issue)"
    )]
    plugins: String,
    #[arg(default = 30, range = 1..=3650, about = "Validity in days (for issue)")]
    days: u64,
    #[arg(
        default = "lain42-dev-license-secret",
        about = "HMAC-SHA256 signing secret (override via LAIN_LICENSE_SECRET in production)"
    )]
    secret: String,
    #[arg(about = "License token L42.<payload>.<sig> (for verify)")]
    token: Option<String>,
}

fn run_license(app: &LicenseCmd, _ctx: &Context) -> Result<Value, AppError> {
    let secret = std::env::var("LAIN_LICENSE_SECRET").unwrap_or_else(|_| app.secret.clone());
    match app.op {
        LicenseOp::Plugins => Ok(json!({ "plugins": builtin_plugins() })),
        LicenseOp::Issue => {
            let now = current_unix_secs();
            let enabled: Vec<String> = app
                .plugins
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let lic = EnterpriseLicense {
                tenant_id: app.tenant.clone(),
                company_name: app.company.clone(),
                enabled_plugins: enabled,
                issued_unix: now,
                expires_unix: now + app.days * 86400,
            };
            let token = issue_license(&secret, &lic)?;
            Ok(json!({ "token": token, "license": lic }))
        }
        LicenseOp::Verify => {
            let token = app
                .token
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("license verify 需要 --token".into()))?;
            let lic = verify_license(&secret, token)?;
            Ok(json!({ "valid": true, "license": lic }))
        }
    }
}

#[cfg(feature = "server")]
#[derive(App)]
#[app(
    name = "gateway",
    run = "run_gateway",
    safety = "T1",
    about = "Start the low-footprint Rust HTTP gateway for lain42.top (natural language chat, plugin entitlements, image crop/compress, proofread, human review inbox, signed download & payment links)"
)]
struct GatewayCmd {
    #[arg(default = "127.0.0.1:8787", about = "Listen address")]
    bind: String,
    #[arg(
        default = "https://lain42.top",
        about = "Public base URL for generated download & payment links"
    )]
    public_url: String,
    #[arg(about = "Require payment order confirmation before unlocking downloads")]
    require_payment: bool,
    #[arg(default = 199, range = 1..=1000000, about = "Price per order in fen (e.g. 199 = ¥1.99)")]
    price_fen: u32,
    #[arg(about = "Require valid enterprise plugin license (X-Lain-License) for paid plugins")]
    require_license: bool,
    #[arg(
        default = "lain42-dev-license-secret",
        about = "HMAC-SHA256 license verification secret"
    )]
    license_secret: String,
}

#[cfg(feature = "server")]
fn run_gateway(app: &GatewayCmd, _ctx: &Context) -> Result<Value, AppError> {
    let secret =
        std::env::var("LAIN_LICENSE_SECRET").unwrap_or_else(|_| app.license_secret.clone());
    let cfg = lilyco_web2cli::GatewayConfig {
        bind_addr: app.bind.clone(),
        public_base_url: app.public_url.clone(),
        require_payment: app.require_payment,
        price_fen: app.price_fen,
        require_license: app.require_license,
        license_secret: secret,
        ..Default::default()
    };
    lilyco_web2cli::serve_gateway(cfg)?;
    Ok(json!({ "stopped": true }))
}

pub fn build_registry(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);
    #[allow(unused_mut)]
    let mut cmds = vec![
        RegisteredCommand::from_app::<OpenCmd>(),
        RegisteredCommand::from_app::<DoCmd>().alias("do"),
        RegisteredCommand::from_app::<FindCmd>(),
        RegisteredCommand::from_app::<ReadCmd>(),
        RegisteredCommand::from_app::<RawCmd>(),
        RegisteredCommand::from_app::<InspectCmd>(),
        RegisteredCommand::from_app::<FromHarCmd>(),
        RegisteredCommand::from_app::<CodegenCmd>(),
        RegisteredCommand::from_app::<SessionCmd>(),
        RegisteredCommand::from_app::<VaultCmd>(),
        RegisteredCommand::from_app::<ImportCookiesCmd>(),
        RegisteredCommand::from_app::<PrepareSubmitCmd>(),
        RegisteredCommand::from_app::<ReviewCmd>(),
        RegisteredCommand::from_app::<CommitSubmitCmd>(),
        RegisteredCommand::from_app::<CdpCmd>(),
        RegisteredCommand::from_app::<RecipeCmd>(),
        RegisteredCommand::from_app::<WizardCmd>(),
        RegisteredCommand::from_app::<ProofreadCmd>(),
        RegisteredCommand::from_app::<LicenseCmd>(),
    ];
    #[cfg(feature = "server")]
    cmds.push(RegisteredCommand::from_app::<GatewayCmd>());
    for c in cmds {
        let name = c.name.clone();
        reg.register(c)
            .unwrap_or_else(|e| panic!("注册命令 `{name}` 失败: {e}"));
    }
    reg
}

fn try_serve_mode() -> Option<()> {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    if raw_args.get(1).and_then(|s| s.to_str()) != Some("serve") {
        return None;
    }
    let Some(source) = raw_args.get(2).and_then(|s| s.to_str()) else {
        eprintln!(
            "Usage: lweb2cli serve <url-or-spec> [subcommand | --schema | --mcp | --tui | --gui]"
        );
        std::process::exit(2);
    };

    let sess = Arc::new(SessionManager::new("default"));
    let spec = match load_or_discover_spec(&sess, source, false) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error loading `{source}`: {e}");
            std::process::exit(1);
        }
    };

    let mut forwarded: Vec<OsString> = Vec::with_capacity(raw_args.len());
    forwarded.push(OsString::from(&spec.name));
    forwarded.extend_from_slice(&raw_args[3..]);

    let has_mcp = forwarded.iter().any(|a| a.to_str() == Some("--mcp"));
    #[cfg(feature = "tui")]
    let has_tui = forwarded.iter().any(|a| a.to_str() == Some("--tui"));
    #[cfg(not(feature = "tui"))]
    let has_tui = false;
    #[cfg(feature = "web")]
    let has_gui = forwarded.iter().any(|a| a.to_str() == Some("--gui"));
    #[cfg(not(feature = "web"))]
    let has_gui = false;

    let backend = if has_mcp {
        lilyco::Backend::Mcp
    } else if has_tui {
        #[cfg(feature = "tui")]
        {
            lilyco::Backend::Tui
        }
        #[cfg(not(feature = "tui"))]
        {
            lilyco::Backend::Cli
        }
    } else if has_gui {
        #[cfg(feature = "web")]
        {
            lilyco::Backend::Web
        }
        #[cfg(not(feature = "web"))]
        {
            lilyco::Backend::Cli
        }
    } else {
        lilyco::Backend::Cli
    };

    let policy: Arc<dyn SafetyPolicy> = match backend {
        lilyco::Backend::Mcp => Arc::new(DenyElevated),
        #[allow(unreachable_patterns)]
        _ => Arc::new(Interactive),
    };

    let reg = spec.to_registry(sess, Some(policy)).unwrap_or_else(|e| {
        eprintln!("Error building registry: {e}");
        std::process::exit(1);
    });

    match backend {
        lilyco::Backend::Cli => lilyco::run_cli_registry_from(&spec.name, reg, forwarded),
        other => lilyco::run_registry_with(&spec.name, reg, other),
    }
    Some(())
}

fn main() {
    if try_serve_mode().is_some() {
        return;
    }
    let backend = lilyco::detect_registry_backend();
    let policy: Arc<dyn SafetyPolicy> = match backend {
        lilyco::Backend::Mcp => Arc::new(DenyElevated),
        #[allow(unreachable_patterns)]
        _ => Arc::new(Interactive),
    };
    let reg = build_registry(policy);
    lilyco::run_registry_with("lweb2cli", reg, backend);
}
