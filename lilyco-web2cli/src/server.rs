//! 高性能、低占用 Rust 商业化服务端网关（面向 `lain42.top` 部署）。
//!
//! 单二进制常驻内存极低（~15MB），无 Python/Node 运行时，直接提供：
//! 1. `GET /` — 自然语言对话 + 图片/证照上传 + 人类审核收件箱一体化中文工作台
//! 2. `POST /api/chat` — 自然语言意图路由（证件照/图片裁剪压缩、国标资料校对、政务填表草稿生成、网页提取）
//! 3. `POST /api/upload` & `GET /downloads/{token}/{filename}` — 安全签名文件上传与下载链接
//! 4. `GET /pay/{order_id}` & `POST /api/orders/{order_id}/confirm` — 可插拔付款链接与核销解锁闭环
//! 5. `GET /api/drafts` & `POST /api/drafts/{id}/approve|reject|commit` — 人类审核工作台 API

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Multipart, Path as AxumPath, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use image::imageops::FilterType;
use lilyco::__core::context::Context;
use lilyco::__core::error::AppError;
use lilyco::__core::theme::{
    AdapterCapabilities, FallbackStrategy, ThemePreset, ThemeSpec, UI_PROTOCOL_VERSION,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::discover_url;
use crate::distill::distill_response;
use crate::plugin::{
    builtin_plugins, check_plugin_entitlement, current_unix_secs, issue_license, verify_license,
    EnterpriseLicense, PLUGIN_FORM_REVIEW, PLUGIN_PROOFREAD, PLUGIN_VISION,
};
use crate::proofread::{proofread_draft, proofread_profile};
use crate::recipe::RecipeStore;
use crate::review::DraftStore;
use crate::session::SessionManager;
use crate::vault::VaultStore;

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub bind_addr: String,
    pub public_base_url: String,
    pub data_dir: PathBuf,
    pub require_payment: bool,
    pub price_fen: u32,
    pub pay_url_template: Option<String>,
    pub require_license: bool,
    pub license_secret: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:8787".to_string(),
            public_base_url: "https://lain42.top".to_string(),
            data_dir: std::env::temp_dir().join("lilyco-lain42-gateway"),
            require_payment: false,
            price_fen: 199,
            pay_url_template: std::env::var("LAIN_PAY_URL_TEMPLATE").ok(),
            require_license: false,
            license_secret: std::env::var("LAIN_LICENSE_SECRET")
                .unwrap_or_else(|_| "lain42-dev-license-secret".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadItem {
    pub token: String,
    pub filename: String,
    pub file_path: PathBuf,
    pub content_type: String,
    pub order_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderItem {
    pub order_id: String,
    pub description: String,
    pub amount_fen: u32,
    pub paid: bool,
    pub pay_url: String,
    pub download_url: Option<String>,
}

#[derive(Debug, Default)]
struct GatewayInner {
    uploads: HashMap<String, PathBuf>,
    downloads: HashMap<String, DownloadItem>,
    orders: HashMap<String, OrderItem>,
    seq: u64,
}

#[derive(Clone)]
pub struct GatewayState {
    pub cfg: GatewayConfig,
    inner: Arc<Mutex<GatewayInner>>,
}

impl GatewayState {
    pub fn new(cfg: GatewayConfig) -> Self {
        let _ = fs::create_dir_all(cfg.data_dir.join("uploads"));
        let _ = fs::create_dir_all(cfg.data_dir.join("outputs"));
        Self {
            cfg,
            inner: Arc::new(Mutex::new(GatewayInner::default())),
        }
    }

    fn next_token(&self, prefix: &str) -> String {
        let mut g = self.inner.lock().expect("gateway lock");
        g.seq += 1;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("{prefix}{ts:x}{:04x}", g.seq)
    }

    fn register_output(
        &self,
        filename: String,
        file_path: PathBuf,
        content_type: String,
        desc: String,
    ) -> (String, Option<OrderItem>) {
        let token = self.next_token("dl_");
        let base = self.cfg.public_base_url.trim_end_matches('/');
        let download_url = format!("{base}/downloads/{token}/{filename}");

        let order = if self.cfg.require_payment {
            let order_id = self.next_token("ord_");
            let pay_url = match &self.cfg.pay_url_template {
                Some(tpl) => tpl
                    .replace("{order_id}", &order_id)
                    .replace("{amount_fen}", &self.cfg.price_fen.to_string()),
                None => format!("{base}/pay/{order_id}"),
            };
            let item = OrderItem {
                order_id: order_id.clone(),
                description: desc,
                amount_fen: self.cfg.price_fen,
                paid: false,
                pay_url,
                download_url: Some(download_url.clone()),
            };
            let mut g = self.inner.lock().expect("gateway lock");
            g.orders.insert(order_id.clone(), item.clone());
            g.downloads.insert(
                token.clone(),
                DownloadItem {
                    token,
                    filename,
                    file_path,
                    content_type,
                    order_id: Some(order_id),
                },
            );
            Some(item)
        } else {
            let mut g = self.inner.lock().expect("gateway lock");
            g.downloads.insert(
                token.clone(),
                DownloadItem {
                    token,
                    filename,
                    file_path,
                    content_type,
                    order_id: None,
                },
            );
            None
        };

        (download_url, order)
    }

    pub fn vault_store(&self) -> VaultStore {
        VaultStore::with_dir(self.cfg.data_dir.join("vault"))
    }

    pub fn draft_store(&self) -> DraftStore {
        DraftStore::with_dir(self.cfg.data_dir.join("drafts"))
    }

    pub fn recipe_store(&self) -> RecipeStore {
        RecipeStore::with_dir(self.cfg.data_dir.join("recipes"))
    }

    pub fn session_manager(&self, name: &str) -> SessionManager {
        SessionManager::with_dir(name, self.cfg.data_dir.join("sessions"))
    }
}

pub fn build_router(state: GatewayState) -> Router {
    Router::new()
        .route("/", get(index_page))
        .route("/ui-gallery", get(ui_gallery_page))
        .route("/llms.txt", get(llms_txt))
        .route("/.well-known/agent.json", get(agent_card))
        .route("/openapi.json", get(openapi_spec))
        .route("/mcp", post(remote_mcp_rpc))
        .route("/api/health", get(health))
        .route("/api/themes", get(list_themes))
        .route("/api/themes/{id}", get(theme_css))
        .route("/api/capabilities", get(adapter_capabilities))
        .route("/api/plugins", get(list_plugins))
        .route("/api/license/verify", post(verify_license_endpoint))
        .route("/api/admin/license/issue", post(admin_issue_license))
        .route("/api/upload", post(upload_file))
        .route("/api/chat", post(chat_dispatch))
        .route("/downloads/{token}/{filename}", get(download_file))
        .route("/pay/{order_id}", get(pay_page))
        .route("/api/orders/{order_id}/confirm", post(confirm_order))
        .route("/api/drafts", get(list_drafts))
        .route("/api/drafts/{id}/approve", post(approve_draft))
        .route("/api/drafts/{id}/reject", post(reject_draft))
        .route("/api/drafts/{id}/commit", post(commit_draft))
        .with_state(state)
}

pub fn serve_gateway(cfg: GatewayConfig) -> Result<(), AppError> {
    let bind_addr = cfg.bind_addr.clone();
    let public_url = cfg.public_base_url.clone();
    let state = GatewayState::new(cfg);
    let app = build_router(state);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::Runtime(format!("创建 tokio 运行时失败: {e}")))?;

    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| AppError::Runtime(format!("绑定 `{bind_addr}` 失败: {e}")))?;
        eprintln!("lilyco-web2cli gateway listening on http://{bind_addr} (public: {public_url})");
        axum::serve(listener, app)
            .await
            .map_err(|e| AppError::Runtime(format!("网关服务异常退出: {e}")))
    })
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "service": "lilyco-web2cli-gateway" }))
}

