//! Axum 0.8 路由层：完整复刻 `lain42.top` 六大子页（Askama 编译期 SSR）+ 全功能 Turso API + AI 原生协议。

use std::sync::Arc;

use askama::Template;
use axum::body::{to_bytes, Body};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderName, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use lilyco::__core::theme::{ThemePreset, ThemeSpec};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::db::{AiModel, Announcement, DbHealth, DownloadItem, PortalDb};
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

#[derive(Template)]
#[template(path = "downloads.html")]
struct DownloadsPage {
    page_title: &'static str,
    active_nav: &'static str,
    env_label: String,
    theme_css: String,
    items: Vec<DownloadItem>,
    total_count: usize,
    self_count: usize,
    oss_count: usize,
    game_count: usize,
    video_count: usize,
    music_count: usize,
    ebook_count: usize,
}

#[derive(Template)]
#[template(path = "download_detail.html")]
struct DownloadDetailPage {
    page_title: String,
    active_nav: &'static str,
    env_label: String,
    theme_css: String,
    item: DownloadItem,
    related: Vec<DownloadItem>,
    software_json_ld: String,
    faq_json_ld: String,
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
        .route("/api/downloads", get(api_downloads))
        .route("/api/downloads/{slug}/resolve", get(api_resolve_download))
        .route("/api/analytics/event", post(api_analytics_event))
        .route("/api/admin/analytics/summary", get(api_admin_analytics_summary))
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
    let items = st.db.list_downloads().await.unwrap_or_default();
    let mut dl_lines = String::new();
    for item in &items {
        dl_lines.push_str(&format!(
            "- /downloads/{} — {} {} ({}) [Quark Mirror + GitHub]\n",
            item.slug, item.name, item.version, item.repo
        ));
    }
    let body = format!(
        "# 云枢智创 · lain42（{}）\n\n\
        > 全 Rust Lilyco 门户（Axum 0.8 + Askama + Turso Pure-Rust SQL + lilyco-pay + lilyco-license）\n\n\
        ## 页面入口\n\
        - / — 首页（公告、精选商品、在线服务目录）\n\
        - /models — AI API 模型与实时积分价格（10 积分 = 1 元）\n\
        - /shop — 全球积分收银台（支付宝/爱发电/Creem/Stripe/Google Play/USDT/卡密）与数字商城\n\
        - /downloads — 软件与高星 GitHub 开源国内高速下载中心（夸克网盘直链 + SHA-256 校验）\n\
        - /account — 账户控制台（积分余额、生效权益、订单账单）\n\
        - /docs — OpenAI 与 Anthropic/Claude Code API 调用文档\n\
        - /tool — 纯浏览器本地 WASM 工具箱（JSON / Base64 / SHA-256 / Token 估算）\n\
        - /vpn — LYCO Connect 代理订阅（月卡 ¥30 / 季卡 98 折 / 年卡 9 折）\n\n\
        ## 软件与高星开源镜像目录（/downloads/{{slug}}）\n\
        {}\n\
        ## 核心 API\n\
        - GET /api/health | GET /api/downloads | GET /robots.txt | GET /sitemap.xml\n\
        - GET /studio/api/products | POST /studio/api/register | POST /studio/api/login | GET /studio/api/me\n\
        - POST /studio/api/payments/{{afdian,alipay,creem,stripe}}/intents\n\
        - POST /studio/api/payments/{{alipay/notify,afdian/webhook,creem/webhook}}\n\
        - GET /studio/api/sub/{{token}}/{{format}} | GET /studio/api/music/sub/{{key}} | POST /studio/api/verify\n\
        - POST /mcp (JSON-RPC 2.0)\n",
        st.env_name, dl_lines
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

async fn robots_txt() -> Response {
    let body = "User-agent: *\nAllow: /\nDisallow: /studio/api/\nDisallow: /account\nSitemap: https://dev.lain42.top/sitemap.xml\n";
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

async fn indexnow_key_txt() -> Response {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], "lain42indexnow2026key").into_response()
}

async fn sitemap_xml(State(st): State<Arc<AppState>>) -> Response {
    let pages = [
        ("/", "1.0"),
        ("/downloads", "0.95"),
        ("/models", "0.9"),
        ("/shop", "0.9"),
        ("/docs", "0.85"),
        ("/tool", "0.8"),
        ("/vpn", "0.85"),
    ];
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
"#,
    );
    for (p, prio) in pages {
        xml.push_str(&format!(
            "  <url><loc>https://dev.lain42.top{p}</loc><changefreq>daily</changefreq><priority>{prio}</priority></url>\n"
        ));
    }
    if let Ok(items) = st.db.list_downloads().await {
        for item in items {
            let lastmod = if item.published_at.is_empty() {
                String::new()
            } else {
                format!("<lastmod>{}</lastmod>", item.published_at)
            };
            xml.push_str(&format!(
                "  <url><loc>https://dev.lain42.top/downloads/{}</loc>{lastmod}<changefreq>weekly</changefreq><priority>0.8</priority></url>\n",
                item.slug
            ));
        }
    }
    xml.push_str("</urlset>\n");
    ([(header::CONTENT_TYPE, "application/xml; charset=utf-8")], xml).into_response()
}

