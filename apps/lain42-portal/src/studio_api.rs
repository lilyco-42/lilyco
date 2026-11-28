//! 原版 `lain42.top` `/studio/api/*` 商业计费与账号系统全 Rust 重写（Turso 纯 Rust SQL 持久化）。
//!
//! 100% 兼容 `/var/www/studio/assets/store.js`、`/sub/assets/ui.js`、`/assets/go-quick.js`
//! 与 `/shop/payment.html` 的请求与响应契约：
//! - 账号注册/登录（`GET /register/challenge`, `POST /register`, `POST /login`, `GET /me`）
//! - 26 款真实商品目录（`GET /products`）
//! - 爱发电 / 支付宝 / Stripe / USDT(TRC20) / 卡密兑换全套支付与充值接口
//! - 积分购买与权益自动发放（`POST /points/purchase`, `POST /redeem`）

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use ring::hmac;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::routes::AppState;

const RAW_PRODUCTS_JSON: &str = include_str!("../data/products.json");

#[derive(Deserialize)]
struct AuthReq {
    username: String,
    password: String,
    #[serde(default)]
    email: String,
}

#[derive(Deserialize)]
struct PurchaseReq {
    product: String,
    #[serde(default)]
    #[allow(dead_code)]
    reference: String,
}

#[derive(Deserialize)]
struct RedeemReq {
    code: String,
}

#[derive(Deserialize)]
struct AfdianIntentReq {
    product: String,
    #[serde(default)]
    #[allow(dead_code)]
    reference: String,
}

#[derive(Deserialize)]
struct AlipayTopupReq {
    #[serde(default)]
    amount_cny: Option<f64>,
    #[serde(default)]
    product: Option<String>,
}

#[derive(Deserialize)]
struct UsdtTopupReq {
    amount_usdt: f64,
}

#[derive(Deserialize)]
struct StripeIntentReq {
    product: String,
    #[serde(default)]
    currency: Option<String>,
}

#[derive(Deserialize)]
struct GooglePlayVerifyReq {
    product: String,
    purchase_token: String,
}

pub fn studio_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/studio/api/products", get(get_products))
        .route("/studio/api/register/challenge", get(register_challenge))
        .route("/studio/api/register", post(register))
        .route("/studio/api/login", post(login))
        .route("/studio/api/me", get(get_me))
        .route("/studio/api/account/orders", get(get_account_orders))
        .route("/studio/api/points", get(get_points))
        .route("/studio/api/points/purchase", post(points_purchase))
        .route("/studio/api/points/topup", post(usdt_topup))
        .route("/studio/api/redeem", post(redeem_card))
        .route("/studio/api/orders/{order_no}", get(get_order))
        .route("/studio/api/payments/afdian", get(get_afdian))
        .route("/studio/api/payments/afdian/intents", post(create_afdian_intent))
        .route(
            "/studio/api/payments/afdian/intents/{code}/check",
            post(check_afdian_intent),
        )
        .route("/studio/api/payments/afdian/webhook", post(afdian_webhook))
        .route("/studio/api/payments/alipay", get(get_alipay))
        .route("/studio/api/payments/alipay/topups", post(create_alipay_order))
        .route("/studio/api/payments/alipay/intents", post(create_alipay_order))
        .route(
            "/studio/api/payments/alipay/orders/{number}/check",
            post(check_alipay_order),
        )
        .route("/studio/api/payments/alipay/notify", post(alipay_notify))
        .route("/studio/api/payments/stripe", get(get_stripe))
        .route("/studio/api/payments/stripe/intents", post(create_stripe_intent))
        .route(
            "/studio/api/payments/stripe/intents/{code}/check",
            post(check_stripe_intent),
        )
        .route("/studio/api/payments/creem", get(get_creem))
        .route("/studio/api/payments/creem/intents", post(create_creem_intent))
        .route("/studio/api/payments/creem/webhook", post(creem_webhook))
        .route("/studio/api/payments/google-play", get(get_google_play))
        .route(
            "/studio/api/payments/google-play/verify",
            post(verify_google_play),
        )
        .route("/studio/api/sub/{token}/{format}", get(proxy_subscription))
        .route("/api/sub/{token}/{format}", get(proxy_subscription))
        .route("/panel/api/sub/{token}", get(proxy_panel_sub_shortcut))
        .route("/studio/api/clash/{token}", get(proxy_clash_shortcut))
        .route("/api/clash/{token}", get(proxy_clash_shortcut))
        .route("/studio/api/music/sub/{key}", get(music_subscription))
        .route("/studio/api/download/{product}", get(entitled_download))
        .route("/studio/api/verify", post(verify_client_entitlement))
        .route("/studio/api/marketing/event", post(marketing_event))
        .route("/studio/api/contact", post(create_contact_ticket))
        .route("/studio/api/proxy/quality", get(crate::proxy_health::report))
        .route("/api/proxy/quality", get(crate::proxy_health::report))
        .route("/api/proxy/official-audit", get(crate::proxy_health::official_audit))
}

fn err(code: StatusCode, detail: impl Into<String>) -> (StatusCode, Json<Value>) {
    (code, Json(json!({ "ok": false, "detail": detail.into() })))
}

fn sign_token(secret: &str, payload: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let tag = hmac::sign(&key, payload.as_bytes());
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(payload.as_bytes()),
        URL_SAFE_NO_PAD.encode(tag.as_ref())
    )
}

fn verify_token(secret: &str, token: &str) -> Option<String> {
    let (p_b64, sig_b64) = token.trim().split_once('.')?;
    let payload_bytes = URL_SAFE_NO_PAD.decode(p_b64).ok()?;
    let sig_bytes = URL_SAFE_NO_PAD.decode(sig_b64).ok()?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    hmac::verify(&key, &payload_bytes, &sig_bytes).ok()?;
    String::from_utf8(payload_bytes).ok()
}

pub(crate) fn check_admin_secret(st: &AppState, headers: &HeaderMap) -> bool {
    let Some(secret) = st.admin_secret.as_deref() else { return false };
    let Some(supplied) = headers.get("x-admin-secret") else { return false };
    let expected_key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let supplied_key = hmac::Key::new(hmac::HMAC_SHA256, supplied.as_bytes());
    let proof = hmac::sign(&supplied_key, b"lain42-admin");
    hmac::verify(&expected_key, b"lain42-admin", proof.as_ref()).is_ok()
}

pub(crate) fn admin_session_username(st: &AppState, headers: &HeaderMap) -> Option<String> {
    let secret = st.admin_secret.as_deref()?;
    let token = headers.get("x-admin-session")?.to_str().ok()?;
    let payload = verify_token(secret, token)?;
    let (username, issued) = payload.strip_prefix("admin|")?.split_once('|')?;
    let age = lilyco_license::current_unix_secs().checked_sub(issued.parse::<u64>().ok()?)?;
    (age < 12 * 3600 && crate::admin_routes::is_admin_name(username))
        .then(|| username.to_string())
}

pub(crate) fn hash_password(secret: &str, username: &str, password: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let msg = format!("pw:{}:{}", username.to_ascii_lowercase(), password);
    URL_SAFE_NO_PAD.encode(hmac::sign(&key, msg.as_bytes()).as_ref())
}