async fn llms_txt(State(state): State<GatewayState>) -> Response {
    let base = state.cfg.public_base_url.trim_end_matches('/');
    let body = format!(
        r#"# Lain42 Web2CLI Workbench (AI-Native Gateway)

> Base URL: {base}
> Engine: Lilyco Rust (lilyco-web2cli, ~3.5MB RSS)
> UI Protocol: v{proto}

## Agent Discovery
- Agent Card (A2A): {base}/.well-known/agent.json
- OpenAPI 3.1 Spec: {base}/openapi.json
- Remote MCP (HTTP JSON-RPC 2024-11-05): POST {base}/mcp
- Themes & UI Capabilities: GET {base}/api/themes , GET {base}/api/capabilities

## Core Endpoints
- POST {base}/api/upload (multipart/form-data) -> upload image/document, returns `upload_id`
- POST {base}/api/chat (JSON: `{{ "message", "upload_id?", "profile?", "license_key?" }}`) -> natural-language router for image crop/compress, GB 32100/11643 proofreading, and government form draft preparation
- GET  {base}/api/drafts -> list PendingReview / Approved / Submitted drafts (Human-in-the-Loop)
- GET  {base}/api/plugins -> list commercial plugins (`vision`, `proofread`, `form-review`, `cdp-wizard`) and entitlement status

## Authentication & Safety
- Pass enterprise license header `X-Lain-License: L42.<payload>.<sig>` when `--require-license` is enabled.
- Government/enterprise form submissions NEVER bypass human review: AI prepares drafts (`PendingReview`), humans approve before commit.
"#,
        proto = UI_PROTOCOL_VERSION
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

async fn agent_card(State(state): State<GatewayState>) -> Json<Value> {
    let base = state.cfg.public_base_url.trim_end_matches('/');
    Json(json!({
        "name": "Lain42 Web2CLI Workbench",
        "description": "AI-native Rust gateway for image/ID-photo cropping, GB 32100/11643 proofreading, and human-reviewed government/enterprise form preparation.",
        "url": base,
        "version": env!("CARGO_PKG_VERSION"),
        "protocol_version": UI_PROTOCOL_VERSION,
        "endpoints": {
            "mcp_http": format!("{base}/mcp"),
            "openapi": format!("{base}/openapi.json"),
            "llms_txt": format!("{base}/llms.txt"),
            "chat": format!("{base}/api/chat"),
            "themes": format!("{base}/api/themes"),
            "ui_gallery": format!("{base}/ui-gallery"),
        },
        "authentication": {
            "type": "header",
            "header": "X-Lain-License",
            "format": "L42.<base64url(payload)>.<base64url(hmac_sha256)>"
        },
        "skills": [
            { "id": "vision", "name": "Image & ID Photo Crop/Compress" },
            { "id": "proofread", "name": "GB 32100/11643 Enterprise Data Proofread" },
            { "id": "form-review", "name": "Form Draft Preparation & Human Review Inbox" },
            { "id": "cdp-wizard", "name": "Multi-Step Browser Wizard with Human Verification Handoff" }
        ]
    }))
}

async fn openapi_spec(State(state): State<GatewayState>) -> Json<Value> {
    let base = state.cfg.public_base_url.trim_end_matches('/');
    Json(json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Lain42 Web2CLI AI-Native Gateway",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "servers": [{ "url": base }],
        "paths": {
            "/api/health": { "get": { "operationId": "health", "summary": "Service health check" } },
            "/api/plugins": { "get": { "operationId": "listPlugins", "summary": "List commercial plugins and license unlock status" } },
            "/api/themes": { "get": { "operationId": "listThemes", "summary": "List 10 built-in UI framework ThemeSpecs" } },
            "/api/capabilities": { "get": { "operationId": "getCapabilities", "summary": "Get UI protocol adapter capabilities" } },
            "/api/upload": { "post": { "operationId": "uploadFile", "summary": "Upload image or attachment via multipart/form-data" } },
            "/api/chat": {
                "post": {
                    "operationId": "chatDispatch",
                    "summary": "Natural-language task dispatcher (image crop, proofread, form draft)",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["message"],
                                    "properties": {
                                        "message": { "type": "string" },
                                        "upload_id": { "type": "string" },
                                        "profile": { "type": "string" },
                                        "license_key": { "type": "string" }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/api/drafts": { "get": { "operationId": "listDrafts", "summary": "List human-in-the-loop submission drafts" } },
            "/mcp": { "post": { "operationId": "remoteMcp", "summary": "Remote MCP 2024-11-05 JSON-RPC endpoint" } }
        }
    }))
}

/// Remote HTTP MCP 2024-11-05 JSON-RPC 端点（供 Claude / Cursor / Codex / Dify 远程直连）。
async fn remote_mcp_rpc(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> Response {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "lain42-web2cli-gateway",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }))
        .into_response(),
        "tools/list" => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [
                    {
                        "name": "proofread_profile",
                        "description": "Proofread an enterprise Vault profile against GB 32100-2015 (USCC) and GB 11643-1999 (ID card)",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "profile": { "type": "string", "default": "default" } }
                        }
                    },
                    {
                        "name": "list_plugins",
                        "description": "List available commercial plugins and entitlement status",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "list_drafts",
                        "description": "List PendingReview / Approved / Submitted form drafts",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "list_themes",
                        "description": "List 10 built-in UI framework ThemeSpecs (shadcn, AntD, MUI, Element Plus, etc.)",
                        "inputSchema": { "type": "object", "properties": {} }
                    }
                ]
            }
        }))
        .into_response(),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let tool_result = match name {
                "proofread_profile" => {
                    if let Err(resp) = enforce_plugin(&state, &headers, None, PLUGIN_PROOFREAD) {
                        return *resp;
                    }
                    let prof_name = args.get("profile").and_then(Value::as_str).unwrap_or("default");
                    match state.vault_store().load(prof_name) {
                        Ok(p) => json!(proofread_profile(&p, 2048)),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                "list_plugins" => json!({ "plugins": builtin_plugins() }),
                "list_drafts" => json!({ "drafts": state.draft_store().list() }),
                "list_themes" => {
                    let themes: Vec<ThemeSpec> = ThemePreset::all_builtins()
                        .iter()
                        .cloned()
                        .map(ThemeSpec::from_preset)
                        .collect();
                    json!({ "themes": themes })
                }
                other => {
                    return Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": format!("Unknown tool: {other}") }
                    }))
                    .into_response()
                }
            };
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": tool_result.to_string() }]
                }
            }))
            .into_response()
        }
        other => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("Method not found: {other}") }
        }))
        .into_response(),
    }
}