async fn downloads_page(
    State(st): State<Arc<AppState>>,
    Query(q): Query<PageQuery>,
) -> Result<Html<String>, (StatusCode, String)> {
    let sh = build_shell(&st, q.theme.as_deref());
    let items = st.db.list_downloads().await.map_err(internal_err)?;
    let total_count = items.len();
    let self_count = items.iter().filter(|i| i.category == "self").count();
    let oss_count = items.iter().filter(|i| i.category == "oss").count();
    let game_count = items.iter().filter(|i| i.category == "game").count();
    let video_count = items.iter().filter(|i| i.category == "video").count();
    let music_count = items.iter().filter(|i| i.category == "music").count();
    let ebook_count = items.iter().filter(|i| i.category == "ebook").count();
    let tpl = DownloadsPage {
        page_title: "软件与高星 GitHub 开源高速下载中心（游戏/影视/音乐/电子书）",
        active_nav: "downloads",
        env_label: sh.env_label,
        theme_css: sh.theme_css,
        items,
        total_count,
        self_count,
        oss_count,
        game_count,
        video_count,
        music_count,
        ebook_count,
    };
    render_tpl(tpl)
}

async fn download_detail_page(
    State(st): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Query(q): Query<PageQuery>,
) -> Result<Response, (StatusCode, String)> {
    let Some(item) = st.db.get_download(&slug).await.map_err(internal_err)? else {
        return Ok((StatusCode::NOT_FOUND, Html("<h1>404 Not Found</h1><p><a href=\"/downloads\">返回下载中心</a></p>")).into_response());
    };
    let sh = build_shell(&st, q.theme.as_deref());
    let all = st.db.list_downloads().await.unwrap_or_default();
    let related: Vec<DownloadItem> = all
        .into_iter()
        .filter(|x| x.slug != item.slug)
        .take(6)
        .collect();
    let software_json_ld = json!({
        "@context": "https://schema.org",
        "@type": "SoftwareApplication",
        "name": item.name,
        "softwareVersion": item.version,
        "operatingSystem": item.platform,
        "applicationCategory": "UtilitiesApplication",
        "description": item.summary,
        "downloadUrl": format!("https://dev.lain42.top/go/download/{}", item.slug),
        "url": format!("https://dev.lain42.top/downloads/{}", item.slug),
        "offers": {
            "@type": "Offer",
            "price": "0",
            "priceCurrency": "CNY"
        }
    })
    .to_string();
    let faq_json_ld = json!({
        "@context": "https://schema.org",
        "@type": "FAQPage",
        "mainEntity": [
            {
                "@type": "Question",
                "name": format!("{} 的夸克网盘安装包安全吗？", item.name),
                "acceptedAnswer": {
                    "@type": "Answer",
                    "text": format!("安全。文件由自动化流水线直接从官方 GitHub Release ({}) 拉取并上传至夸克网盘，附带 SHA-256 校验值。", item.repo)
                }
            }
        ]
    })
    .to_string();
    let tpl = DownloadDetailPage {
        page_title: format!("{} {} 国内高速下载（夸克网盘直链）", item.name, item.version),
        active_nav: "downloads",
        env_label: sh.env_label,
        theme_css: sh.theme_css,
        item,
        related,
        software_json_ld,
        faq_json_ld,
    };
    let html = render_tpl(tpl)?;
    Ok(html.into_response())
}