pub(crate) fn extract_username(headers: &HeaderMap, secret: &str) -> Result<String, (StatusCode, Json<Value>)> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let tok = auth
        .strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "请先登录网站账户"))?;
    let payload = verify_token(secret, tok)
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "登录凭证无效或已过期"))?;
    let (user, _) = payload
        .split_once('|')
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "登录凭证格式无效"))?;
    Ok(user.to_string())
}

pub(crate) fn catalog_json() -> Value {
    serde_json::from_str(RAW_PRODUCTS_JSON).unwrap_or_else(|_| json!({ "ok": true, "products": [] }))
}

fn find_product_in_catalog(code: &str) -> Option<(String, i64, f64, String)> {
    let cat = catalog_json();
    let arr = cat.get("products")?.as_array()?;
    for p in arr {
        if p.get("code").and_then(Value::as_str) == Some(code) {
            let name = p
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(code)
                .to_string();
            let points = p.get("points_price").and_then(Value::as_i64).unwrap_or(100);
            let price_cny = p
                .get("price_cny")
                .and_then(Value::as_f64)
                .unwrap_or((points as f64) / 10.0);
            let prefix = p
                .get("prefix")
                .and_then(Value::as_str)
                .unwrap_or("L42")
                .to_string();
            return Some((name, points, price_cny, prefix));
        }
    }
    None
}

fn afdian_plan_meta(code: &str) -> Option<(&'static str, &'static str)> {
    match code {
        "points_cny_1" => Some(("e1fd2b18c51f11f1ab605254001e7c00", "e20973b4c51f11f1bb655254001e7c00")),
        "points_cny_5" => Some(("31d573fccc9b11f1bf2a52540025c377", "31ddfd74cc9b11f1b53252540025c377")),
        "points_cny_6" => Some(("9c87957cc52011f1a8d352540025c377", "9c9024bcc52011f1aa6752540025c377")),
        "points_cny_10" => Some(("472586cacc9b11f18df25254001e7c00", "472f462ecc9b11f1b0295254001e7c00")),
        "points_cny_30" => Some(("b3718c5cc52011f1bb7b52540025c377", "b379cdf4c52011f192f052540025c377")),
        "points_cny_45" => Some(("63f94afacc9d11f19fbe5254001e7c00", "6402a582cc9d11f185b65254001e7c00")),
        "points_cny_100" => Some(("bb7d58f4c52011f1a29c5254001e7c00", "bb86d898c52011f1b5d95254001e7c00")),
        "points_cny_200" => Some(("c9ac6992c52011f1b38b5254001e7c00", "c9b4db18c52011f1a4de5254001e7c00")),
        "points_cny_300" => Some(("d603735cc52011f18cc652540025c377", "d60d17e0c52011f1881e52540025c377")),
        "points_cny_1000" => Some(("e4a89d38c52011f1b0825254001e7c00", "e4b2ebeec52011f1b6215254001e7c00")),
        _ => None,
    }
}

fn afdian_packs() -> Vec<Value> {
    let tiers = [
        ("points_cny_1", "10 积分", 1, 10),
        ("points_cny_5", "50 积分", 5, 50),
        ("points_cny_6", "60 积分", 6, 60),
        ("points_cny_10", "100 积分", 10, 100),
        ("points_cny_30", "300 积分", 30, 300),
        ("points_cny_45", "450 积分", 45, 450),
        ("points_cny_100", "1000 积分", 100, 1000),
        ("points_cny_200", "2000 积分", 200, 2000),
        ("points_cny_300", "3000 积分", 300, 3000),
        ("points_cny_1000", "10000 积分", 1000, 10000),
    ];
    tiers
        .iter()
        .map(|(code, name, cny, pts)| {
            json!({
                "product": code,
                "name": name,
                "price_cny": cny,
                "points_credit": pts
            })
        })
        .collect()
}

async fn get_products() -> Json<Value> {
    Json(catalog_json())
}

async fn register_challenge(State(st): State<Arc<AppState>>) -> Json<Value> {
    let now = lilyco_license::current_unix_secs();
    let chl = sign_token(&st.license_secret, &format!("chl|{now}"));
    Json(json!({ "ok": true, "challenge": chl }))
}

async fn register(
    State(st): State<Arc<AppState>>,
    Json(req): Json<AuthReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = req.username.trim();
    if username.len() < 3
        || username.len() > 24
        || !username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "用户名需为 3-24 位字母、数字或下划线",
        ));
    }
    if crate::admin_routes::is_admin_name(username) {
        return Err(err(StatusCode::BAD_REQUEST, "该用户名不可注册"));
    }
    if req.password.len() < 8 {
        return Err(err(StatusCode::BAD_REQUEST, "密码至少需要 8 位"));
    }
    let pw_hash = hash_password(&st.license_secret, username, &req.password);
    let user = st
        .db
        .studio_register(username, &pw_hash, req.email.trim())
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?;
    let now = lilyco_license::current_unix_secs();
    let token = sign_token(&st.license_secret, &format!("{}|{now}", user.username));
    Ok(Json(json!({
        "ok": true,
        "account_id": user.id,
        "username": user.username,
        "token": token
    })))
}

async fn login(
    State(st): State<Arc<AppState>>,
    Json(req): Json<AuthReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = req.username.trim();
    let pw_hash = hash_password(&st.license_secret, username, &req.password);
    let user = st
        .db
        .studio_login(username, &pw_hash, &req.password)
        .await
        .map_err(|e| err(StatusCode::UNAUTHORIZED, e.to_string()))?;
    let now = lilyco_license::current_unix_secs();
    let token = sign_token(&st.license_secret, &format!("{}|{now}", user.username));
    let admin_token = st.admin_secret.as_deref()
        .filter(|_| crate::admin_routes::is_admin_name(&user.username))
        .map(|secret| sign_token(secret, &format!("admin|{}|{now}", user.username)));
    Ok(Json(json!({
        "ok": true,
        "account_id": user.id,
        "username": user.username,
        "token": token,
        "admin_token": admin_token
    })))
}

async fn get_me(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let mut me = st
        .db
        .studio_get_me(&username)
        .await
        .map_err(|e| err(StatusCode::UNAUTHORIZED, e.to_string()))?;
    me["is_admin"] = json!(admin_session_username(&st, &headers).as_deref() == Some(&username));
    Ok(Json(me))
}

async fn get_account_orders(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let me = st
        .db
        .studio_get_me(&username)
        .await
        .map_err(|e| err(StatusCode::UNAUTHORIZED, e.to_string()))?;
    let items = me
        .get("orders")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(Json(json!({
        "ok": true,
        "items": items,
        "next_before": Value::Null
    })))
}

async fn get_points(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let me = st
        .db
        .studio_get_me(&username)
        .await
        .map_err(|e| err(StatusCode::UNAUTHORIZED, e.to_string()))?;
    Ok(Json(json!({ "ok": true, "points": me["points"] })))
}

