//! Axum 0.8 路由层：统一门户路由装配、核心页面与核心业务 API。

mod downloads;
mod proxy;
mod seo;

use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use lilyco::__core::theme::{ThemePreset, ThemeSpec};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::db::{AiModel, Announcement, DbHealth, PortalDb};
use crate::studio_api::studio_router;

pub(crate) use self::downloads::check_admin_access;
use self::downloads::{
    api_admin_adjust_points, api_admin_analytics_summary, api_admin_grant_subscription,
    api_admin_list_users, api_admin_reset_password, api_admin_update_email,
    api_admin_update_quark, api_analytics_event, api_download_assets, api_downloads, api_mirrors,
    api_resolve_download, download_detail_page, downloads_page, go_download, go_mirror_download,
};
use self::proxy::{
    proxy_mc_assets_root, proxy_mc_assets_sub, proxy_pool_root, proxy_pool_sub, proxy_relay_root,
    proxy_relay_sub,
};
use self::seo::{
    agent_json, indexnow_key_txt, llms_txt, mcp_endpoint, openapi_json, robots_txt, sitemap_xml,
};

#[derive(Clone)]
pub struct AppState {
    pub db: PortalDb,
    pub env_name: String,
    pub license_secret: String,
    pub admin_secret: Option<String>,
    pub static_dir: Option<String>,
    pub pay_config_dir: Option<String>,
}

pub(super) struct ShellCtx {
    pub(super) env_label: String,
    pub(super) theme_css: String,
}

pub(super) fn build_shell(st: &AppState, theme_q: Option<&str>) -> ShellCtx {
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
    pub(super) env_label: String,
    pub(super) theme_css: String,
    health: DbHealth,
    announcements: Vec<Announcement>,
}

#[derive(Template)]
#[template(path = "models.html")]
struct ModelsPage {
    page_title: &'static str,
    active_nav: &'static str,
    pub(super) env_label: String,
    pub(super) theme_css: String,
    models: Vec<AiModel>,
}

#[derive(Template)]
#[template(path = "shop.html")]
struct ShopPage {
    page_title: &'static str,
    active_nav: &'static str,
    pub(super) env_label: String,
    pub(super) theme_css: String,
}

#[derive(Template)]
#[template(path = "account.html")]
struct AccountPage {
    page_title: &'static str,
    active_nav: &'static str,
    pub(super) env_label: String,
    pub(super) theme_css: String,
}

#[derive(Template)]
#[template(path = "docs.html")]
struct DocsPage {
    page_title: &'static str,
    active_nav: &'static str,
    pub(super) env_label: String,
    pub(super) theme_css: String,
}

#[derive(Template)]
#[template(path = "tool.html")]
struct ToolPage {
    page_title: &'static str,
    active_nav: &'static str,
    pub(super) env_label: String,
    pub(super) theme_css: String,
}

#[derive(Template)]
#[template(path = "vpn.html")]
struct VpnPage {
    page_title: &'static str,
    active_nav: &'static str,
    pub(super) env_label: String,
    pub(super) theme_css: String,
}


#[derive(Deserialize)]
pub(super) struct PageQuery {
    pub(super) theme: Option<String>,
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


pub fn build_router(state: Arc<AppState>) -> Router {
    let api_and_ai = Router::new()
        .merge(studio_router())
        .merge(crate::admin_routes::router())
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
        .route("/api/downloads", get(api_downloads))
        .route("/api/downloads/{slug}/resolve", get(api_resolve_download))
        .route("/api/downloads/{slug}/assets", get(api_download_assets))
        .route("/api/analytics/event", post(api_analytics_event))
        .route("/api/admin/analytics/summary", get(api_admin_analytics_summary))
        .route("/api/admin/users", get(api_admin_list_users))
        .route("/api/admin/users/email", post(api_admin_update_email))
        .route("/api/admin/users/reset-password", post(api_admin_reset_password))
        .route("/api/admin/users/adjust-points", post(api_admin_adjust_points))
        .route("/api/admin/users/grant-subscription", post(api_admin_grant_subscription))
        .route("/api/admin/downloads/quark", post(api_admin_update_quark))
        .route("/api/mirrors", get(api_mirrors))
        .route("/go/download/{slug}", get(go_download))
        .route("/go/mirror/{slug}", get(go_mirror_download))
        .route("/mc-assets", any(proxy_mc_assets_root))
        .route("/mc-assets/", any(proxy_mc_assets_root))
        .route("/mc-assets/{*rest}", any(proxy_mc_assets_sub))
        .route("/relay", any(proxy_relay_root))
        .route("/relay/", any(proxy_relay_root))
        .route("/relay/{*rest}", any(proxy_relay_sub))
        .route("/api/pool", any(proxy_pool_root))
        .route("/api/pool/", any(proxy_pool_root))
        .route("/api/pool/{*rest}", any(proxy_pool_sub))
        .route("/lain42-indexnow-key.txt", get(indexnow_key_txt))
        .route("/llms.txt", get(llms_txt))
        .route("/.well-known/agent.json", get(agent_json))
        .route("/openapi.json", get(openapi_json))
        .route("/robots.txt", get(robots_txt))
        .route("/sitemap.xml", get(sitemap_xml))
        .route("/mcp", post(mcp_endpoint));

    let router = if let Some(dir) = state.static_dir.clone() {
        let index_file = std::path::PathBuf::from(&dir).join("index.html");
        // 商业级 React + shadcn/ui SPA 前端（保留 /downloads/{slug} SSR SEO 与全量 /api/* /studio/api/*）
        api_and_ai
            .route("/downloads/{slug}", get(download_detail_page))
            .fallback_service(
                ServeDir::new(dir)
                    .append_index_html_on_directories(true)
                    .fallback(tower_http::services::ServeFile::new(index_file)),
            )
    } else {
        api_and_ai
            .route("/", get(home_page))
            .route("/models", get(models_page))
            .route("/models/", get(models_page))
            .route("/shop", get(shop_page))
            .route("/shop/", get(shop_page))
            .route("/downloads", get(downloads_page))
            .route("/downloads/", get(downloads_page))
            .route("/downloads/{slug}", get(download_detail_page))
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

pub(super) fn render_tpl<T: Template>(tpl: T) -> Result<Html<String>, (StatusCode, String)> {
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

pub(super) fn internal_err<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