async fn list_themes() -> Json<Value> {
    let themes: Vec<ThemeSpec> = ThemePreset::all_builtins()
        .iter()
        .cloned()
        .map(ThemeSpec::from_preset)
        .collect();
    Json(json!({
        "ok": true,
        "protocol_version": UI_PROTOCOL_VERSION,
        "themes": themes,
    }))
}

async fn theme_css(AxumPath(id): AxumPath<String>) -> Response {
    let clean = id.trim_end_matches(".css");
    let Some(preset) = ThemePreset::all_builtins()
        .iter()
        .find(|p| p.id().eq_ignore_ascii_case(clean))
        .cloned()
    else {
        return (StatusCode::NOT_FOUND, "Theme preset not found").into_response();
    };
    let css = ThemeSpec::from_preset(preset).to_css_vars();
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], css).into_response()
}

async fn adapter_capabilities() -> Json<Value> {
    let caps = AdapterCapabilities {
        adapter_id: "lain42-workbench".into(),
        framework_name: "Lain42 Multi-Theme Web (shadcn/ui default)".into(),
        protocol_version: UI_PROTOCOL_VERSION.into(),
        supports_dark_mode: true,
        supports_density: true,
        fallback: FallbackStrategy::DegradeToCard,
        supported_kinds: vec![
            "card".into(),
            "tabs".into(),
            "table".into(),
            "alert".into(),
            "badge".into(),
            "progress".into(),
            "dialog".into(),
            "stepper".into(),
        ],
    };
    Json(json!({ "ok": true, "capabilities": caps }))
}