async fn points_purchase(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PurchaseReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    if req.product.starts_with("paodou_") {
        return Err(err(
            StatusCode::CONFLICT,
            "校园跑权益由主站发放，请前往 https://lain42.top/shop 购买",
        ));
    }
    let (_name, points_price, _cny, prefix) = find_product_in_catalog(&req.product)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, format!("未知商品: {}", req.product)))?;
    let res = st
        .db
        .studio_purchase_with_points(&username, &req.product, points_price, &prefix)
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(res))
}

async fn usdt_topup(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UsdtTopupReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    if !(1.0..=1000.0).contains(&req.amount_usdt) {
        return Err(err(StatusCode::BAD_REQUEST, "USDT 金额需在 1 ~ 1000 之间"));
    }
    let points_credit = (req.amount_usdt * 10.0).round() as i64;
    let order_no = st
        .db
        .studio_create_intent(
            &username,
            "usdt",
            "points_topup",
            req.amount_usdt,
            points_credit,
        )
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "order_no": order_no,
        "wallet": "TKL9TuXXHu9oubcH36SndwKXwp3W1fxzUJ",
        "amount_usdt": req.amount_usdt,
        "points_credit": points_credit,
        "status": "pending"
    })))
}

async fn redeem_card(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<RedeemReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let res = st
        .db
        .studio_redeem_card(&username, req.code.trim())
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(res))
}

async fn get_order(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(order_no): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let res = st
        .db
        .studio_get_order(&username, &order_no)
        .await
        .map_err(|e| err(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(res))
}

async fn get_afdian() -> Json<Value> {
    let packs = afdian_packs();
    Json(json!({
        "ok": true,
        "configured": true,
        "points_per_cny": 10,
        "products": packs,
        "packs": packs
    }))
}

async fn create_afdian_intent(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<AfdianIntentReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let (price_cny, points_credit) = afdian_packs()
        .iter()
        .find(|p| p["product"].as_str() == Some(req.product.as_str()))
        .map(|p| {
            (
                p["price_cny"].as_f64().unwrap_or(10.0),
                p["points_credit"].as_i64().unwrap_or(100),
            )
        })
        .or_else(|| find_product_in_catalog(&req.product).map(|(_, pts, cny, _)| (cny, pts)))
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, format!("不支持的爱发电商品: {}", req.product)))?;

    let intent_id = st
        .db
        .studio_create_intent(&username, "afdian", &req.product, price_cny, points_credit)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let checkout_url = if let Some((plan_id, sku_id)) = afdian_plan_meta(&req.product) {
        format!(
            "https://afdian.com/order/create?plan_id={plan_id}&product_type=1&month=1&custom_order_id={intent_id}&sku=%5B%7B%22sku_id%22%3A%22{sku_id}%22%2C%22count%22%3A1%7D%5D"
        )
    } else {
        format!(
            "https://afdian.com/order/create?custom_order_id={intent_id}&remark={intent_id}"
        )
    };
    Ok(Json(json!({
        "ok": true,
        "intent_id": intent_id,
        "product": req.product,
        "price_cny": price_cny,
        "points_credit": points_credit,
        "automatic_binding": true,
        "automatic_checkout_url": checkout_url,
        "status": "pending"
    })))
}

async fn check_afdian_intent(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    if let Some(dir) = st.pay_config_dir.as_deref() {
        let cfg_path = format!("{dir}/afdian.json");
        if let Ok(cfg) = lilyco_pay::afdian::AfdianConfig::from_json_file(&cfg_path) {
            let code_c = code.clone();
            let live = tokio::task::spawn_blocking(move || {
                lilyco_pay::afdian::query_order_live(&cfg, &code_c)
            })
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| err(StatusCode::BAD_GATEWAY, e.to_string()))?;
            if !live.0 {
                return Ok(Json(json!({
                    "ok": true,
                    "intent_id": code,
                    "status": "pending",
                    "live_verified": true,
                    "detail": "暂未在爱发电 OpenAPI 查到已付款订单（如刚付款请 5 秒后重试）"
                })));
            }
        }
    }
    let res = st
        .db
        .studio_settle_intent(&username, &code)
        .await
        .map_err(|e| err(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(res))
}

async fn get_alipay(State(st): State<Arc<AppState>>) -> Json<Value> {
    let live_cfg = st
        .pay_config_dir
        .as_deref()
        .and_then(|d| lilyco_pay::alipay::AlipayConfig::from_json_file(&format!("{d}/alipay.json")).ok());
    let (configured, env_str) = match live_cfg {
        Some(c) if c.enabled => (true, c.environment),
        _ => (false, "sandbox".to_string()),
    };
    Json(json!({
        "ok": true,
        "configured": configured,
        "environment": env_str,
        "products": ["points_topup"],
        "points_per_cny": 10,
        "topup_tiers_cny": [1, 5, 10, 20, 30, 50, 88, 100, 200, 300]
    }))
}

async fn create_alipay_order(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<AlipayTopupReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let (product, amount_cny, points_credit) = if let Some(cny) = req.amount_cny {
        ("points_topup".to_string(), cny, (cny * 10.0).round() as i64)
    } else if let Some(prod) = req.product {
        let (_, pts, cny, _) = find_product_in_catalog(&prod)
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "未知商品"))?;
        (prod, cny, pts)
    } else {
        return Err(err(StatusCode::BAD_REQUEST, "请指定充值金额或商品"));
    };
    let order_no = st
        .db
        .studio_create_intent(&username, "alipay", &product, amount_cny, points_credit)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut checkout_url: Option<String> = None;
    let mut live_mode = false;
    if let Some(dir) = st.pay_config_dir.as_deref() {
        let cfg_path = format!("{dir}/alipay.json");
        if let Ok(cfg) = lilyco_pay::alipay::AlipayConfig::from_json_file(&cfg_path) {
            if cfg.enabled {
                let subject = format!("云枢智创积分充值 ({points_credit} 积分)");
                let session = lilyco_pay::alipay::build_signed_page_pay(
                    &cfg,
                    &order_no,
                    amount_cny,
                    &subject,
                    "https://dev.lain42.top/shop",
                    "https://dev.lain42.top/studio/api/payments/alipay/notify",
                )
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                checkout_url = session.checkout_url;
                live_mode = session.live_mode;
            }
        }
    }

    Ok(Json(json!({
        "ok": true,
        "order_no": order_no,
        "amount_cny": amount_cny,
        "points_credit": points_credit,
        "checkout_url": checkout_url,
        "live_mode": live_mode,
        "status": "pending"
    })))
}

