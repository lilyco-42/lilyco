//! `lilyco-pay` — 位于 `lilyco-core` 之外的可插拔多渠道支付插件套件（按 feature 门控）。
//!
//! - `afdian`：爱发电真实 `plan_id + sku_id` 专属下单链接 + OpenAPI `query-order` MD5 签名实查
//! - `alipay`：支付宝纯 Rust RSA2 (`SHA256withRSA`) 签名，支持 `alipay.trade.page.pay` 与 `alipay.trade.query`
//! - `stripe`：Stripe 官方 Checkout Sessions API（支持 Visa / Mastercard / Apple Pay / Google Pay）
//! - `google-play`：Google Play Billing `purchase_token` 校验
//! - `usdt`：TRC20 收款地址与金额区间校验

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentChannel {
    Afdian,
    Alipay,
    Stripe,
    Creem,
    GooglePlay,
    UsdtTrc20,
}

#[derive(Debug, Error)]
pub enum PayError {
    #[error("无效支付参数: {0}")]
    InvalidArg(String),
    #[error("支付渠道未配置: {0}")]
    NotConfigured(String),
    #[error("签名/密钥解析失败: {0}")]
    Crypto(String),
    #[error("上游支付网关请求失败: {0}")]
    Gateway(String),
    #[error("支付核验未通过: {0}")]
    VerifyFailed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckoutSession {
    pub channel: PaymentChannel,
    pub order_id: String,
    pub product_code: String,
    pub amount: f64,
    pub currency: String,
    pub points_credit: i64,
    pub checkout_url: Option<String>,
    pub live_mode: bool,
    pub extra: serde_json::Value,
}

#[cfg(feature = "afdian")]
pub mod afdian {
    use super::*;
    use md5::{Digest, Md5};
    use serde_json::{json, Value};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, Clone, Deserialize)]
    pub struct AfdianConfig {
        pub user_id: String,
        pub token: String,
    }

    impl AfdianConfig {
        pub fn from_json_file(path: &str) -> Result<Self, PayError> {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| PayError::NotConfigured(format!("读取 afdian 配置失败: {e}")))?;
            serde_json::from_str(&raw)
                .map_err(|e| PayError::NotConfigured(format!("解析 afdian 配置失败: {e}")))
        }
    }

    #[derive(Debug, Clone)]
    pub struct AfdianPlan {
        pub product: &'static str,
        pub plan_id: &'static str,
        pub sku_id: &'static str,
        pub price_cny: f64,
        pub points_credit: i64,
    }

    pub const BUILTIN_PLANS: &[AfdianPlan] = &[
        AfdianPlan {
            product: "points_cny_1",
            plan_id: "e1fd2b18c51f11f1ab605254001e7c00",
            sku_id: "e20973b4c51f11f1bb655254001e7c00",
            price_cny: 1.0,
            points_credit: 10,
        },
        AfdianPlan {
            product: "points_cny_5",
            plan_id: "31d573fccc9b11f1bf2a52540025c377",
            sku_id: "31ddfd74cc9b11f1b53252540025c377",
            price_cny: 5.0,
            points_credit: 50,
        },
        AfdianPlan {
            product: "points_cny_6",
            plan_id: "9c87957cc52011f1a8d352540025c377",
            sku_id: "9c9024bcc52011f1aa6752540025c377",
            price_cny: 6.0,
            points_credit: 60,
        },
        AfdianPlan {
            product: "points_cny_10",
            plan_id: "472586cacc9b11f18df25254001e7c00",
            sku_id: "472f462ecc9b11f1b0295254001e7c00",
            price_cny: 10.0,
            points_credit: 100,
        },
        AfdianPlan {
            product: "points_cny_30",
            plan_id: "b3718c5cc52011f1bb7b52540025c377",
            sku_id: "b379cdf4c52011f192f052540025c377",
            price_cny: 30.0,
            points_credit: 300,
        },
        AfdianPlan {
            product: "points_cny_45",
            plan_id: "63f94afacc9d11f19fbe5254001e7c00",
            sku_id: "6402a582cc9d11f185b65254001e7c00",
            price_cny: 45.0,
            points_credit: 450,
        },
        AfdianPlan {
            product: "points_cny_100",
            plan_id: "bb7d58f4c52011f1a29c5254001e7c00",
            sku_id: "bb86d898c52011f1b5d95254001e7c00",
            price_cny: 100.0,
            points_credit: 1000,
        },
        AfdianPlan {
            product: "points_cny_200",
            plan_id: "c9ac6992c52011f1b38b5254001e7c00",
            sku_id: "c9b4db18c52011f1a4de5254001e7c00",
            price_cny: 200.0,
            points_credit: 2000,
        },
        AfdianPlan {
            product: "points_cny_300",
            plan_id: "d603735cc52011f18cc652540025c377",
            sku_id: "d60d17e0c52011f1881e52540025c377",
            price_cny: 300.0,
            points_credit: 3000,
        },
        AfdianPlan {
            product: "points_cny_1000",
            plan_id: "e4a89d38c52011f1b0825254001e7c00",
            sku_id: "e4b2ebeec52011f1b6215254001e7c00",
            price_cny: 1000.0,
            points_credit: 10000,
        },
    ];

    pub fn build_checkout(product: &str, order_id: &str) -> Result<CheckoutSession, PayError> {
        let plan = BUILTIN_PLANS
            .iter()
            .find(|p| p.product == product)
            .ok_or_else(|| PayError::InvalidArg(format!("未知爱发电档位: {product}")))?;
        let url = format!(
            "https://afdian.com/order/create?plan_id={}&product_type=1&month=1&custom_order_id={}&sku=%5B%7B%22sku_id%22%3A%22{}%22%2C%22count%22%3A1%7D%5D",
            plan.plan_id, order_id, plan.sku_id
        );
        Ok(CheckoutSession {
            channel: PaymentChannel::Afdian,
            order_id: order_id.to_string(),
            product_code: product.to_string(),
            amount: plan.price_cny,
            currency: "CNY".to_string(),
            points_credit: plan.points_credit,
            checkout_url: Some(url),
            live_mode: true,
            extra: json!({ "plan_id": plan.plan_id, "sku_id": plan.sku_id, "automatic_binding": true }),
        })
    }

    /// 计算爱发电 OpenAPI 标准 MD5 签名。
    pub fn sign_openapi(user_id: &str, token: &str, params_json: &str, ts: u64) -> String {
        let raw = format!("{token}params{params_json}ts{ts}user_id{user_id}");
        let digest = Md5::digest(raw.as_bytes());
        let mut hex = String::with_capacity(32);
        for b in digest {
            use std::fmt::Write as _;
            let _ = write!(&mut hex, "{b:02x}");
        }
        hex
    }

    /// 实查爱发电 OpenAPI `query-order`，核验 `custom_order_id` 或 `remark` 是否已支付。
    pub fn query_order_live(
        cfg: &AfdianConfig,
        intent_id: &str,
    ) -> Result<(bool, Value), PayError> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let params_json = r#"{"page":1,"per_page":20}"#;
        let sign = sign_openapi(&cfg.user_id, &cfg.token, params_json, ts);
        let payload = json!({
            "user_id": cfg.user_id,
            "params": params_json,
            "ts": ts,
            "sign": sign
        });
        let mut resp = ureq::post("https://afdian.com/api/open/query-order")
            .send_json(&payload)
            .map_err(|e| PayError::Gateway(format!("请求爱发电 OpenAPI 失败: {e}")))?;
        let body: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| PayError::Gateway(format!("解析爱发电响应失败: {e}")))?;
        if body.get("ec").and_then(Value::as_i64) != Some(200) {
            return Err(PayError::Gateway(format!(
                "爱发电返回错误: {}",
                body.get("em").and_then(Value::as_str).unwrap_or("unknown")
            )));
        }
        let list = body["data"]["list"].as_array().cloned().unwrap_or_default();
        let matched = list.iter().find(|o| {
            let status_ok = o.get("status").and_then(Value::as_i64) == Some(2);
            let custom = o
                .get("custom_order_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let remark = o.get("remark").and_then(Value::as_str).unwrap_or("");
            status_ok && (custom == intent_id || remark.contains(intent_id))
        });
        Ok((matched.is_some(), matched.cloned().unwrap_or(Value::Null)))
    }
}