async fn list_plugins(State(state): State<GatewayState>, headers: HeaderMap) -> Json<Value> {
    let token = headers.get("x-lain-license").and_then(|v| v.to_str().ok());
    let verified = token.and_then(|t| verify_license(&state.cfg.license_secret, t).ok());
    let plugins: Vec<Value> = builtin_plugins()
        .into_iter()
        .map(|p| {
            let unlocked = !state.cfg.require_license
                || verified.as_ref().is_some_and(|lic| lic.has_plugin(&p.id));
            json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "price_fen_monthly": p.price_fen_monthly,
                "unlocked": unlocked,
            })
        })
        .collect();
    Json(json!({
        "ok": true,
        "require_license": state.cfg.require_license,
        "tenant": verified.as_ref().map(|l| &l.company_name),
        "plugins": plugins,
    }))
}

#[derive(Debug, Deserialize)]
struct VerifyLicenseReq {
    token: String,
}

async fn verify_license_endpoint(
    State(state): State<GatewayState>,
    Json(req): Json<VerifyLicenseReq>,
) -> Response {
    match verify_license(&state.cfg.license_secret, &req.token) {
        Ok(lic) => Json(json!({ "ok": true, "license": lic })).into_response(),
        Err(e) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct AdminIssueLicenseReq {
    tenant_id: String,
    company_name: String,
    enabled_plugins: Vec<String>,
    #[serde(default = "default_days")]
    days: u64,
}

fn default_days() -> u64 {
    30
}

async fn admin_issue_license(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<AdminIssueLicenseReq>,
) -> Response {
    let expected =
        std::env::var("LAIN_ADMIN_SECRET").unwrap_or_else(|_| state.cfg.license_secret.clone());
    let provided = headers
        .get("x-admin-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if provided.is_empty() || provided != expected {
        return (
            StatusCode::UNAUTHORIZED,
            Json(
                json!({ "ok": false, "error": "管理员密钥校验失败（需提供有效 X-Admin-Secret）" }),
            ),
        )
            .into_response();
    }
    let now = current_unix_secs();
    let lic = EnterpriseLicense {
        tenant_id: req.tenant_id,
        company_name: req.company_name,
        enabled_plugins: req.enabled_plugins,
        issued_unix: now,
        expires_unix: now + req.days.max(1) * 86400,
    };
    match issue_license(&state.cfg.license_secret, &lic) {
        Ok(token) => Json(json!({ "ok": true, "token": token, "license": lic })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

fn enforce_plugin(
    state: &GatewayState,
    headers: &HeaderMap,
    body_token: Option<&str>,
    plugin_id: &str,
) -> Result<(), Box<Response>> {
    if !state.cfg.require_license {
        return Ok(());
    }
    let token = headers
        .get("x-lain-license")
        .and_then(|v| v.to_str().ok())
        .or(body_token);
    if let Err(e) = check_plugin_entitlement(&state.cfg.license_secret, token, plugin_id) {
        let base = state.cfg.public_base_url.trim_end_matches('/');
        let resp = (
            StatusCode::PAYMENT_REQUIRED,
            Json(json!({
                "ok": false,
                "error": e.to_string(),
                "required_plugin": plugin_id,
                "upgrade_url": format!("{base}/api/plugins"),
            })),
        )
            .into_response();
        return Err(Box::new(resp));
    }
    Ok(())
}

async fn upload_file(State(state): State<GatewayState>, mut multipart: Multipart) -> Response {
    if let Ok(Some(field)) = multipart.next_field().await {
        let filename = field
            .file_name()
            .unwrap_or("upload.bin")
            .replace(['/', '\\'], "_");
        let Ok(bytes) = field.bytes().await else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": "读取上传文件失败" })),
            )
                .into_response();
        };
        let id = state.next_token("up_");
        let dest = state
            .cfg
            .data_dir
            .join("uploads")
            .join(format!("{id}_{filename}"));
        if let Err(e) = fs::write(&dest, &bytes) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": format!("保存文件失败: {e}") })),
            )
                .into_response();
        }
        state
            .inner
            .lock()
            .expect("gateway lock")
            .uploads
            .insert(id.clone(), dest);
        return Json(json!({
            "ok": true,
            "upload_id": id,
            "filename": filename,
            "bytes": bytes.len(),
        }))
        .into_response();
    }
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "ok": false, "error": "未收到 multipart 文件字段" })),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    upload_id: Option<String>,
    #[serde(default)]
    license_key: Option<String>,
    #[serde(default = "default_profile")]
    profile: String,
    #[serde(default = "default_profile")]
    session: String,
}