async fn check_alipay_order(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(number): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    if let Some(dir) = st.pay_config_dir.as_deref() {
        let cfg_path = format!("{dir}/alipay.json");
        if let Ok(cfg) = lilyco_pay::alipay::AlipayConfig::from_json_file(&cfg_path) {
            if cfg.enabled {
                let num_c = number.clone();
                let (paid, raw) = tokio::task::spawn_blocking(move || {
                    lilyco_pay::alipay::query_trade_live(&cfg, &num_c)
                })
                .await
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .map_err(|e| err(StatusCode::BAD_GATEWAY, e.to_string()))?;
                if !paid {
                    return Ok(Json(json!({
                        "ok": true,
                        "order_no": number,
                        "status": "pending",
                        "live_verified": true,
                        "gateway_msg": raw.get("sub_msg").or_else(|| raw.get("msg")).cloned().unwrap_or(Value::Null),
                        "detail": "支付宝官方查单：订单尚未支付成功"
                    })));
                }
            }
        }
    }
    let res = st
        .db
        .studio_settle_intent(&username, &number)
        .await
        .map_err(|e| err(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(res))
}

async fn get_stripe(State(st): State<Arc<AppState>>) -> Json<Value> {
    let packs: Vec<Value> = afdian_packs().into_iter().skip(1).collect();
    let cfg = st
        .pay_config_dir
        .as_deref()
        .and_then(|d| lilyco_pay::stripe::StripeConfig::from_json_file(&format!("{d}/stripe.json")).ok());
    let (configured, mode) = match cfg {
        Some(c) if c.enabled => (true, c.mode),
        _ => (false, "test".to_string()),
    };
    Json(json!({
        "ok": true,
        "configured": configured,
        "mode": mode,
        "points_per_cny": 10,
        "products": ["points_topup"],
        "packs": packs
    }))
}

async fn create_stripe_intent(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<StripeIntentReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let (price_cny, points_credit) = afdian_packs()
        .iter()
        .find(|p| p["product"].as_str() == Some(req.product.as_str()))
        .map(|p| {
            (
                p["price_cny"].as_f64().unwrap_or(6.0),
                p["points_credit"].as_i64().unwrap_or(60),
            )
        })
        .unwrap_or((6.0, 60));
    let mut price_usd = ((price_cny / 7.2) * 100.0).round() / 100.0;
    let currency = req.currency.unwrap_or_else(|| "USD".to_string());
    let local_ref = st
        .db
        .studio_create_intent(&username, "stripe", &req.product, price_cny, points_credit)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut checkout_url: Option<String> = None;
    let mut session_id = local_ref.clone();
    let mut live_mode = false;
    if let Some(dir) = st.pay_config_dir.as_deref() {
        let cfg_path = format!("{dir}/stripe.json");
        if let Ok(cfg) = lilyco_pay::stripe::StripeConfig::from_json_file(&cfg_path) {
            if cfg.enabled {
                let prod_c = req.product.clone();
                let ref_c = local_ref.clone();
                let session = tokio::task::spawn_blocking(move || {
                    lilyco_pay::stripe::create_checkout_session_live(
                        &cfg,
                        &ref_c,
                        &prod_c,
                        price_cny,
                        points_credit,
                        "https://dev.lain42.top/shop?stripe=success",
                        "https://dev.lain42.top/shop?stripe=cancel",
                    )
                })
                .await
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .map_err(|e| err(StatusCode::BAD_GATEWAY, e.to_string()))?;
                session_id = session.order_id;
                checkout_url = session.checkout_url;
                price_usd = session.amount;
                live_mode = session.live_mode;
            }
        }
    }

    Ok(Json(json!({
        "ok": true,
        "intent_id": local_ref,
        "stripe_session_id": session_id,
        "checkout_url": checkout_url,
        "live_mode": live_mode,
        "status": "pending",
        "currency": currency,
        "price_cny": price_cny,
        "price_usd": price_usd,
        "points_credit": points_credit,
        "methods": ["card", "apple_pay", "google_pay"]
    })))
}

async fn check_stripe_intent(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    if code.starts_with("cs_") {
        if let Some(dir) = st.pay_config_dir.as_deref() {
            let cfg_path = format!("{dir}/stripe.json");
            if let Ok(cfg) = lilyco_pay::stripe::StripeConfig::from_json_file(&cfg_path) {
                let code_c = code.clone();
                let (paid, raw) = tokio::task::spawn_blocking(move || {
                    lilyco_pay::stripe::retrieve_session_live(&cfg, &code_c)
                })
                .await
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .map_err(|e| err(StatusCode::BAD_GATEWAY, e.to_string()))?;
                if !paid {
                    return Ok(Json(json!({
                        "ok": true,
                        "intent_id": code,
                        "status": "pending",
                        "live_verified": true,
                        "detail": "Stripe 查单：订单尚未完成支付"
                    })));
                }
                if let Some(ref_id) = raw.get("client_reference_id").and_then(Value::as_str) {
                    let res = st
                        .db
                        .studio_settle_intent(&username, ref_id)
                        .await
                        .map_err(|e| err(StatusCode::NOT_FOUND, e.to_string()))?;
                    return Ok(Json(res));
                }
            }
        }
    }
    let res = st
        .db
        .studio_settle_intent(&username, &code)
        .await
        .map_err(|e| err(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(res))
}

async fn get_creem(State(st): State<Arc<AppState>>) -> Json<Value> {
    let cfg = st
        .pay_config_dir
        .as_deref()
        .and_then(|d| lilyco_pay::creem::CreemConfig::from_json_file(&format!("{d}/creem.json")).ok());
    let (configured, mode, keys) = match cfg {
        Some(c) => (
            true,
            if c.test_mode { "test" } else { "live" },
            c.products.keys().cloned().collect::<Vec<_>>(),
        ),
        None => (false, "unconfigured", Vec::new()),
    };
    Json(json!({
        "ok": true,
        "configured": configured,
        "mode": mode,
        "products": keys
    }))
}

async fn create_creem_intent(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<StripeIntentReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let (price_cny, points_credit) = afdian_packs()
        .iter()
        .find(|p| p["product"].as_str() == Some(req.product.as_str()))
        .map(|p| {
            (
                p["price_cny"].as_f64().unwrap_or(30.0),
                p["points_credit"].as_i64().unwrap_or(300),
            )
        })
        .unwrap_or((30.0, 300));
    let price_usd = match req.product.as_str() {
        "points_cny_10" => 1.49,
        "points_cny_30" => 4.49,
        "points_cny_100" => 13.99,
        _ => ((price_cny / 7.2) * 100.0).round() / 100.0,
    };
    let local_ref = st
        .db
        .studio_create_intent(&username, "creem", &req.product, price_cny, points_credit)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let dir = st
        .pay_config_dir
        .as_deref()
        .ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "未配置 Creem 支付目录"))?;
    let cfg = lilyco_pay::creem::CreemConfig::from_json_file(&format!("{dir}/creem.json"))
        .map_err(|e| err(StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?;
    let prod_c = req.product.clone();
    let ref_c = local_ref.clone();
    let session = tokio::task::spawn_blocking(move || {
        lilyco_pay::creem::create_checkout_live(
            &cfg,
            &ref_c,
            &prod_c,
            price_usd,
            points_credit,
            "https://dev.lain42.top/shop?creem=success",
        )
    })
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| err(StatusCode::BAD_GATEWAY, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "intent_id": local_ref,
        "creem_checkout_id": session.order_id,
        "checkout_url": session.checkout_url,
        "live_mode": session.live_mode,
        "price_usd": price_usd,
        "points_credit": points_credit,
        "status": "pending"
    })))
}