async fn go_download(
    State(st): State<Arc<AppState>>,
    Path(slug): Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Ok(Some(item)) = st.db.get_download(&slug).await {
        let _ = st.db.increment_download_count(&slug).await;
        let plat = crate::mirror::detect_platform(&headers, None, None);
        let ua = headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let (target, channel) = if !item.quark_url.is_empty() {
            (item.quark_url, "quark")
        } else if !item.github_download_url.is_empty() {
            (item.github_download_url, "mirror")
        } else {
            (item.github_release_url, "mirror")
        };
        let _ = st
            .db
            .record_download_event(&slug, channel, channel, &plat.os, &plat.arch, ua)
            .await;
        if !target.is_empty() {
            let mut resp = Redirect::temporary(&target).into_response();
            resp.headers_mut().insert(
                header::REFERRER_POLICY,
                HeaderValue::from_static("no-referrer"),
            );
            return resp;
        }
    }
    Redirect::temporary("/downloads").into_response()
}

async fn api_downloads(State(st): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, String)> {
    let items = st.db.list_downloads().await.map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "count": items.len(), "items": items })))
}

#[derive(Deserialize)]
struct AnalyticsEventReq {
    #[serde(default)]
    slug: String,
    channel: String,
    #[serde(default)]
    mirror: String,
    #[serde(default)]
    os: String,
    #[serde(default)]
    arch: String,
}

async fn api_analytics_event(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AnalyticsEventReq>,
) -> Json<Value> {
    let ch = req.channel.trim();
    if !matches!(ch, "quark" | "quark_qr" | "mirror" | "page_view" | "search") {
        return Json(json!({ "ok": false }));
    }
    let plat = crate::mirror::detect_platform(
        &headers,
        if req.os.is_empty() { None } else { Some(req.os.as_str()) },
        if req.arch.is_empty() { None } else { Some(req.arch.as_str()) },
    );
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let slug = if req.slug.trim().is_empty() {
        "_site"
    } else {
        req.slug.trim()
    };
    let _ = st
        .db
        .record_download_event(slug, ch, req.mirror.trim(), &plat.os, &plat.arch, ua)
        .await;
    Json(json!({ "ok": true }))
}

async fn api_admin_analytics_summary(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, (StatusCode, String)> {
    let secret = headers
        .get("x-admin-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let is_secret_ok = !secret.is_empty() && secret == st.license_secret;
    let is_admin_user = crate::studio_api::extract_username(&headers, &st.license_secret)
        .map(|u| u.eq_ignore_ascii_case("lilyco42ovo") || u.eq_ignore_ascii_case("admin"))
        .unwrap_or(false);
    if !is_secret_ok && !is_admin_user {
        return Err((StatusCode::FORBIDDEN, "仅管理员可访问数据看板".to_string()));
    }
    let summary = st.db.analytics_summary().await.map_err(internal_err)?;
    Ok(Json(summary))
}

#[derive(Deserialize)]
struct AdminQuarkReq {
    slug: String,
    #[serde(default)]
    version: String,
    quark_url: String,
}

async fn api_admin_update_quark(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AdminQuarkReq>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let secret = headers
        .get("x-admin-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if secret.is_empty() || secret != st.license_secret {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    }
    let updated = st
        .db
        .update_download_quark_url(req.slug.trim(), req.version.trim(), req.quark_url.trim())
        .await
        .map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "slug": req.slug, "updated": updated })))
}

async fn api_mirrors() -> Json<Value> {
    Json(crate::mirror::supported_mirrors_registry())
}

async fn api_resolve_download(
    State(st): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Query(q): Query<crate::mirror::MirrorQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, (StatusCode, String)> {
    let Some(item) = st.db.get_download(&slug).await.map_err(internal_err)? else {
        return Err((StatusCode::NOT_FOUND, format!("未找到下载项: {slug}")));
    };
    let plat = crate::mirror::detect_platform(&headers, q.os.as_deref(), q.arch.as_deref());
    let mirror = q.mirror.unwrap_or_else(|| "auto".to_string());
    let ghproxy = q.ghproxy;
    let resolved = tokio::task::spawn_blocking(move || {
        crate::mirror::resolve_download_mirror(&item, &mirror, ghproxy.as_deref(), &plat)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "ok": true, "resolution": resolved })))
}

