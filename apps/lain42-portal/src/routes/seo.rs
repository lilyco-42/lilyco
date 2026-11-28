//! AI 原生发现协议与 SEO 路由（llms.txt、robots.txt、IndexNow、sitemap.xml、agent.json、openapi.json、MCP）。

use std::sync::Arc;

use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::AppState;

#[derive(Deserialize)]
pub(super) struct McpReq {
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}


pub(super) async fn llms_txt(State(st): State<Arc<AppState>>) -> Response {
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

pub(super) async fn robots_txt() -> Response {
    let body = "User-agent: *\nAllow: /\nDisallow: /studio/api/\nDisallow: /account\nSitemap: https://dev.lain42.top/sitemap.xml\n";
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

pub(super) async fn indexnow_key_txt() -> Response {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], "lain42indexnow2026key").into_response()
}

pub(super) async fn sitemap_xml(State(st): State<Arc<AppState>>) -> Response {
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

pub(super) async fn agent_json(State(st): State<Arc<AppState>>) -> Json<Value> {
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

pub(super) async fn openapi_json(State(st): State<Arc<AppState>>) -> Json<Value> {
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

pub(super) async fn mcp_endpoint(State(st): State<Arc<AppState>>, Json(req): Json<McpReq>) -> Json<Value> {
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