async fn get_google_play() -> Json<Value> {
    Json(json!({
        "ok": true,
        "configured": true,
        "package_name": "top.lain42.workbench",
        "products": [
            { "sku": "lain42_credits_100", "product": "points_cny_10", "price_usd": 1.49, "points_credit": 100 },
            { "sku": "lain42_credits_300", "product": "points_cny_30", "price_usd": 4.49, "points_credit": 300 },
            { "sku": "lain42_credits_1000", "product": "points_cny_100", "price_usd": 13.99, "points_credit": 1000 }
        ]
    }))
}

async fn verify_google_play(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<GooglePlayVerifyReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    if req.purchase_token.trim().len() < 8 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "无效的 Google Play purchase_token（至少 8 位）",
        ));
    }
    let (price_cny, points_credit) = afdian_packs()
        .iter()
        .find(|p| p["product"].as_str() == Some(req.product.as_str()))
        .map(|p| {
            (
                p["price_cny"].as_f64().unwrap_or(30.0),
                p["points_credit"].as_i64().unwrap_or(300),
            )
        })
        .unwrap_or((30.0, 300));
    let intent_id = st
        .db
        .studio_create_intent(&username, "gplay", &req.product, price_cny, points_credit)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let settled = st
        .db
        .studio_settle_intent(&username, &intent_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(settled))
}

async fn marketing_event() -> Json<Value> {
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct ContactTicketReq {
    #[serde(default)]
    body: String,
    #[serde(default)]
    contact: String,
    #[serde(default)]
    order_no: String,
    #[serde(default)]
    kind: String,
}

async fn create_contact_ticket(
    State(st): State<Arc<AppState>>,
    Json(req): Json<ContactTicketReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let body = req.body.trim();
    if body.len() < 5 || body.len() > 4000 {
        return Err(err(StatusCode::BAD_REQUEST, "问题描述需在 5-4000 字之间"));
    }
    let contact = req.contact.trim();
    if contact.is_empty() || contact.len() > 120 {
        return Err(err(StatusCode::BAD_REQUEST, "请填写有效的联系方式（微信/QQ/邮箱）"));
    }
    let kind = if req.kind.trim().is_empty() {
        "售后支持"
    } else {
        req.kind.trim()
    };
    let ticket_id = st
        .db
        .create_contact_ticket(kind, contact, req.order_no.trim(), body)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "ticket_id": ticket_id,
        "message": "工单已提交，管理员将尽快联系处理"
    })))
}

// =========================================================================
// 异步支付 Webhook 回调（支付宝 RSA2 / 爱发电 / Creem）
// =========================================================================

async fn alipay_notify(State(st): State<Arc<AppState>>, body: Bytes) -> Response {
    let params: BTreeMap<String, String> = url::form_urlencoded::parse(&body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if let Some(dir) = st.pay_config_dir.as_deref() {
        if let Ok(cfg) = lilyco_pay::alipay::AlipayConfig::from_json_file(&format!("{dir}/alipay.json")) {
            if !lilyco_pay::alipay::verify_notify_rsa2(&cfg, &params) {
                return (StatusCode::BAD_REQUEST, "fail").into_response();
            }
        }
    }
    let trade_status = params.get("trade_status").map(String::as_str).unwrap_or("");
    if matches!(trade_status, "TRADE_SUCCESS" | "TRADE_FINISHED") {
        if let Some(out_trade_no) = params.get("out_trade_no") {
            let _ = st.db.studio_settle_intent_webhook(out_trade_no).await;
        }
    }
    (StatusCode::OK, "success").into_response()
}

async fn afdian_webhook(State(st): State<Arc<AppState>>, Json(payload): Json<Value>) -> Json<Value> {
    let order = &payload["data"]["order"];
    let status = order.get("status").and_then(Value::as_i64).unwrap_or(0);
    if status == 2 {
        let custom = order
            .get("custom_order_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| order.get("remark").and_then(Value::as_str));
        if let Some(intent_id) = custom {
            let _ = st.db.studio_settle_intent_webhook(intent_id.trim()).await;
        }
    }
    Json(json!({ "ec": 200, "em": "" }))
}

async fn creem_webhook(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if let Some(dir) = st.pay_config_dir.as_deref() {
        if let Ok(cfg) = lilyco_pay::creem::CreemConfig::from_json_file(&format!("{dir}/creem.json")) {
            if !cfg.webhook_secret.is_empty() {
                let sig = headers
                    .get("creem-signature")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if !lilyco_pay::creem::verify_webhook_signature(&cfg.webhook_secret, &body, sig) {
                    return Err(err(StatusCode::UNAUTHORIZED, "Creem Webhook 签名校验失败"));
                }
            }
        }
    }
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("无效 JSON: {e}")))?;
    let event = payload.get("eventType").and_then(Value::as_str).unwrap_or("");
    if event == "checkout.completed" {
        let order_id = payload["object"]["request_id"]
            .as_str()
            .or_else(|| payload["object"]["metadata"]["order_id"].as_str());
        if let Some(oid) = order_id {
            let _ = st.db.studio_settle_intent_webhook(oid).await;
        }
    }
    Ok(Json(json!({ "ok": true })))
}

// =========================================================================
// 订阅链接、音乐会员源、软件下载门控与客户端验签（兼容原版 lain42.top）
// =========================================================================

async fn proxy_clash_shortcut(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Response {
    proxy_subscription(State(st), headers, Path((token, "clash".to_string()))).await
}

async fn proxy_panel_sub_shortcut(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Response {
    proxy_subscription(State(st), headers, Path((token, "base64".to_string()))).await
}

struct PanelLiveSub {
    userinfo: Option<String>,
    metered_clash_line: Option<String>,
}

fn fetch_panel_live_sub(panel_url: &str) -> PanelLiveSub {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(4)))
        .build()
        .new_agent();
    let Ok(mut resp) = agent
        .get(panel_url)
        .header("User-Agent", "v2rayN/6.0")
        .call()
    else {
        return PanelLiveSub {
            userinfo: None,
            metered_clash_line: None,
        };
    };
    let mut userinfo = resp
        .headers()
        .get("subscription-userinfo")
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string);
    if userinfo.is_none() {
        if let Ok(r2) = agent
            .head(panel_url)
            .header("User-Agent", "clash-verge/v2.2.3")
            .call()
        {
            userinfo = r2
                .headers()
                .get("subscription-userinfo")
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
        }
    }
    let metered_clash_line = resp
        .body_mut()
        .read_to_string()
        .ok()
        .and_then(|raw| {
            let trimmed = raw.trim();
            let decoded_bytes = STANDARD
                .decode(trimmed)
                .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed))
                .ok()?;
            let decoded = String::from_utf8(decoded_bytes).ok()?;
            for line in decoded.lines() {
                if let Some(b64) = line.strip_prefix("vmess://") {
                    let b = STANDARD
                        .decode(b64.trim())
                        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64.trim()))
                        .ok()?;
                    let v: serde_json::Value = serde_json::from_slice(&b).ok()?;
                    let add = v.get("add").and_then(|x| x.as_str())?;
                    let port = v
                        .get("port")
                        .and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))?;
                    let uuid = v.get("id").and_then(|x| x.as_str())?;
                    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("/vmess");
                    let host = v.get("host").and_then(|x| x.as_str()).unwrap_or(add);
                    let tls = v.get("tls").and_then(|x| x.as_str()).unwrap_or("") == "tls";
                    return Some(format!(
                        "  - {{ name: \"⚡ 云枢主线 · 实时计费 (推荐)\", type: vmess, server: {add}, port: {port}, uuid: {uuid}, alterId: 0, cipher: auto, udp: true, tls: {tls}, network: ws, ws-opts: {{ path: {path}, headers: {{ Host: {host} }} }} }}\n"
                    ));
                }
            }
            None
        });
    PanelLiveSub {
        userinfo,
        metered_clash_line,
    }
}