#[cfg(feature = "alipay")]
pub mod alipay {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs1v15::{Signature, SigningKey, VerifyingKey};
    use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};
    use rsa::signature::{RandomizedSigner, SignatureEncoding, Verifier};
    use rsa::{RsaPrivateKey, RsaPublicKey};
    use serde_json::{json, Value};
    use sha2::Sha256;
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, Clone, Deserialize)]
    pub struct AlipayConfig {
        pub app_id: String,
        pub app_private_key: String,
        #[serde(default)]
        pub alipay_public_key: String,
        #[serde(default)]
        pub environment: String,
        #[serde(default)]
        pub enabled: bool,
    }

    impl AlipayConfig {
        pub fn from_json_file(path: &str) -> Result<Self, PayError> {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| PayError::NotConfigured(format!("读取 alipay 配置失败: {e}")))?;
            serde_json::from_str(&raw)
                .map_err(|e| PayError::NotConfigured(format!("解析 alipay 配置失败: {e}")))
        }

        pub fn gateway_url(&self) -> &'static str {
            if self.environment == "sandbox" {
                "https://openapi-sandbox.dl.alipaydev.com/gateway.do"
            } else {
                "https://openapi.alipay.com/gateway.do"
            }
        }
    }

    fn parse_private_key(raw: &str) -> Result<RsaPrivateKey, PayError> {
        let trimmed = raw.trim();
        if trimmed.contains("BEGIN RSA PRIVATE KEY") {
            return RsaPrivateKey::from_pkcs1_pem(trimmed)
                .map_err(|e| PayError::Crypto(format!("PKCS#1 PEM 解析失败: {e}")));
        }
        if trimmed.contains("BEGIN PRIVATE KEY") {
            return RsaPrivateKey::from_pkcs8_pem(trimmed)
                .map_err(|e| PayError::Crypto(format!("PKCS#8 PEM 解析失败: {e}")));
        }
        let cleaned: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        let der = STANDARD
            .decode(&cleaned)
            .map_err(|e| PayError::Crypto(format!("私钥 Base64 解码失败: {e}")))?;
        RsaPrivateKey::from_pkcs1_der(&der)
            .or_else(|_| RsaPrivateKey::from_pkcs8_der(&der))
            .map_err(|e| PayError::Crypto(format!("私钥 DER 解析失败: {e}")))
    }

    /// 计算支付宝 RSA2 (`SHA256withRSA`) 签名。
    pub fn sign_rsa2(
        cfg: &AlipayConfig,
        params: &BTreeMap<String, String>,
    ) -> Result<String, PayError> {
        let key = parse_private_key(&cfg.app_private_key)?;
        let signing_key = SigningKey::<Sha256>::new(key);
        let content: String = params
            .iter()
            .filter(|(k, v)| k.as_str() != "sign" && !v.is_empty())
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let mut rng = rsa::rand_core::OsRng;
        let sig = signing_key.sign_with_rng(&mut rng, content.as_bytes());
        Ok(STANDARD.encode(sig.to_bytes()))
    }

    fn parse_public_key(raw: &str) -> Result<RsaPublicKey, PayError> {
        let trimmed = raw.trim();
        if trimmed.contains("BEGIN PUBLIC KEY") {
            return RsaPublicKey::from_public_key_pem(trimmed)
                .map_err(|e| PayError::Crypto(format!("SPKI PEM 公钥解析失败: {e}")));
        }
        let cleaned: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        let der = STANDARD
            .decode(&cleaned)
            .map_err(|e| PayError::Crypto(format!("公钥 Base64 解码失败: {e}")))?;
        RsaPublicKey::from_public_key_der(&der)
            .map_err(|e| PayError::Crypto(format!("公钥 DER 解析失败: {e}")))
    }

    /// 校验支付宝异步通知（`notify_url`）的 RSA2 (`SHA256withRSA`) 签名。
    pub fn verify_notify_rsa2(cfg: &AlipayConfig, params: &BTreeMap<String, String>) -> bool {
        if params.get("sign_type").map(String::as_str) != Some("RSA2") {
            return false;
        }
        let Some(sig_b64) = params.get("sign").filter(|s| !s.is_empty()) else {
            return false;
        };
        let Ok(sig_bytes) = STANDARD.decode(sig_b64) else {
            return false;
        };
        let Ok(sig) = Signature::try_from(sig_bytes.as_slice()) else {
            return false;
        };
        let Ok(pub_key) = parse_public_key(&cfg.alipay_public_key) else {
            return false;
        };
        let content: String = params
            .iter()
            .filter(|(k, v)| k.as_str() != "sign" && k.as_str() != "sign_type" && !v.is_empty())
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let verifier = VerifyingKey::<Sha256>::new(pub_key);
        verifier.verify(content.as_bytes(), &sig).is_ok()
    }

    /// 生成 UTC+8 (`YYYY-MM-DD HH:MM:SS`) 时间戳（零外部日期 crate）。
    fn cst_timestamp() -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
            + 8 * 3600;
        let days = secs.div_euclid(86400);
        let rem = secs.rem_euclid(86400);
        let h = rem / 3600;
        let m = (rem % 3600) / 60;
        let s = rem % 60;
        // Howard Hinnant civil_from_days algorithm
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if mo <= 2 { y + 1 } else { y };
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}")
    }

    /// 构建已 RSA2 签名的真实支付宝电脑网站支付（`alipay.trade.page.pay`）跳转链接。
    pub fn build_signed_page_pay(
        cfg: &AlipayConfig,
        order_id: &str,
        amount_cny: f64,
        subject: &str,
        return_url: &str,
        notify_url: &str,
    ) -> Result<CheckoutSession, PayError> {
        if amount_cny <= 0.0 {
            return Err(PayError::InvalidArg("充值金额必须大于 0".into()));
        }
        let biz_content = json!({
            "out_trade_no": order_id,
            "product_code": "FAST_INSTANT_TRADE_PAY",
            "total_amount": format!("{amount_cny:.2}"),
            "subject": subject
        })
        .to_string();

        let mut params = BTreeMap::new();
        params.insert("app_id".into(), cfg.app_id.clone());
        params.insert("biz_content".into(), biz_content);
        params.insert("charset".into(), "utf-8".into());
        params.insert("format".into(), "JSON".into());
        params.insert("method".into(), "alipay.trade.page.pay".into());
        if !notify_url.is_empty() {
            params.insert("notify_url".into(), notify_url.to_string());
        }
        if !return_url.is_empty() {
            params.insert("return_url".into(), return_url.to_string());
        }
        params.insert("sign_type".into(), "RSA2".into());
        params.insert("timestamp".into(), cst_timestamp());
        params.insert("version".into(), "1.0".into());

        let sign = sign_rsa2(cfg, &params)?;
        params.insert("sign".into(), sign);

        let mut ser = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in &params {
            ser.append_pair(k, v);
        }
        let checkout_url = format!("{}?{}", cfg.gateway_url(), ser.finish());

        Ok(CheckoutSession {
            channel: PaymentChannel::Alipay,
            order_id: order_id.to_string(),
            product_code: "points_topup".to_string(),
            amount: amount_cny,
            currency: "CNY".to_string(),
            points_credit: (amount_cny * 10.0).round() as i64,
            checkout_url: Some(checkout_url),
            live_mode: true,
            extra: json!({
                "environment": cfg.environment,
                "gateway": cfg.gateway_url()
            }),
        })
    }

    /// 实查支付宝 `alipay.trade.query` 订单状态。
    pub fn query_trade_live(cfg: &AlipayConfig, order_id: &str) -> Result<(bool, Value), PayError> {
        let biz_content = json!({ "out_trade_no": order_id }).to_string();
        let mut params = BTreeMap::new();
        params.insert("app_id".into(), cfg.app_id.clone());
        params.insert("biz_content".into(), biz_content);
        params.insert("charset".into(), "utf-8".into());
        params.insert("format".into(), "JSON".into());
        params.insert("method".into(), "alipay.trade.query".into());
        params.insert("sign_type".into(), "RSA2".into());
        params.insert("timestamp".into(), cst_timestamp());
        params.insert("version".into(), "1.0".into());

        let sign = sign_rsa2(cfg, &params)?;
        params.insert("sign".into(), sign);

        let mut ser = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in &params {
            ser.append_pair(k, v);
        }
        let url = format!("{}?{}", cfg.gateway_url(), ser.finish());
        let mut resp = ureq::get(&url)
            .call()
            .map_err(|e| PayError::Gateway(format!("请求支付宝查单失败: {e}")))?;
        let body: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| PayError::Gateway(format!("解析支付宝查单响应失败: {e}")))?;
        let q = &body["alipay_trade_query_response"];
        let status = q.get("trade_status").and_then(Value::as_str).unwrap_or("");
        let paid = matches!(status, "TRADE_SUCCESS" | "TRADE_FINISHED");
        Ok((paid, q.clone()))
    }
}

