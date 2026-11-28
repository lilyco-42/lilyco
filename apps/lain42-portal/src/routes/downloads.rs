//! 下载库 SSR 页面、多平台镜像解析与跳转、下载行为埋点与后台用户/夸克管理路由。

use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::DownloadItem;

use super::{build_shell, internal_err, render_tpl, AppState, PageQuery};

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


pub(super) async fn downloads_page(
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

pub(super) async fn download_detail_page(
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

pub(super) async fn go_download(
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

pub(super) async fn api_downloads(State(st): State<Arc<AppState>>) -> Result<Json<Value>, (StatusCode, String)> {
    let items = st.db.list_downloads().await.map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "count": items.len(), "items": items })))
}

#[derive(Deserialize)]
pub(super) struct AnalyticsEventReq {
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

pub(super) async fn api_analytics_event(
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

pub(crate) fn check_admin_access(st: &AppState, headers: &axum::http::HeaderMap) -> Result<(), (StatusCode, String)> {
    let is_secret_ok = crate::studio_api::check_admin_secret(st, headers);
    let is_admin_user = crate::studio_api::admin_session_username(st, headers).is_some();
    if !is_secret_ok && !is_admin_user {
        return Err((StatusCode::FORBIDDEN, "仅管理员可执行该操作".to_string()));
    }
    Ok(())
}

pub(super) async fn api_admin_analytics_summary(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, (StatusCode, String)> {
    check_admin_access(&st, &headers)?;
    let summary = st.db.analytics_summary().await.map_err(internal_err)?;
    Ok(Json(summary))
}

#[derive(Deserialize)]
pub(super) struct AdminUsersQuery {
    #[serde(default)]
    q: String,
}

pub(super) async fn api_admin_list_users(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<AdminUsersQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    check_admin_access(&st, &headers)?;
    let users = st.db.admin_list_users(&q.q).await.map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "users": users })))
}

#[derive(Deserialize)]
pub(super) struct AdminEmailReq {
    username: String,
    email: String,
}

pub(super) async fn api_admin_update_email(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AdminEmailReq>,
) -> Result<Json<Value>, (StatusCode, String)> {
    check_admin_access(&st, &headers)?;
    let updated = st
        .db
        .admin_update_email(&req.username, &req.email)
        .await
        .map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "updated": updated })))
}

#[derive(Deserialize)]
pub(super) struct AdminResetPwReq {
    username: String,
    new_password: String,
}

pub(super) async fn api_admin_reset_password(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AdminResetPwReq>,
) -> Result<Json<Value>, (StatusCode, String)> {
    check_admin_access(&st, &headers)?;
    if req.new_password.len() < 8 {
        return Err((StatusCode::BAD_REQUEST, "新密码至少需要 8 位".to_string()));
    }
    let uname = req.username.trim().to_ascii_lowercase();
    let hash = crate::studio_api::hash_password(&st.license_secret, &uname, &req.new_password);
    let updated = st
        .db
        .admin_reset_password(&uname, &hash)
        .await
        .map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "updated": updated })))
}

#[derive(Deserialize)]
pub(super) struct AdminPointsReq {
    username: String,
    #[serde(default)]
    paid_delta: i64,
    #[serde(default)]
    bonus_delta: i64,
}

pub(super) async fn api_admin_adjust_points(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AdminPointsReq>,
) -> Result<Json<Value>, (StatusCode, String)> {
    check_admin_access(&st, &headers)?;
    let user = st
        .db
        .admin_adjust_points(&req.username, req.paid_delta, req.bonus_delta)
        .await
        .map_err(internal_err)?;
    Ok(Json(json!({
        "ok": true,
        "username": user.username,
        "paid_balance": user.points_balance,
        "bonus_balance": user.bonus_points
    })))
}

#[derive(Deserialize)]
pub(super) struct AdminGrantSubReq {
    username: String,
    product: String,
    #[serde(default = "default_grant_days")]
    duration_days: i64,
    #[serde(default = "default_grant_gb")]
    traffic_gb: i64,
    #[serde(default)]
    sub_url: String,
}

fn default_grant_days() -> i64 { 30 }
fn default_grant_gb() -> i64 { 220 }

pub(super) async fn api_admin_grant_subscription(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AdminGrantSubReq>,
) -> Result<Json<Value>, (StatusCode, String)> {
    check_admin_access(&st, &headers)?;
    let res = st
        .db
        .admin_grant_subscription(
            &req.username,
            req.product.trim(),
            req.duration_days,
            req.traffic_gb,
            &req.sub_url,
        )
        .await
        .map_err(internal_err)?;
    Ok(Json(res))
}

#[derive(Deserialize)]
pub(super) struct AdminQuarkReq {
    slug: String,
    #[serde(default)]
    version: String,
    quark_url: String,
}

pub(super) async fn api_admin_update_quark(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AdminQuarkReq>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !crate::studio_api::check_admin_secret(&st, &headers) {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    }
    let updated = st
        .db
        .update_download_quark_url(req.slug.trim(), req.version.trim(), req.quark_url.trim())
        .await
        .map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "slug": req.slug, "updated": updated })))
}

pub(super) async fn api_mirrors() -> Json<Value> {
    Json(crate::mirror::supported_mirrors_registry())
}

pub(super) async fn api_resolve_download(
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

pub(super) async fn api_download_assets(
    State(st): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Query(q): Query<crate::mirror::MirrorQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let Some(item) = st.db.get_download(&slug).await.map_err(internal_err)? else {
        return Err((StatusCode::NOT_FOUND, format!("未找到下载项: {slug}")));
    };
    let ghproxy = q.ghproxy;
    let res = tokio::task::spawn_blocking(move || {
        crate::mirror::resolve_release_assets(&item, ghproxy.as_deref())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "ok": true, "data": res })))
}

pub(super) async fn go_mirror_download(
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