async fn go_mirror_download(
    State(st): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Query(q): Query<crate::mirror::MirrorQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Ok(Some(item)) = st.db.get_download(&slug).await {
        let _ = st.db.increment_download_count(&slug).await;
        let plat = crate::mirror::detect_platform(&headers, q.os.as_deref(), q.arch.as_deref());
        let mirror = q.mirror.unwrap_or_else(|| "auto".to_string());
        let ghproxy = q.ghproxy;
        let ua = headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let plat_log = plat.clone();
        if let Ok(resolved) = tokio::task::spawn_blocking(move || {
            crate::mirror::resolve_download_mirror(&item, &mirror, ghproxy.as_deref(), &plat)
        })
        .await
        {
            if !resolved.target_url.is_empty() {
                let ch = if resolved.mirror == "quark" || resolved.target_url.contains("pan.quark.cn") {
                    "quark"
                } else {
                    "mirror"
                };
                let _ = st
                    .db
                    .record_download_event(&slug, ch, &resolved.mirror_label, &plat_log.os, &plat_log.arch, &ua)
                    .await;
                let mut resp = Redirect::to(&resolved.target_url).into_response();
                resp.headers_mut().insert(
                    header::REFERRER_POLICY,
                    HeaderValue::from_static("no-referrer"),
                );
                return resp;
            }
        }
    }
    Redirect::temporary("/downloads").into_response()
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

async fn proxy_mc_assets_root(req: Request) -> Response {
    proxy_local_service(8088, "", req).await
}

async fn proxy_mc_assets_sub(Path(rest): Path<String>, req: Request) -> Response {
    proxy_local_service(8088, &rest, req).await
}

async fn proxy_relay_root(req: Request) -> Response {
    proxy_local_service(8090, "", req).await
}

async fn proxy_relay_sub(Path(rest): Path<String>, req: Request) -> Response {
    proxy_local_service(8090, &rest, req).await
}

async fn proxy_local_service(port: u16, rest: &str, req: Request) -> Response {
    let method = req.method().as_str().to_string();
    let query = req.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
    let target_path = if rest.is_empty() {
        format!("/{query}")
    } else {
        format!("/{rest}{query}")
    };
    let req_headers = req.headers().clone();
    let body_bytes = match to_bytes(req.into_body(), 4 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("read body error: {e}")).into_response(),
    };

    let mut stream = match TcpStream::connect(format!("127.0.0.1:{port}")).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("后端服务 (127.0.0.1:{port}) 暂未就绪: {e}"),
            )
                .into_response()
        }
    };

    let mut wire = format!("{method} {target_path} HTTP/1.1\r\nHost: 127.0.0.1:8088\r\nConnection: close\r\nAccept-Encoding: identity\r\n");
    for (k, v) in &req_headers {
        let kn = k.as_str().to_ascii_lowercase();
        if matches!(kn.as_str(), "host" | "connection" | "accept-encoding" | "content-length" | "transfer-encoding") {
            continue;
        }
        if let Ok(vs) = v.to_str() {
            wire.push_str(&format!("{}: {}\r\n", k.as_str(), vs));
        }
    }
    if !body_bytes.is_empty() {
        wire.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
    }
    wire.push_str("\r\n");

    if stream.write_all(wire.as_bytes()).await.is_err()
        || (!body_bytes.is_empty() && stream.write_all(&body_bytes).await.is_err())
    {
        return (StatusCode::BAD_GATEWAY, "写入上游请求失败").into_response();
    }

    let mut raw = Vec::with_capacity(8192);
    if stream.read_to_end(&mut raw).await.is_err() {
        return (StatusCode::BAD_GATEWAY, "读取上游响应失败").into_response();
    }

    let Some(sep) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return (StatusCode::BAD_GATEWAY, "上游响应格式无效").into_response();
    };
    let head_str = String::from_utf8_lossy(&raw[..sep]);
    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().unwrap_or("HTTP/1.1 502 Bad Gateway");
    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(502);
    let status = StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY);

    let mut resp_builder = Response::builder().status(status);
    let mut is_html = false;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let kn = k.trim().to_ascii_lowercase();
            let vv = v.trim();
            if matches!(kn.as_str(), "content-length" | "transfer-encoding" | "connection") {
                continue;
            }
            if kn == "content-type" && vv.contains("text/html") {
                is_html = true;
            }
            if let (Ok(hn), Ok(hv)) = (
                HeaderName::from_bytes(kn.as_bytes()),
                HeaderValue::from_str(vv),
            ) {
                resp_builder = resp_builder.header(hn, hv);
            }
        }
    }

    let body_slice = &raw[sep + 4..];
    let final_body = if is_html {
        let html = String::from_utf8_lossy(body_slice)
            .replace("'/api/v1/", "'/mc-assets/api/v1/")
            .replace("`/api/v1/", "`/mc-assets/api/v1/");
        html.into_bytes()
    } else {
        body_slice.to_vec()
    };

    resp_builder
        .body(Body::from(final_body))
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "response build error").into_response())
}