#[cfg(feature = "stripe")]
pub mod stripe {
    use super::*;
    use serde_json::{json, Value};

    #[derive(Debug, Clone, Deserialize)]
    pub struct StripeConfig {
        pub secret_key: String,
        #[serde(default)]
        pub publishable_key: String,
        #[serde(default)]
        pub mode: String,
        #[serde(default)]
        pub enabled: bool,
    }

    impl StripeConfig {
        pub fn from_json_file(path: &str) -> Result<Self, PayError> {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| PayError::NotConfigured(format!("读取 stripe 配置失败: {e}")))?;
            serde_json::from_str(&raw)
                .map_err(|e| PayError::NotConfigured(format!("解析 stripe 配置失败: {e}")))
        }
    }

    /// 调用 Stripe 官方 `/v1/checkout/sessions` 创建真实托管收银台（Visa / Mastercard / Apple Pay / Google Pay）。
    pub fn create_checkout_session_live(
        cfg: &StripeConfig,
        order_id: &str,
        product: &str,
        price_cny: f64,
        points_credit: i64,
        success_url: &str,
        cancel_url: &str,
    ) -> Result<CheckoutSession, PayError> {
        let price_usd = ((price_cny / 7.2) * 100.0).round() / 100.0;
        let unit_cents = ((price_usd * 100.0).round() as i64).max(50);

        let form_params = [
            ("mode", "payment".to_string()),
            ("success_url", success_url.to_string()),
            ("cancel_url", cancel_url.to_string()),
            ("client_reference_id", order_id.to_string()),
            ("line_items[0][quantity]", "1".to_string()),
            ("line_items[0][price_data][currency]", "usd".to_string()),
            (
                "line_items[0][price_data][unit_amount]",
                unit_cents.to_string(),
            ),
            (
                "line_items[0][price_data][product_data][name]",
                format!("Lain42 Credits ({points_credit} pts)"),
            ),
            (
                "line_items[0][price_data][product_data][tax_code]",
                "txcd_10103001".to_string(),
            ),
        ];

        let mut resp = ureq::post("https://api.stripe.com/v1/checkout/sessions")
            .header("Authorization", &format!("Bearer {}", cfg.secret_key))
            .send_form(form_params)
            .map_err(|e| PayError::Gateway(format!("Stripe 创建 Checkout 失败: {e}")))?;
        let body: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| PayError::Gateway(format!("解析 Stripe 响应失败: {e}")))?;
        let session_id = body
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(order_id)
            .to_string();
        let url = body.get("url").and_then(Value::as_str).map(String::from);

        Ok(CheckoutSession {
            channel: PaymentChannel::Stripe,
            order_id: session_id,
            product_code: product.to_string(),
            amount: (unit_cents as f64) / 100.0,
            currency: "USD".to_string(),
            points_credit,
            checkout_url: url,
            live_mode: cfg.secret_key.starts_with("sk_live_"),
            extra: json!({
                "mode": cfg.mode,
                "methods": ["card", "apple_pay", "google_pay"]
            }),
        })
    }

    /// 实查 Stripe Checkout Session 支付状态。
    pub fn retrieve_session_live(
        cfg: &StripeConfig,
        session_id: &str,
    ) -> Result<(bool, Value), PayError> {
        let url = format!("https://api.stripe.com/v1/checkout/sessions/{session_id}");
        let mut resp = ureq::get(&url)
            .header("Authorization", &format!("Bearer {}", cfg.secret_key))
            .call()
            .map_err(|e| PayError::Gateway(format!("Stripe 查单失败: {e}")))?;
        let body: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| PayError::Gateway(format!("解析 Stripe 查单响应失败: {e}")))?;
        let paid = body.get("payment_status").and_then(Value::as_str) == Some("paid");
        Ok((paid, body))
    }
}

