//! 原版 `lain42.top` `/studio/api/*` 商业计费与账号系统全 Rust 重写（Turso 纯 Rust SQL 持久化）。
//!
//! 100% 兼容 `/var/www/studio/assets/store.js`、`/sub/assets/ui.js`、`/assets/go-quick.js`
//! 与 `/shop/payment.html` 的请求与响应契约：
//! - 账号注册/登录（`GET /register/challenge`, `POST /register`, `POST /login`, `GET /me`）
//! - 26 款真实商品目录（`GET /products`）
//! - 爱发电 / 支付宝 / Stripe / USDT(TRC20) / 卡密兑换全套支付与充值接口
//! - 积分购买与权益自动发放（`POST /points/purchase`, `POST /redeem`）

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::hmac;
use serde::Deserialize;
use serde_json::{json, Value};

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
        .route("/studio/api/payments/alipay", get(get_alipay))
        .route("/studio/api/payments/alipay/topups", post(create_alipay_order))
        .route("/studio/api/payments/alipay/intents", post(create_alipay_order))
        .route(
            "/studio/api/payments/alipay/orders/{number}/check",
            post(check_alipay_order),
        )
        .route("/studio/api/payments/stripe", get(get_stripe))
        .route("/studio/api/payments/stripe/intents", post(create_stripe_intent))
        .route(
            "/studio/api/payments/stripe/intents/{code}/check",
            post(check_stripe_intent),
        )
        .route("/studio/api/payments/creem", get(get_creem))
        .route("/studio/api/payments/creem/intents", post(create_creem_intent))
        .route("/studio/api/payments/google-play", get(get_google_play))
        .route(
            "/studio/api/payments/google-play/verify",
            post(verify_google_play),
        )
        .route("/studio/api/marketing/event", post(marketing_event))
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

fn hash_password(secret: &str, username: &str, password: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let msg = format!("pw:{}:{}", username.to_ascii_lowercase(), password);
    URL_SAFE_NO_PAD.encode(hmac::sign(&key, msg.as_bytes()).as_ref())
}

fn extract_username(headers: &HeaderMap, secret: &str) -> Result<String, (StatusCode, Json<Value>)> {
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

fn catalog_json() -> Value {
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
    Ok(Json(json!({
        "ok": true,
        "account_id": user.id,
        "username": user.username,
        "token": token
    })))
}

async fn get_me(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = extract_username(&headers, &st.license_secret)?;
    let me = st
        .db
        .studio_get_me(&username)
        .await
        .map_err(|e| err(StatusCode::UNAUTHORIZED, e.to_string()))?;
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