fn default_profile() -> String {
    "default".to_string()
}

async fn chat_dispatch(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Response {
    let msg = req.message.trim();
    if msg.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "message 不能为空" })),
        )
            .into_response();
    }

    // 1. 图片裁剪 / 证件照 / 压缩处理意图（需要 `vision` 插件授权）
    if msg.contains("裁剪")
        || msg.contains("寸照")
        || msg.contains("缩放")
        || msg.contains("压缩")
        || msg.contains("图片")
        || msg.to_ascii_lowercase().contains("crop")
        || msg.to_ascii_lowercase().contains("resize")
    {
        if let Err(resp) =
            enforce_plugin(&state, &headers, req.license_key.as_deref(), PLUGIN_VISION)
        {
            return *resp;
        }
        return handle_image_intent(&state, msg, req.upload_id.as_deref());
    }

    // 2. 资料校对意图（需要 `proofread` 插件授权）
    if msg.contains("校对") || msg.contains("核验") || msg.contains("校验") {
        if let Err(resp) = enforce_plugin(
            &state,
            &headers,
            req.license_key.as_deref(),
            PLUGIN_PROOFREAD,
        ) {
            return *resp;
        }
        let profile = match state.vault_store().load(&req.profile) {
            Ok(p) => p,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "ok": false, "error": e.to_string() })),
                )
                    .into_response()
            }
        };
        let report = proofread_profile(&profile, 2048);
        return Json(json!({
            "ok": true,
            "intent": "proofread",
            "profile": req.profile,
            "reply": if report.passed {
                format!("资料 `{}` 校对通过（共核验 {} 项，0 项错误）。", req.profile, report.checked_count)
            } else {
                format!("资料 `{}` 校对发现 {} 项问题，请核对后修正。", req.profile, report.issues.len())
            },
            "report": report,
        }))
        .into_response();
    }

    // 3. 政务填表 / 生成待审核草稿意图（包含 URL，需要 `form-review` 插件授权）
    if let Some(url) = extract_first_url(msg) {
        if msg.contains("填表")
            || msg.contains("草稿")
            || msg.contains("申报")
            || msg.contains("登记")
        {
            if let Err(resp) = enforce_plugin(
                &state,
                &headers,
                req.license_key.as_deref(),
                PLUGIN_FORM_REVIEW,
            ) {
                return *resp;
            }
            let sess = state.session_manager(&req.session);
            let spec = match discover_url(&sess, &url, false) {
                Ok(s) => s,
                Err(e) => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({ "ok": false, "error": e.to_string() })),
                    )
                        .into_response()
                }
            };
            let Some(cmd) = spec
                .commands
                .iter()
                .find(|c| !matches!(c.name.as_str(), "read-page" | "links" | "tables"))
                .cloned()
            else {
                return Json(json!({
                    "ok": true,
                    "intent": "inspect",
                    "reply": format!("已分析 {url}，未发现标准 HTML 表单；建议使用 CDP 模式操作 SPA 页面。"),
                    "commands": spec.commands.len(),
                }))
                .into_response();
            };
            let profile = state.vault_store().load(&req.profile).unwrap_or_default();
            let rec = state.recipe_store().find_matching(&cmd.url, &cmd.name);
            let draft = match state.draft_store().prepare_draft_with_recipe(
                &profile,
                &req.session,
                &spec.base_url,
                cmd,
                rec.as_ref(),
                &Map::new(),
            ) {
                Ok(d) => d,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "ok": false, "error": e.to_string() })),
                    )
                        .into_response()
                }
            };
            let check = proofread_draft(&draft);
            return Json(json!({
                "ok": true,
                "intent": "prepare_submit",
                "reply": format!("已自动生成待人类审核草稿 `{}`（绝未直接提交），请在右侧审核收件箱核对并批准。", draft.id),
                "draft_id": draft.id,
                "status": draft.status,
                "filled_args": draft.args,
                "missing_required": draft.missing_required,
                "proofread": check,
            }))
            .into_response();
        }

        // 默认网页蒸馏阅读
        let sess = state.session_manager(&req.session);
        return match sess.request("GET", &url, &Default::default(), None, false) {
            Ok(resp) => {
                let page = distill_response(&resp.final_url, &resp.content_type, &resp.body, 3000);
                Json(json!({
                    "ok": true,
                    "intent": "open_url",
                    "reply": format!("已提取 `{}`（{} 个可交互项，{} 张表格）。", page.title, page.actions.len(), page.tables.len()),
                    "title": page.title,
                    "compact_view": page.compact_view,
                }))
                .into_response()
            }
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "ok": false, "error": e.to_string() })),
            )
                .into_response(),
        };
    }

    // 4. 审核草稿查询意图
    if msg.contains("审核") || msg.contains("草稿") {
        let list = state.draft_store().list();
        return Json(json!({
            "ok": true,
            "intent": "list_drafts",
            "reply": format!("当前共有 {} 份草稿等待或完成审核。", list.len()),
            "drafts": list,
        }))
        .into_response();
    }

    Json(json!({
        "ok": true,
        "intent": "help",
        "reply": "支持直接输入自然语言：1) 上传图片后说「裁剪成一寸照 295x413 并压缩」获取下载与付款链接；2) 「校对 default 企业资料」执行国标信用代码/身份证号核验；3) 「给 https://... 填表生成审核草稿」自动预填并进入人类审核收件箱。"
    }))
    .into_response()
}