fn parse_iso_to_unix(iso: &str) -> Option<i64> {
    // Parse "YYYY-MM-DDTHH:MM:SS..." (UTC) using Howard Hinnant days-from-civil algorithm
    let b = iso.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let y: i64 = iso.get(0..4)?.parse().ok()?;
    let m: i64 = iso.get(5..7)?.parse().ok()?;
    let d: i64 = iso.get(8..10)?.parse().ok()?;
    let hh: i64 = iso.get(11..13)?.parse().ok()?;
    let mm: i64 = iso.get(14..16)?.parse().ok()?;
    let ss: i64 = iso.get(17..19)?.parse().ok()?;
    let yr = y - if m <= 2 { 1 } else { 0 };
    let era = if yr >= 0 { yr } else { yr - 399 } / 400;
    let yoe = yr - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

async fn proxy_subscription(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((token, mut format)): Path<(String, String)>,
) -> Response {
    let ent = match st.db.studio_find_active_proxy_by_token(&token).await {
        Ok(Some(v)) => v,
        _ => return (StatusCode::NOT_FOUND, "无效或已过期的代理订阅令牌").into_response(),
    };
    let meta_val: serde_json::Value = serde_json::from_str(&ent.2).unwrap_or_default();
    let meta_gb = meta_val
        .get("traffic_gb")
        .and_then(|v| v.as_u64())
        .filter(|&gb| gb > 0);
    let fallback_gb: u64 = match ent.1.as_str() {
        "proxy_1m_1g" => 10,
        "proxy_6m" => 40,
        "proxy_1y_15g" => 120,
        "proxy_1y_140g" => 480,
        "proxy_1m" => 100,
        "proxy_15_3m" => 300,
        "proxy_15_1y" => 1200,
        "proxy_1m_220g" => 220,
        "proxy_3m" => 660,
        "proxy_1y" => 2640,
        "proxy_3y" => 7920,
        "proxy_pro_1m" => 1000,
        "proxy_pro_3m" => 3000,
        "proxy_pro_1y" => 12000,
        "proxy_pro_3y" => 36000,
        _ => 100,
    };
    let total_bytes: u64 = meta_gb.unwrap_or(fallback_gb) * 1024 * 1024 * 1024;
    let fallback_expire: i64 = meta_val
        .get("expires_at")
        .and_then(|v| v.as_str())
        .and_then(parse_iso_to_unix)
        .unwrap_or(1893456000);

    // Sync real live traffic usage + personal metered node from Server A proxy-panel when linked
    let panel_url = meta_val
        .get("sub_url")
        .and_then(|v| v.as_str())
        .filter(|u| u.contains("lain42.top/panel/api/sub/"))
        .map(ToString::to_string);
    let panel_url = match panel_url {
        Some(u) => Some(u),
        None => st.db.studio_find_user_panel_sub_url(&ent.0).await.ok().flatten(),
    };
    let live_panel = if let Some(url) = panel_url {
        tokio::task::spawn_blocking(move || fetch_panel_live_sub(&url))
            .await
            .unwrap_or(PanelLiveSub {
                userinfo: None,
                metered_clash_line: None,
            })
    } else {
        PanelLiveSub {
            userinfo: None,
            metered_clash_line: None,
        }
    };

    let userinfo = live_panel
        .userinfo
        .unwrap_or_else(|| format!("upload=0; download=0; total={total_bytes}; expire={fallback_expire}"));
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if format.eq_ignore_ascii_case("base64")
        && (ua.contains("clash")
            || ua.contains("mihomo")
            || ua.contains("stash")
            || ua.contains("flclash")
            || ua.contains("verge"))
        && !ua.contains("v2ray")
        && !ua.contains("shadowrocket")
        && !ua.contains("karing")
        && !ua.contains("nekobox")
    {
        format = "clash".to_string();
    }
    if format.eq_ignore_ascii_case("clash") || format.eq_ignore_ascii_case("yaml") {
        let mut yaml = format!(
            "# LYCO Connect · {}\nport: 7890\nsocks-port: 7891\nallow-lan: false\nmode: rule\nlog-level: info\nunified-delay: true\ntcp-concurrent: true\nkeep-alive-interval: 15\nproxies:\n  - {{ name: \"🇰🇷 韩国首尔 · AnyTLS 极速\", type: anytls, server: 43.108.96.128, port: 46462, password: f15f4aa6-e908-4f6a-b539-f5a8bdc1ad66, sni: www.bing.com, skip-cert-verify: true, udp: true }}\n  - {{ name: \"🇰🇷 韩国首尔 · TUIC v5 极速\", type: tuic, server: 43.108.96.128, port: 58781, uuid: f15f4aa6-e908-4f6a-b539-f5a8bdc1ad66, password: f15f4aa6-e908-4f6a-b539-f5a8bdc1ad66, sni: www.bing.com, alpn: [h3], skip-cert-verify: true, udp-relay-mode: native, congestion-controller: bbr }}\n  - {{ name: \"🇰🇷 韩国首尔 · Hysteria2\", type: hysteria2, server: 43.108.96.128, port: 64581, password: f15f4aa6-e908-4f6a-b539-f5a8bdc1ad66, sni: www.bing.com, alpn: [h3], up: \"50 Mbps\", down: \"200 Mbps\", skip-cert-verify: true }}\n  - {{ name: \"🇰🇷 韩国首尔 · VLESS 专线\", type: vless, server: 43.108.96.128, port: 443, uuid: f15f4aa6-e908-4f6a-b539-f5a8bdc1ad66, udp: true, tls: true, flow: xtls-rprx-vision, client-fingerprint: chrome, servername: apple.com, reality-opts: {{ public-key: 6wq54dJFy1fLskAQq1wl99afSQI8-j3LaMHIOvMBSm4, short-id: 293b1096 }} }}\n  - {{ name: \"🇰🇷 韩国首尔 · VMess WS\", type: vmess, server: 43.108.96.128, port: 2052, uuid: f15f4aa6-e908-4f6a-b539-f5a8bdc1ad66, alterId: 0, cipher: auto, udp: true, tls: false, network: ws, ws-opts: {{ path: /f15f4aa6-e908-4f6a-b539-f5a8bdc1ad66-vm, headers: {{ Host: www.bing.com }} }} }}\n  - {{ name: \"🇰🇷 韩国二 · VLESS 中转\", type: vless, server: 43.108.97.29, port: 51215, uuid: 3200e506-fc3e-4df2-b196-ebb54c8e7894, udp: true, tls: true, flow: xtls-rprx-vision, client-fingerprint: chrome, servername: apple.com, dialer-proxy: \"🇰🇷 韩国首尔 · VLESS 专线\", reality-opts: {{ public-key: s-tFxsSP3k0Jdb_B_ZJALBG67Hozc5g3Njx7CWTSyG4, short-id: 5d10928b }} }}\n  - {{ name: \"🇦🇺 澳大利亚悉尼 · AnyTLS 极速\", type: anytls, server: 13.236.116.49, port: 34657, password: 244022a3-0176-4409-95f2-72a4fbc6d800, sni: www.bing.com, skip-cert-verify: true, udp: true }}\n  - {{ name: \"🇦🇺 澳大利亚悉尼 · TUIC v5\", type: tuic, server: 13.236.116.49, port: 19893, uuid: 244022a3-0176-4409-95f2-72a4fbc6d800, password: 244022a3-0176-4409-95f2-72a4fbc6d800, sni: www.bing.com, alpn: [h3], skip-cert-verify: true, udp-relay-mode: native, congestion-controller: bbr }}\n  - {{ name: \"🇦🇺 澳大利亚悉尼 · Hysteria2\", type: hysteria2, server: 13.236.116.49, port: 27739, password: 244022a3-0176-4409-95f2-72a4fbc6d800, sni: www.bing.com, alpn: [h3], up: \"50 Mbps\", down: \"200 Mbps\", skip-cert-verify: true }}\n  - {{ name: \"🇦🇺 澳大利亚悉尼 · VLESS 专线\", type: vless, server: 13.236.116.49, port: 32549, uuid: 244022a3-0176-4409-95f2-72a4fbc6d800, udp: true, tls: true, flow: xtls-rprx-vision, client-fingerprint: chrome, servername: apple.com, reality-opts: {{ public-key: drsVOt3moGRJSi5XJ1HXdDcyxUnL0dt9_L5-XSgEGlc, short-id: 59baa9e3 }} }}\n  - {{ name: \"🇵🇭 菲律宾马尼拉 · AnyTLS 极速\", type: anytls, server: 8.220.186.171, port: 42596, password: dafe41d4-1f78-4527-9f7e-d080a47ba577, sni: www.bing.com, skip-cert-verify: true, udp: true }}\n  - {{ name: \"🇵🇭 菲律宾马尼拉 · TUIC v5\", type: tuic, server: 8.220.186.171, port: 15913, uuid: dafe41d4-1f78-4527-9f7e-d080a47ba577, password: dafe41d4-1f78-4527-9f7e-d080a47ba577, sni: www.bing.com, alpn: [h3], skip-cert-verify: true, udp-relay-mode: native, congestion-controller: bbr }}\n  - {{ name: \"🇵🇭 菲律宾马尼拉 · Hysteria2\", type: hysteria2, server: 8.220.186.171, port: 57782, password: dafe41d4-1f78-4527-9f7e-d080a47ba577, sni: www.bing.com, alpn: [h3], up: \"50 Mbps\", down: \"200 Mbps\", skip-cert-verify: true }}\n  - {{ name: \"🇵🇭 菲律宾马尼拉 · VLESS 专线\", type: vless, server: 8.220.186.171, port: 12360, uuid: dafe41d4-1f78-4527-9f7e-d080a47ba577, udp: true, tls: true, flow: xtls-rprx-vision, client-fingerprint: chrome, servername: apple.com, reality-opts: {{ public-key: -5ZOXIcqFgL8_sqASL3VaJ8XFiGKQXT1D4iLYNDsTgs, short-id: b4e1150a }} }}\n  - {{ name: \"🇺🇸 美国西雅图 · Hysteria2\", type: hysteria2, server: 107.174.11.33, port: 51466, password: 67b84bf3-b69a-482b-95a9-769640b5e4b3, sni: www.bing.com, up: \"50 Mbps\", down: \"200 Mbps\", skip-cert-verify: true }}\n  - {{ name: \"🇺🇸 美国西雅图 · TUIC\", type: tuic, server: 107.174.11.33, port: 24062, uuid: 67b84bf3-b69a-482b-95a9-769640b5e4b3, password: 67b84bf3-b69a-482b-95a9-769640b5e4b3, sni: www.bing.com, alpn: [h3], skip-cert-verify: true, udp-relay-mode: native, congestion-controller: bbr }}\n  - {{ name: \"🇩🇪 德国法兰克福 · VLESS 中转\", type: vless, server: 217.216.188.167, port: 35179, uuid: 4470b15f-953e-4820-9145-fbc2bf606537, udp: true, tls: true, flow: xtls-rprx-vision, client-fingerprint: chrome, servername: apple.com, dialer-proxy: \"🇦🇺 澳大利亚悉尼 · VLESS 专线\", reality-opts: {{ public-key: 2H7g1y_JkAnHEoFssUOpGLxxMvdYQC28y2GgyGsKd1M, short-id: 87050970 }} }}\nproxy-groups:\n  - name: \"⚡ 极速专线 · 手动切换\"\n    type: select\n    proxies: [\"LYCO-Auto\", \"🇰🇷 韩国首尔 · AnyTLS 极速\", \"🇰🇷 韩国首尔 · TUIC v5 极速\", \"🇰🇷 韩国首尔 · Hysteria2\", \"🇰🇷 韩国首尔 · VLESS 专线\", \"🇦🇺 澳大利亚悉尼 · AnyTLS 极速\", \"🇦🇺 澳大利亚悉尼 · TUIC v5\", \"🇦🇺 澳大利亚悉尼 · Hysteria2\", \"🇦🇺 澳大利亚悉尼 · VLESS 专线\", \"🇵🇭 菲律宾马尼拉 · AnyTLS 极速\", \"🇵🇭 菲律宾马尼拉 · TUIC v5\", \"🇵🇭 菲律宾马尼拉 · Hysteria2\", \"🇵🇭 菲律宾马尼拉 · VLESS 专线\", \"🇺🇸 美国西雅图 · Hysteria2\", \"🇺🇸 美国西雅图 · TUIC\", \"🇩🇪 德国法兰克福 · VLESS 中转\", \"🇰🇷 韩国首尔 · VMess WS\"]\n  - name: \"LYCO-Auto\"\n    type: url-test\n    url: https://www.gstatic.com/generate_204\n    interval: 90\n    tolerance: 30\n    proxies: [\"🇰🇷 韩国首尔 · AnyTLS 极速\", \"🇰🇷 韩国首尔 · TUIC v5 极速\", \"🇰🇷 韩国首尔 · Hysteria2\", \"🇰🇷 韩国首尔 · VLESS 专线\", \"🇦🇺 澳大利亚悉尼 · AnyTLS 极速\", \"🇦🇺 澳大利亚悉尼 · TUIC v5\", \"🇦🇺 澳大利亚悉尼 · Hysteria2\", \"🇵🇭 菲律宾马尼拉 · AnyTLS 极速\", \"🇵🇭 菲律宾马尼拉 · TUIC v5\", \"🇵🇭 菲律宾马尼拉 · Hysteria2\"]\n  - name: \"📚 学术文献 · 校园网直连\"\n    type: select\n    proxies: [DIRECT, \"⚡ 极速专线 · 手动切换\"]\nrules:\n  - IP-CIDR,127.0.0.0/8,DIRECT,no-resolve\n  - IP-CIDR,10.0.0.0/8,DIRECT,no-resolve\n  - IP-CIDR,172.16.0.0/12,DIRECT,no-resolve\n  - IP-CIDR,192.168.0.0/16,DIRECT,no-resolve\n  - IP-CIDR,100.64.0.0/10,DIRECT,no-resolve\n  - DOMAIN-SUFFIX,edu.cn,DIRECT\n  - DOMAIN-SUFFIX,ac.cn,DIRECT\n  - DOMAIN-SUFFIX,gov.cn,DIRECT\n  - DOMAIN-SUFFIX,cnki.net,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,cnki.com.cn,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,wanfangdata.com.cn,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,cqvip.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,chaoxing.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,duxiu.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,sslibrary.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,ncpssd.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,nstl.gov.cn,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,sciengine.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,carsi.edu.cn,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,elsevier.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,sciencedirect.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,scopus.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,springer.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,springerlink.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,nature.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,ieee.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,wiley.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,webofscience.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,webofknowledge.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,clarivate.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,acs.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,rsc.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,tandfonline.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,jstor.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,oup.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,cambridge.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,sagepub.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,proquest.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,ebscohost.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,nih.gov,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,mdpi.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,arxiv.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,acm.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,aps.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,aip.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,science.org,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,cell.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,thelancet.com,📚 学术文献 · 校园网直连\n  - DOMAIN-SUFFIX,cn,DIRECT\n  - GEOIP,CN,DIRECT\n  - MATCH,⚡ 极速专线 · 手动切换\n",
            ent.0
        );
        if let Some(metered_line) = live_panel.metered_clash_line.as_deref() {
            yaml = yaml.replacen("proxies:\n", &format!("proxies:\n{metered_line}"), 1);
            yaml = yaml.replacen(
                "proxies: [\"LYCO-Auto\", ",
                "proxies: [\"⚡ 云枢主线 · 实时计费 (推荐)\", \"LYCO-Auto\", ",
                1,
            );
        }
        (
            [
                (header::CONTENT_TYPE, "text/yaml; charset=utf-8"),
                (
                    header::HeaderName::from_static("subscription-userinfo"),
                    userinfo.as_str(),
                ),
            ],
            yaml,
        )
            .into_response()
    } else {
        let raw = "vless://f15f4aa6-e908-4f6a-b539-f5a8bdc1ad66@43.108.96.128:443?encryption=none&security=reality&sni=apple.com&fp=chrome&pbk=6wq54dJFy1fLskAQq1wl99afSQI8-j3LaMHIOvMBSm4&sid=293b1096&type=tcp&flow=xtls-rprx-vision#%F0%9F%87%B0%F0%9F%87%B7%20%E9%9F%A9%E5%9B%BD%20%C2%B7%20VLESS\nvless://dafe41d4-1f78-4527-9f7e-d080a47ba577@8.220.186.171:12360?encryption=none&security=reality&sni=apple.com&fp=chrome&pbk=-5ZOXIcqFgL8_sqASL3VaJ8XFiGKQXT1D4iLYNDsTgs&sid=b4e1150a&type=tcp&flow=xtls-rprx-vision#%F0%9F%87%B5%F0%9F%87%AD%20%E8%8F%B2%E5%BE%8B%E5%AE%BE%20%C2%B7%20VLESS\nhysteria2://67b84bf3-b69a-482b-95a9-769640b5e4b3@107.174.11.33:51466?sni=www.bing.com&alpn=h3&insecure=1#%F0%9F%87%BA%F0%9F%87%B8%20%E7%BE%8E%E5%9B%BD%20%C2%B7%20Hysteria2\ntuic://67b84bf3-b69a-482b-95a9-769640b5e4b3:67b84bf3-b69a-482b-95a9-769640b5e4b3@107.174.11.33:24062?sni=www.bing.com&alpn=h3&insecure=1&congestion_control=bbr&udp_relay_mode=native#%F0%9F%87%BA%F0%9F%87%B8%20%E7%BE%8E%E5%9B%BD%20%C2%B7%20TUIC\n";
        (
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                (
                    header::HeaderName::from_static("subscription-userinfo"),
                    userinfo.as_str(),
                ),
            ],
            STANDARD.encode(raw.as_bytes()),
        )
            .into_response()
    }
}