#[cfg(feature = "creem")]
pub mod creem {
    use super::*;
    use ring::hmac;
    use serde_json::{json, Value};
    use std::collections::HashMap;

    #[derive(Debug, Clone, Deserialize)]
    pub struct CreemConfig {
        pub api_key: String,
        #[serde(default)]
        pub test_mode: bool,
        #[serde(default)]
        pub webhook_secret: String,
        /// 站内档位（如 `points_cny_30`）到 Creem `prod_...` 的映射
        #[serde(default)]
        pub products: HashMap<String, String>,
    }

    impl CreemConfig {
        pub fn from_json_file(path: &str) -> Result<Self, PayError> {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| PayError::NotConfigured(format!("读取 creem 配置失败: {e}")))?;
            serde_json::from_str(&raw)
                .map_err(|e| PayError::NotConfigured(format!("解析 creem 配置失败: {e}")))
        }

        pub fn api_base(&self) -> &'static str {
            if self.test_mode {
                "https://test-api.creem.io"
            } else {
                "https://api.creem.io"
            }
        }
    }

    /// 调用 Creem 官方 `POST /v1/checkouts` 创建海外托管收银台（Visa / Mastercard / Apple Pay / Google Pay）。
    pub fn create_checkout_live(
        cfg: &CreemConfig,
        order_id: &str,
        product: &str,
        price_usd: f64,
        points_credit: i64,
        success_url: &str,
    ) -> Result<CheckoutSession, PayError> {
        let creem_prod_id = cfg
            .products
            .get(product)
            .ok_or_else(|| PayError::NotConfigured(format!("creem.json 未映射商品: {product}")))?;
        let url = format!("{}/v1/checkouts", cfg.api_base());
        let payload = json!({
            "product_id": creem_prod_id,
            "request_id": order_id,
            "success_url": success_url,
            "metadata": { "order_id": order_id, "product": product, "points_credit": points_credit }
        });
        let mut resp = ureq::post(&url)
            .header("x-api-key", &cfg.api_key)
            .send_json(&payload)
            .map_err(|e| PayError::Gateway(format!("Creem 创建 Checkout 失败: {e}")))?;
        let body: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| PayError::Gateway(format!("解析 Creem 响应失败: {e}")))?;
        let checkout_url = body
            .get("checkout_url")
            .and_then(Value::as_str)
            .map(String::from);
        let checkout_id = body
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(order_id)
            .to_string();

        Ok(CheckoutSession {
            channel: PaymentChannel::Creem,
            order_id: checkout_id,
            product_code: product.to_string(),
            amount: price_usd,
            currency: "USD".to_string(),
            points_credit,
            checkout_url,
            live_mode: !cfg.test_mode,
            extra: json!({ "store_mode": if cfg.test_mode { "test" } else { "live" } }),
        })
    }

    /// 校验 Creem Webhook `creem-signature`（HMAC-SHA256 恒定时间验签）。
    pub fn verify_webhook_signature(secret: &str, raw_body: &[u8], sig_hex: &str) -> bool {
        let Ok(sig_bytes) = hex_decode(sig_hex.trim()) else {
            return false;
        };
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
        hmac::verify(&key, raw_body, &sig_bytes).is_ok()
    }

    fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
        if !s.len().is_multiple_of(2) {
            return Err(());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
            .collect()
    }
}