fn handle_image_intent(state: &GatewayState, msg: &str, upload_id: Option<&str>) -> Response {
    let Some(uid) = upload_id else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "请先上传图片文件（upload_id）再执行裁剪/压缩" })),
        )
            .into_response();
    };
    let src_path = {
        state
            .inner
            .lock()
            .expect("gateway lock")
            .uploads
            .get(uid)
            .cloned()
    };
    let Some(src_path) = src_path else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": format!("未找到上传文件 `{uid}`") })),
        )
            .into_response();
    };

    let img = match image::open(&src_path) {
        Ok(i) => i,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": format!("无法解析图片: {e}") })),
            )
                .into_response()
        }
    };

    let (target_w, target_h) = parse_dimensions(msg).unwrap_or_else(|| {
        if msg.contains("二寸") {
            (413, 579)
        } else if msg.contains("一寸") || msg.contains("寸照") {
            (295, 413)
        } else {
            (img.width().min(1200), img.height().min(1200))
        }
    });

    // 居中裁剪并缩放到目标尺寸（适合证件照与营业执照扫描件标准化）
    let processed = img.resize_to_fill(target_w, target_h, FilterType::Lanczos3);
    let out_name = format!("processed_{target_w}x{target_h}.jpg");
    let out_file = state
        .cfg
        .data_dir
        .join("outputs")
        .join(format!("{}_{out_name}", state.next_token("img_")));

    if let Err(e) = processed.to_rgb8().save(&out_file) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": format!("保存处理后图片失败: {e}") })),
        )
            .into_response();
    }
    let out_size = fs::metadata(&out_file).map(|m| m.len()).unwrap_or(0);
    let (download_url, order) = state.register_output(
        out_name.clone(),
        out_file,
        "image/jpeg".to_string(),
        format!("图片裁剪处理 ({target_w}x{target_h})"),
    );

    let pay_url = order.as_ref().map(|o| o.pay_url.clone());
    let order_id = order.as_ref().map(|o| o.order_id.clone());
    let reply = if let Some(pu) = &pay_url {
        format!(
            "已完成图片裁剪与压缩（{target_w}x{target_h}，{} KB）。付款链接：{pu} ；付款后下载链接：{download_url}",
            out_size.div_ceil(1024)
        )
    } else {
        format!(
            "已完成图片裁剪与压缩（{target_w}x{target_h}，{} KB）。下载链接：{download_url}",
            out_size.div_ceil(1024)
        )
    };

    Json(json!({
        "ok": true,
        "intent": "image_process",
        "width": target_w,
        "height": target_h,
        "bytes": out_size,
        "download_url": download_url,
        "pay_url": pay_url,
        "order_id": order_id,
        "reply": reply,
    }))
    .into_response()
}