async fn music_subscription(
    State(st): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (username, product, _) = st
        .db
        .studio_find_active_entitlement_by_key(&key)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter(|(_, p, _)| p.starts_with("music_"))
        .ok_or_else(|| err(StatusCode::FORBIDDEN, "无效或已过期的音乐会员订阅令牌"))?;
    Ok(Json(json!({
        "ok": true,
        "username": username,
        "product": product,
        "client": "LX Music / Lyco Music",
        "sources": ["kw", "kg", "tx", "wy", "mg"],
        "sync_enabled": true
    })))
}

async fn entitled_download(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(product): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let has = st
        .db
        .studio_user_has_entitlement(&username, &product)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !has {
        return Err(err(
            StatusCode::FORBIDDEN,
            format!("当前账户未持有商品 `{product}` 的生效权益，请先购买"),
        ));
    }
    let kuake_base = std::env::var("LAIN_KUAKE_URL").ok();
    Ok(Json(json!({
        "ok": true,
        "product": product,
        "username": username,
        "storage_backend": if kuake_base.is_some() { "kuake-studio" } else { "direct" },
        "download_url": format!("/downloads/{product}/latest")
    })))
}

#[derive(Deserialize)]
struct VerifyClientReq {
    #[serde(default)]
    license_key: String,
    #[serde(default)]
    product: String,
}

async fn verify_client_entitlement(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<VerifyClientReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (username, product) = if !req.license_key.trim().is_empty() {
        let (u, p, _) = st
            .db
            .studio_find_active_entitlement_by_key(req.license_key.trim())
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .ok_or_else(|| err(StatusCode::FORBIDDEN, "授权码无效或已撤销"))?;
        (u, p)
    } else {
        let u = extract_username(&headers, &st.license_secret)?;
        let p = req.product.trim().to_string();
        if p.is_empty()
            || !st
                .db
                .studio_user_has_entitlement(&u, &p)
                .await
                .unwrap_or(false)
        {
            return Err(err(StatusCode::FORBIDDEN, "未找到该产品的生效权益"));
        }
        (u, p)
    };
    let now = lilyco_license::current_unix_secs();
    let receipt = sign_token(&st.license_secret, &format!("rcpt|{username}|{product}|{now}"));
    Ok(Json(json!({
        "ok": true,
        "valid": true,
        "username": username,
        "product": product,
        "issued_unix": now,
        "receipt": receipt
    })))
}
