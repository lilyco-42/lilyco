//! 插件注册表与企业付费授权验证引擎（HMAC-SHA256 签名 License Key）。
//!
//! 职责：
//! 1. 定义可独立售卖/按需启用的商业能力插件（`vision` / `proofread` / `form-review` / `cdp-wizard`）
//! 2. 使用 `ring::hmac`（HMAC-SHA256 恒定时间验签）签发与校验企业付费令牌 `L42.<payload_b64>.<sig_b64>`
//! 3. 统一门控校验：签名有效性、有效期、插件授权范围（支持 `*` 全量企业版）

use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use lilyco::__core::error::AppError;
use ring::hmac;
use serde::{Deserialize, Serialize};

pub const PLUGIN_VISION: &str = "vision";
pub const PLUGIN_PROOFREAD: &str = "proofread";
pub const PLUGIN_FORM_REVIEW: &str = "form-review";
pub const PLUGIN_CDP_WIZARD: &str = "cdp-wizard";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub price_fen_monthly: u32,
}

/// 返回系统内置的全部商业插件目录。
pub fn builtin_plugins() -> Vec<PluginDescriptor> {
    vec![
        PluginDescriptor {
            id: PLUGIN_VISION.to_string(),
            name: "图片与证件照合规裁剪".to_string(),
            description: "一寸/二寸证件照裁剪、营业执照与扫描件缩放压缩、签名下载链接".to_string(),
            price_fen_monthly: 2900,
        },
        PluginDescriptor {
            id: PLUGIN_PROOFREAD.to_string(),
            name: "政务资料国标校对".to_string(),
            description: "GB 32100-2015 统一社会信用代码、GB 11643-1999 身份证号、手机号与附件体积核验".to_string(),
            price_fen_monthly: 4900,
        },
        PluginDescriptor {
            id: PLUGIN_FORM_REVIEW.to_string(),
            name: "智能预填 + 人类审核收件箱 + 模板沉淀".to_string(),
            description: "分类资料库、多格式 Cookie 导入、表单预填草稿、人类审批提交与成功模板复放".to_string(),
            price_fen_monthly: 19900,
        },
        PluginDescriptor {
            id: PLUGIN_CDP_WIZARD.to_string(),
            name: "多步骤浏览器向导与人工验证接管".to_string(),
            description: "附着已登录 Chrome、多层 iframe 穿透填表、截图入审、遇验证码/UKey 自动暂停交由人工完成".to_string(),
            price_fen_monthly: 29900,
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnterpriseLicense {
    pub tenant_id: String,
    pub company_name: String,
    /// 允许使用的插件 ID 列表（如 `["vision", "proofread"]`，或 `["*"]` 表示全量企业版）
    pub enabled_plugins: Vec<String>,
    pub issued_unix: u64,
    pub expires_unix: u64,
}

impl EnterpriseLicense {
    pub fn has_plugin(&self, plugin_id: &str) -> bool {
        self.enabled_plugins
            .iter()
            .any(|p| p == "*" || p.eq_ignore_ascii_case(plugin_id))
    }

    pub fn is_expired_at(&self, now_unix: u64) -> bool {
        now_unix > self.expires_unix
    }
}

/// 签发企业付费授权令牌（格式：`L42.<base64url(json)>.<base64url(hmac_sha256)>`）。
pub fn issue_license(secret: &str, license: &EnterpriseLicense) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Err(AppError::InvalidArg("license secret 不能为空".into()));
    }
    let payload_bytes = serde_json::to_vec(license)
        .map_err(|e| AppError::Runtime(format!("序列化 License 失败: {e}")))?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(&payload_bytes);

    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let tag = hmac::sign(&key, payload_b64.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(tag.as_ref());

    Ok(format!("L42.{payload_b64}.{sig_b64}"))
}

/// 校验企业付费授权令牌（恒定时间验签 + 解析载荷 + 校验有效期）。
pub fn verify_license(secret: &str, token: &str) -> Result<EnterpriseLicense, AppError> {
    let now = current_unix_secs();
    verify_license_at(secret, token, now)
}

pub fn verify_license_at(
    secret: &str,
    token: &str,
    now_unix: u64,
) -> Result<EnterpriseLicense, AppError> {
    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() != 3 || parts[0] != "L42" {
        return Err(AppError::InvalidArg(
            "无效的 License 格式（应为 L42.<payload>.<sig>）".into(),
        ));
    }
    let payload_b64 = parts[1];
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| AppError::InvalidArg("License 签名编码无效".into()))?;

    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    hmac::verify(&key, payload_b64.as_bytes(), &sig_bytes).map_err(|_| {
        AppError::InvalidArg("License 签名校验失败（令牌被篡改或密钥不匹配）".into())
    })?;

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| AppError::InvalidArg("License 载荷编码无效".into()))?;
    let license: EnterpriseLicense = serde_json::from_slice(&payload_bytes)
        .map_err(|e| AppError::InvalidArg(format!("解析 License 载荷失败: {e}")))?;

    if license.is_expired_at(now_unix) {
        return Err(AppError::InvalidInput(format!(
            "企业授权已过期（tenant: {}，到期时间戳: {}）",
            license.tenant_id, license.expires_unix
        )));
    }
    Ok(license)
}

/// 校验令牌是否已付费解锁指定插件。
pub fn check_plugin_entitlement(
    secret: &str,
    token: Option<&str>,
    plugin_id: &str,
) -> Result<EnterpriseLicense, AppError> {
    let token = token
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "插件 `{plugin_id}` 需要企业付费授权（请提供有效 X-Lain-License 令牌）"
            ))
        })?;
    let lic = verify_license(secret, token)?;
    if !lic.has_plugin(plugin_id) {
        return Err(AppError::InvalidInput(format!(
            "企业 `{}` 的当前授权未包含插件 `{plugin_id}`，请升级套餐后使用",
            lic.company_name
        )));
    }
    Ok(lic)
}

pub fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issues_verifies_and_enforces_plugin_entitlements() {
        let secret = "test-enterprise-secret-key";
        let now = 1_800_000_000u64;
        let lic = EnterpriseLicense {
            tenant_id: "ent-001".into(),
            company_name: "星火科技有限公司".into(),
            enabled_plugins: vec![PLUGIN_VISION.into(), PLUGIN_PROOFREAD.into()],
            issued_unix: now,
            expires_unix: now + 86400 * 30,
        };

        let token = issue_license(secret, &lic).unwrap();
        assert!(token.starts_with("L42."));

        // Valid verification
        let verified = verify_license_at(secret, &token, now + 100).unwrap();
        assert_eq!(verified.tenant_id, "ent-001");
        assert!(verified.has_plugin(PLUGIN_VISION));
        assert!(verified.has_plugin(PLUGIN_PROOFREAD));
        assert!(!verified.has_plugin(PLUGIN_CDP_WIZARD));

        // Tampered token rejected
        let tampered = format!("{token}A");
        assert!(verify_license_at(secret, &tampered, now + 100).is_err());

        // Wrong secret rejected
        assert!(verify_license_at("wrong-secret", &token, now + 100).is_err());

        // Expired token rejected
        assert!(verify_license_at(secret, &token, now + 86400 * 31).is_err());
    }
}
