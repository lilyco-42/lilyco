//! Axum 0.8 路由层：完整复刻 `lain42.top` 六大子页（Askama 编译期 SSR）+ 全功能 Turso API + AI 原生协议。

use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use lilyco::__core::theme::{ThemePreset, ThemeSpec};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::db::{AiModel, Announcement, DbHealth, PortalDb};
use crate::studio_api::studio_router;

#[derive(Clone)]
pub struct AppState {
    pub db: PortalDb,
    pub env_name: String,
    pub license_secret: String,
    pub static_dir: Option<String>,
    pub pay_config_dir: Option<String>,
}

struct ShellCtx {
    env_label: String,
    theme_css: String,
}

fn build_shell(st: &AppState, theme_q: Option<&str>) -> ShellCtx {
    let preset = theme_q
        .and_then(|id| {
            ThemePreset::all_builtins()
                .iter()
                .find(|p| p.id() == id || p.id().starts_with(id))
                .cloned()
        })
        .unwrap_or(ThemePreset::ShadcnZinc);
    let spec = ThemeSpec::from_preset(preset);
    let env_label = match st.env_name.as_str() {
        "test" => "TEST".to_string(),
        "dev" => "DEV".to_string(),
        other => other.to_uppercase(),
    };
    ShellCtx {
        env_label,
        theme_css: spec.to_css_vars(),
    }
}

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage {
    page_title: &'static str,
    active_nav: &'static str,
    env_label: String,
    theme_css: String,
    health: DbHealth,
    announcements: Vec<Announcement>,
}

#[derive(Template)]
#[template(path = "models.html")]
struct ModelsPage {
    page_title: &'static str,
    active_nav: &'static str,
    env_label: String,
    theme_css: String,
    models: Vec<AiModel>,
}

#[derive(Template)]
#[template(path = "shop.html")]
struct ShopPage {
    page_title: &'static str,
    active_nav: &'static str,
    env_label: String,
    theme_css: String,
}

#[derive(Template)]
#[template(path = "account.html")]
struct AccountPage {
    page_title: &'static str,
    active_nav: &'static str,
    env_label: String,
    theme_css: String,
}

#[derive(Template)]
#[template(path = "docs.html")]
struct DocsPage {
    page_title: &'static str,
    active_nav: &'static str,
    env_label: String,
    theme_css: String,
}

#[derive(Template)]
#[template(path = "tool.html")]
struct ToolPage {
    page_title: &'static str,
    active_nav: &'static str,
    env_label: String,
    theme_css: String,
}

#[derive(Template)]
#[template(path = "vpn.html")]
struct VpnPage {
    page_title: &'static str,
    active_nav: &'static str,
    env_label: String,
    theme_css: String,
}

#[derive(Deserialize)]
struct PageQuery {
    theme: Option<String>,
}

#[derive(Deserialize)]
struct CreateOrderReq {
    product_id: String,
    buyer_email: String,
}

#[derive(Deserialize)]
struct AccountLookupReq {
    email: String,
}

#[derive(Deserialize)]
struct AddAnnouncementReq {
    published_date: String,
    title: String,
    summary: String,
    #[serde(default = "default_link")]
    link_url: String,
    #[serde(default)]
    pinned: bool,
}

fn default_link() -> String {
    "/".to_string()
}

#[derive(Deserialize)]
struct VerifyLicenseReq {
    token: String,
}

#[derive(Deserialize)]
struct McpReq {
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let api_and_ai = Router::new()
        .merge(studio_router())
        .route("/api/health", get(api_health))
        .route(
            "/api/announcements",
            get(api_announcements).post(api_add_announcement),
        )
        .route("/api/models", get(api_models))
        .route("/api/products", get(api_products))
        .route("/api/services", get(api_services))
        .route("/api/orders", get(api_list_orders).post(api_create_order))
        .route("/api/orders/{id}/confirm", post(api_confirm_order))
        .route("/api/accounts/lookup", post(api_account_lookup))
        .route("/api/license/verify", post(api_verify_license))
        .route("/api/themes", get(api_themes))
        .route("/llms.txt", get(llms_txt))
        .route("/.well-known/agent.json", get(agent_json))
        .route("/openapi.json", get(openapi_json))
        .route("/mcp", post(mcp_endpoint));