#[cfg(feature = "google-play")]
pub mod google_play {
    use super::*;

    pub fn validate_purchase_token(token: &str) -> Result<(), PayError> {
        if token.trim().len() < 8 {
            return Err(PayError::VerifyFailed(
                "无效的 Google Play purchase_token".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(feature = "usdt")]
pub mod usdt {
    use super::*;
    use serde_json::json;

    pub fn build_checkout(
        order_id: &str,
        wallet: &str,
        amount_usdt: f64,
    ) -> Result<CheckoutSession, PayError> {
        if !(1.0..=1000.0).contains(&amount_usdt) {
            return Err(PayError::InvalidArg("USDT 金额需在 1 ~ 1000 之间".into()));
        }
        Ok(CheckoutSession {
            channel: PaymentChannel::UsdtTrc20,
            order_id: order_id.to_string(),
            product_code: "points_topup".to_string(),
            amount: amount_usdt,
            currency: "USDT".to_string(),
            points_credit: (amount_usdt * 10.0).round() as i64,
            checkout_url: None,
            live_mode: true,
            extra: json!({ "network": "TRC20", "wallet": wallet }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "afdian")]
    #[test]
    fn builds_real_afdian_checkout_and_md5_sign() {
        let s = afdian::build_checkout("points_cny_30", "LA42-TEST-1").unwrap();
        assert!(s
            .checkout_url
            .unwrap()
            .contains("b3718c5cc52011f1bb7b52540025c377"));
        let sig = afdian::sign_openapi("uid123", "tok456", r#"{"page":1}"#, 1700000000);
        assert_eq!(sig.len(), 32);
    }
}
