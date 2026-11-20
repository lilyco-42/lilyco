//! 商业插件清单与企业付费授权引擎（HMAC-SHA256 签名 License Key）。
//!
//! 从 `lilyco-web2cli` 抽离的独立复用套件，供所有 Lilyco 商业应用（门户、工作台、CLI、MCP）共享：
//! - 统一插件描述符 [`PluginDescriptor`] 与权益声明 [`EnterpriseLicense`]
//! - 基于 [`ring::hmac`] 恒定时间验签，签发与校验 `L42.<payload_b64>.<sig_b64>` 离线授权令牌
//! - 支持按插件 ID 门控与 `*` 全量企业版通配符

use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::hmac;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// License 签发与校验错误。
#[derive(Debug, Error)]
pub enum LicenseError {
    #[error("无效参数: {0}")]
    InvalidArg(String),
    #[error("授权校验未通过: {0}")]
    Unauthorized(String),
    #[error("序列化失败: {0}")]
    Codec(String),
}

/// 商业插件元信息（用于门户展示、商城定价与权限门控）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub price_fen_monthly: u32,
}

/// 企业离线授权声明（序列化为 JSON 后经 HMAC-SHA256 签名）。
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
pub fn issue_license(secret: &str, license: &EnterpriseLicense) -> Result<String, LicenseError> {
    if secret.trim().is_empty() {
        return Err(LicenseError::InvalidArg("license secret 不能为空".into()));
    }
    let payload_bytes = serde_json::to_vec(license)
        .map_err(|e| LicenseError::Codec(format!("序列化 License 失败: {e}")))?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(&payload_bytes);

    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let tag = hmac::sign(&key, payload_b64.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(tag.as_ref());

    Ok(format!("L42.{payload_b64}.{sig_b64}"))
}

/// 校验企业付费授权令牌（恒定时间验签 + 解析载荷 + 校验有效期）。
pub fn verify_license(secret: &str, token: &str) -> Result<EnterpriseLicense, LicenseError> {
    verify_license_at(secret, token, current_unix_secs())
}

/// 在指定 Unix 时间戳下校验令牌（便于单元测试时间边界）。
pub fn verify_license_at(
    secret: &str,
    token: &str,
    now_unix: u64,
) -> Result<EnterpriseLicense, LicenseError> {
    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() != 3 || parts[0] != "L42" {
        return Err(LicenseError::InvalidArg(
            "无效的 License 格式（应为 L42.<payload>.<sig>）".into(),
        ));
    }
    let payload_b64 = parts[1];
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| LicenseError::InvalidArg("License 签名编码无效".into()))?;

    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    hmac::verify(&key, payload_b64.as_bytes(), &sig_bytes).map_err(|_| {
        LicenseError::InvalidArg("License 签名校验失败（令牌被篡改或密钥不匹配）".into())
    })?;

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| LicenseError::InvalidArg("License 载荷编码无效".into()))?;
    let license: EnterpriseLicense = serde_json::from_slice(&payload_bytes)
        .map_err(|e| LicenseError::InvalidArg(format!("解析 License 载荷失败: {e}")))?;

    if license.is_expired_at(now_unix) {
        return Err(LicenseError::Unauthorized(format!(
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
) -> Result<EnterpriseLicense, LicenseError> {
    let token = token
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            LicenseError::Unauthorized(format!(
                "插件 `{plugin_id}` 需要企业付费授权（请提供有效 X-Lain-License 令牌）"
            ))
        })?;
    let lic = verify_license(secret, token)?;
    if !lic.has_plugin(plugin_id) {
        return Err(LicenseError::Unauthorized(format!(
            "企业 `{}` 的当前授权未包含插件 `{plugin_id}`，请升级套餐后使用",
            lic.company_name
        )));
    }
    Ok(lic)
}

/// 当前系统 Unix 秒级时间戳。
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
            enabled_plugins: vec!["vision".into(), "proofread".into()],
            issued_unix: now,
            expires_unix: now + 86400 * 30,
        };

        let token = issue_license(secret, &lic).unwrap();
        assert!(token.starts_with("L42."));

        let verified = verify_license_at(secret, &token, now + 100).unwrap();
        assert_eq!(verified.tenant_id, "ent-001");
        assert!(verified.has_plugin("vision"));
        assert!(verified.has_plugin("proofread"));
        assert!(!verified.has_plugin("cdp-wizard"));

        let tampered = format!("{token}A");
        assert!(verify_license_at(secret, &tampered, now + 100).is_err());
        assert!(verify_license_at("wrong-secret", &token, now + 100).is_err());
        assert!(verify_license_at(secret, &token, now + 86400 * 31).is_err());
    }
}