    let router = if let Some(dir) = state.static_dir.clone() {
        // 挂载原版 lain42.top 完整静态前端（含 /shop/ 付款界面、/account/、/models/、/docs/、/tool/、/vpn/、/go.html）
        api_and_ai.fallback_service(ServeDir::new(dir).append_index_html_on_directories(true))
    } else {
        api_and_ai
            .route("/", get(home_page))
            .route("/models", get(models_page))
            .route("/models/", get(models_page))
            .route("/shop", get(shop_page))
            .route("/shop/", get(shop_page))
            .route("/account", get(account_page))
            .route("/account/", get(account_page))
            .route("/docs", get(docs_page))
            .route("/docs/", get(docs_page))
            .route("/tool", get(tool_page))
            .route("/tool/", get(tool_page))
            .route("/vpn", get(vpn_page))
            .route("/vpn/", get(vpn_page))
    };

    router
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn home_page(
    State(st): State<Arc<AppState>>,
    Query(q): Query<PageQuery>,
) -> Result<Html<String>, (StatusCode, String)> {
    let sh = build_shell(&st, q.theme.as_deref());
    let tpl = HomePage {
        page_title: "首页",
        active_nav: "home",
        env_label: sh.env_label,
        theme_css: sh.theme_css,
        health: st.db.health().await.map_err(internal_err)?,
        announcements: st.db.list_announcements().await.map_err(internal_err)?,
    };
    render_tpl(tpl)
}

async fn models_page(
    State(st): State<Arc<AppState>>,
    Query(q): Query<PageQuery>,
) -> Result<Html<String>, (StatusCode, String)> {
    let sh = build_shell(&st, q.theme.as_deref());
    let tpl = ModelsPage {
        page_title: "AI API 模型与价格",
        active_nav: "models",
        env_label: sh.env_label,
        theme_css: sh.theme_css,
        models: st.db.list_ai_models().await.map_err(internal_err)?,
    };
    render_tpl(tpl)
}

async fn shop_page(
    State(st): State<Arc<AppState>>,
    Query(q): Query<PageQuery>,
) -> Result<Html<String>, (StatusCode, String)> {
    let sh = build_shell(&st, q.theme.as_deref());
    render_tpl(ShopPage {
        page_title: "商城与积分收银台",
        active_nav: "shop",
        env_label: sh.env_label,
        theme_css: sh.theme_css,
    })
}

async fn account_page(
    State(st): State<Arc<AppState>>,
    Query(q): Query<PageQuery>,
) -> Result<Html<String>, (StatusCode, String)> {
    let sh = build_shell(&st, q.theme.as_deref());
    render_tpl(AccountPage {
        page_title: "我的账户控制台",
        active_nav: "account",
        env_label: sh.env_label,
        theme_css: sh.theme_css,
    })
}

async fn docs_page(
    State(st): State<Arc<AppState>>,
    Query(q): Query<PageQuery>,
) -> Result<Html<String>, (StatusCode, String)> {
    let sh = build_shell(&st, q.theme.as_deref());
    render_tpl(DocsPage {
        page_title: "API 调用文档",
        active_nav: "docs",
        env_label: sh.env_label,
        theme_css: sh.theme_css,
    })
}

async fn tool_page(
    State(st): State<Arc<AppState>>,
    Query(q): Query<PageQuery>,
) -> Result<Html<String>, (StatusCode, String)> {
    let sh = build_shell(&st, q.theme.as_deref());
    render_tpl(ToolPage {
        page_title: "WASM 本地工具箱",
        active_nav: "tool",
        env_label: sh.env_label,
        theme_css: sh.theme_css,
    })
}

async fn vpn_page(
    State(st): State<Arc<AppState>>,
    Query(q): Query<PageQuery>,
) -> Result<Html<String>, (StatusCode, String)> {
    let sh = build_shell(&st, q.theme.as_deref());
    render_tpl(VpnPage {
        page_title: "LYCO Connect 代理订阅",
        active_nav: "vpn",
        env_label: sh.env_label,
        theme_css: sh.theme_css,
    })
}

fn render_tpl<T: Template>(tpl: T) -> Result<Html<String>, (StatusCode, String)> {
    tpl.render().map(Html).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("模板渲染失败: {e}"),
        )
    })
}