async fn download_file(
    State(state): State<GatewayState>,
    AxumPath((token, _filename)): AxumPath<(String, String)>,
) -> Response {
    let item = {
        state
            .inner
            .lock()
            .expect("gateway lock")
            .downloads
            .get(&token)
            .cloned()
    };
    let Some(item) = item else {
        return (StatusCode::NOT_FOUND, "Download link not found or expired").into_response();
    };

    if let Some(oid) = &item.order_id {
        let order = {
            state
                .inner
                .lock()
                .expect("gateway lock")
                .orders
                .get(oid)
                .cloned()
        };
        if let Some(o) = order {
            if !o.paid {
                return (
                    StatusCode::PAYMENT_REQUIRED,
                    Json(json!({
                        "ok": false,
                        "error": "该文件需完成付款后下载",
                        "order_id": o.order_id,
                        "pay_url": o.pay_url,
                    })),
                )
                    .into_response();
            }
        }
    }

    match fs::read(&item.file_path) {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, item.content_type),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", item.filename),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "File missing on disk").into_response(),
    }
}

async fn pay_page(
    State(state): State<GatewayState>,
    AxumPath(order_id): AxumPath<String>,
) -> Response {
    let order = {
        state
            .inner
            .lock()
            .expect("gateway lock")
            .orders
            .get(&order_id)
            .cloned()
    };
    let Some(o) = order else {
        return (StatusCode::NOT_FOUND, "Order not found").into_response();
    };
    let yuan = f64::from(o.amount_fen) / 100.0;
    let html = format!(
        r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>订单支付 - {oid}</title>
<style>body{{font-family:system-ui,sans-serif;max-width:480px;margin:48px auto;padding:24px;background:#0f172a;color:#f8fafc}}
.card{{background:#1e293b;padding:24px;border-radius:12px;border:1px solid #334155}}
button{{background:#10b981;color:#fff;border:none;padding:12px 20px;border-radius:8px;cursor:pointer;font-size:15px;width:100%}}
a{{color:#38bdf8}}</style></head><body><div class="card">
<h2>订单收银台</h2><p>订单号：{oid}</p><p>服务：{desc}</p><p>金额：<b>¥{yuan:.2}</b></p>
<p id="st">状态：{st}</p>
<button onclick="fetch('/api/orders/{oid}/confirm',{{method:'POST'}}).then(r=>r.json()).then(d=>{{document.getElementById('st').innerHTML='状态：已支付！<a href='+d.download_url+'>立即下载</a>';}})">确认支付并解锁下载</button>
</div></body></html>"#,
        oid = o.order_id,
        desc = o.description,
        st = if o.paid { "已支付" } else { "待支付" }
    );
    Html(html).into_response()
}

async fn confirm_order(
    State(state): State<GatewayState>,
    AxumPath(order_id): AxumPath<String>,
) -> Response {
    let mut g = state.inner.lock().expect("gateway lock");
    let Some(order) = g.orders.get_mut(&order_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "订单不存在" })),
        )
            .into_response();
    };
    order.paid = true;
    Json(json!({
        "ok": true,
        "order_id": order.order_id,
        "paid": true,
        "download_url": order.download_url,
    }))
    .into_response()
}

async fn list_drafts(State(state): State<GatewayState>) -> Json<Value> {
    Json(json!({ "ok": true, "drafts": state.draft_store().list() }))
}

async fn approve_draft(
    State(state): State<GatewayState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state
        .draft_store()
        .approve(&id, Some("Web 工作台人工审核批准".into()))
    {
        Ok(d) => Json(json!({ "ok": true, "draft": d })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn reject_draft(
    State(state): State<GatewayState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state
        .draft_store()
        .reject(&id, Some("Web 工作台人工驳回".into()))
    {
        Ok(d) => Json(json!({ "ok": true, "draft": d })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn commit_draft(
    State(state): State<GatewayState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let store = state.draft_store();
    let preview = match store.load(&id) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "ok": false, "error": e.to_string() })),
            )
                .into_response()
        }
    };
    let sess = state.session_manager(&preview.session);
    let (tx, _rx) = std::sync::mpsc::channel();
    let ctx = Context::new_test(tx);
    match store.commit_draft(&id, &ctx, &sess) {
        Ok(submitted) => {
            let _ = state
                .vault_store()
                .load(&submitted.profile)
                .ok()
                .and_then(|p| state.recipe_store().learn_from_draft(&submitted, &p).ok());
            Json(json!({ "ok": true, "draft": submitted })).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn index_page() -> Html<&'static str> {
    Html(include_str!("workbench.html"))
}

async fn ui_gallery_page() -> Html<&'static str> {
    Html(include_str!("ui_gallery.html"))
}

fn parse_dimensions(s: &str) -> Option<(u32, u32)> {
    let re = Regex::new(r"(\d{2,4})\s*[xX×*]\s*(\d{2,4})").ok()?;
    let cap = re.captures(s)?;
    let w = cap.get(1)?.as_str().parse::<u32>().ok()?;
    let h = cap.get(2)?.as_str().parse::<u32>().ok()?;
    if (32..=4096).contains(&w) && (32..=4096).contains(&h) {
        Some((w, h))
    } else {
        None
    }
}

fn extract_first_url(s: &str) -> Option<String> {
    let re = Regex::new(r"https?://[^\s\u{4e00}-\u{9fff}]+").ok()?;
    re.find(s).map(|m| m.as_str().to_string())
}

#[allow(dead_code)]
fn _ensure_path_import(_: &Path) {}