async fn api_health(State(st): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, String)> {
    let h = st.db.health().await.map_err(internal_err)?;
    Ok(Json(json!({
        "ok": true,
        "env": st.env_name,
        "service": "lain42-portal",
        "db": h,
    })))
}

async fn api_announcements(
    State(st): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    Ok(Json(json!({
        "announcements": st.db.list_announcements().await.map_err(internal_err)?
    })))
}

async fn api_add_announcement(
    State(st): State<Arc<AppState>>,
    Json(req): Json<AddAnnouncementReq>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if req.title.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "公告标题不能为空" })),
        ));
    }
    st.db
        .add_announcement(
            req.published_date.trim(),
            req.title.trim(),
            req.summary.trim(),
            req.link_url.trim(),
            req.pinned,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": e.to_string() })),
            )
        })?;
    Ok((StatusCode::CREATED, Json(json!({ "ok": true }))))
}

async fn api_models(State(st): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, String)> {
    Ok(Json(json!({
        "models": st.db.list_ai_models().await.map_err(internal_err)?,
        "credits_per_cny": 10
    })))
}

async fn api_products(State(st): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, String)> {
    Ok(Json(json!({
        "products": st.db.list_products().await.map_err(internal_err)?
    })))
}

async fn api_services(State(st): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, String)> {
    Ok(Json(json!({
        "services": st.db.list_services().await.map_err(internal_err)?
    })))
}

async fn api_list_orders(
    State(st): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    Ok(Json(json!({
        "orders": st.db.list_orders().await.map_err(internal_err)?
    })))
}

async fn api_create_order(
    State(st): State<Arc<AppState>>,
    Json(req): Json<CreateOrderReq>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if !req.buyer_email.contains('@') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "请输入有效的邮箱地址" })),
        ));
    }
    let rec = st
        .db
        .create_order(&req.product_id, req.buyer_email.trim())
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": e.to_string() })),
            )
        })?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "ok": true, "order": rec })),
    ))
}

async fn api_confirm_order(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (order, account) = st.db.confirm_order(&id).await.map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
    })?;
    Ok(Json(json!({
        "ok": true,
        "order": order,
        "account": account
    })))
}

async fn api_account_lookup(
    State(st): State<Arc<AppState>>,
    Json(req): Json<AccountLookupReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !req.email.contains('@') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "请输入有效的邮箱地址" })),
        ));
    }
    let account = st
        .db
        .get_or_create_account(&req.email)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": e.to_string() })),
            )
        })?;
    Ok(Json(json!({ "ok": true, "account": account })))
}

async fn api_verify_license(
    State(st): State<Arc<AppState>>,
    Json(req): Json<VerifyLicenseReq>,
) -> Json<Value> {
    match lilyco_license::verify_license(&st.license_secret, &req.token) {
        Ok(lic) => Json(json!({ "ok": true, "license": lic })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn api_themes() -> Json<Value> {
    let presets: Vec<Value> = ThemePreset::all_builtins()
        .iter()
        .map(|p| {
            let s = ThemeSpec::from_preset(p.clone());
            json!({ "id": p.id(), "name": s.display_name })
        })
        .collect();
    Json(json!({
        "protocol_version": lilyco::__core::theme::UI_PROTOCOL_VERSION,
        "presets": presets
    }))
}

async fn llms_txt(State(st): State<Arc<AppState>>) -> Response {
    let body = format!(
        "# 云枢智创 · lain42（{}）\n\n\
        > 全 Rust Lilyco 门户（Axum 0.8 + Askama + Turso Pure-Rust SQL + lilyco-license）\n\n\
        ## 页面入口\n\
        - / — 首页（公告、精选商品、在线服务目录）\n\
        - /models — AI API 模型与实时积分价格（10 积分 = 1 元）\n\
        - /shop — 数字商品商城、积分充值、账户余额与离线授权验签\n\
        - /docs — OpenAI 与 Anthropic/Claude Code API 调用文档\n\
        - /tool — 纯浏览器本地 WASM 工具箱（JSON / Base64 / SHA-256 / Token 估算）\n\
        - /vpn — LYCO Connect 三地四专线 20 节点月卡订阅\n\n\
        ## 核心 API\n\
        - GET /api/health\n\
        - GET /api/models | GET /api/products | GET /api/services | GET /api/announcements\n\
        - POST /api/orders | POST /api/orders/{{id}}/confirm | POST /api/accounts/lookup\n\
        - POST /api/license/verify\n\
        - POST /mcp (JSON-RPC 2.0)\n",
        st.env_name
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

async fn agent_json(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "name": "lain42-portal",
        "env": st.env_name,
        "framework": "lilyco",
        "database": "turso",
        "pages": ["/", "/models", "/shop", "/docs", "/tool", "/vpn"],
        "mcp_endpoint": "/mcp",
        "openapi": "/openapi.json",
        "llms_txt": "/llms.txt"
    }))
}

async fn openapi_json(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "openapi": "3.1.0",
        "info": {
            "title": format!("lain42-portal ({})", st.env_name),
            "version": env!("CARGO_PKG_VERSION")
        },
        "paths": {
            "/api/health": { "get": { "summary": "数据库与服务健康检查" } },
            "/api/announcements": {
                "get": { "summary": "获取公告列表" },
                "post": { "summary": "发布新公告" }
            },
            "/api/models": { "get": { "summary": "获取 AI API 模型与实时价格" } },
            "/api/products": { "get": { "summary": "获取商城产品与积分套餐" } },
            "/api/services": { "get": { "summary": "获取在线服务入口目录" } },
            "/api/orders": {
                "get": { "summary": "查询最近订单" },
                "post": { "summary": "创建订单并持久化到 Turso" }
            },
            "/api/orders/{id}/confirm": { "post": { "summary": "确认支付并自动发放积分" } },
            "/api/accounts/lookup": { "post": { "summary": "查询或自动开通邮箱积分账户与 lkr_ 密钥" } },
            "/api/license/verify": { "post": { "summary": "验证 L42 企业授权令牌" } }
        }
    }))
}

async fn mcp_endpoint(State(st): State<Arc<AppState>>, Json(req): Json<McpReq>) -> Json<Value> {
    let result = match req.method.as_str() {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "lain42-portal", "version": env!("CARGO_PKG_VERSION") }
        }),
        "tools/list" => json!({
            "tools": [
                { "name": "list_models", "description": "查询 Turso 数据库中的 AI API 模型与实时积分价格", "inputSchema": { "type": "object", "properties": {} } },
                { "name": "list_products", "description": "查询商城产品与套餐", "inputSchema": { "type": "object", "properties": {} } },
                { "name": "db_health", "description": "查询 Turso 数据库状态与表行数", "inputSchema": { "type": "object", "properties": {} } }
            ]
        }),
        "tools/call" => {
            let name = req.params.get("name").and_then(Value::as_str).unwrap_or("");
            let payload = match name {
                "list_models" => serde_json::to_string_pretty(&st.db.list_ai_models().await.unwrap_or_default()).unwrap_or_default(),
                "list_products" => serde_json::to_string_pretty(&st.db.list_products().await.unwrap_or_default()).unwrap_or_default(),
                "db_health" => serde_json::to_string_pretty(&st.db.health().await.ok()).unwrap_or_default(),
                _ => format!("unknown tool: {name}"),
            };
            json!({ "content": [{ "type": "text", "text": payload }] })
        }
        _ => json!({ "error": format!("unsupported method: {}", req.method) }),
    };
    Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": result }))
}

fn internal_err<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
