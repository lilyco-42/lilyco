//! 插件注册表与企业付费授权适配层（底层签发与恒定时间验签统一复用 `lilyco-license` crate）。

use lilyco::__core::error::AppError;
pub use lilyco_license::{current_unix_secs, EnterpriseLicense, LicenseError, PluginDescriptor};

pub const PLUGIN_VISION: &str = "vision";
pub const PLUGIN_PROOFREAD: &str = "proofread";
pub const PLUGIN_FORM_REVIEW: &str = "form-review";
pub const PLUGIN_CDP_WIZARD: &str = "cdp-wizard";

/// 返回 `lilyco-web2cli` 内置的全部商业插件目录。
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
            description:
                "GB 32100-2015 统一社会信用代码、GB 11643-1999 身份证号、手机号与附件体积核验"
                    .to_string(),
            price_fen_monthly: 4900,
        },
        PluginDescriptor {
            id: PLUGIN_FORM_REVIEW.to_string(),
            name: "智能预填 + 人类审核收件箱 + 模板沉淀".to_string(),
            description: "分类资料库、多格式 Cookie 导入、表单预填草稿、人类审批提交与成功模板复放"
                .to_string(),
            price_fen_monthly: 19900,
        },
        PluginDescriptor {
            id: PLUGIN_CDP_WIZARD.to_string(),
            name: "多步骤浏览器向导与人工验证接管".to_string(),
            description:
                "附着已登录 Chrome、多层 iframe 穿透填表、截图入审、遇验证码/UKey 自动暂停交由人工完成"
                    .to_string(),
            price_fen_monthly: 29900,
        },
    ]
}

fn map_err(e: LicenseError) -> AppError {
    match e {
        LicenseError::InvalidArg(m) => AppError::InvalidArg(m),
        LicenseError::Unauthorized(m) => AppError::InvalidInput(m),
        LicenseError::Codec(m) => AppError::Runtime(m),
    }
}

pub fn issue_license(secret: &str, license: &EnterpriseLicense) -> Result<String, AppError> {
    lilyco_license::issue_license(secret, license).map_err(map_err)
}

pub fn verify_license(secret: &str, token: &str) -> Result<EnterpriseLicense, AppError> {
    lilyco_license::verify_license(secret, token).map_err(map_err)
}

pub fn verify_license_at(
    secret: &str,
    token: &str,
    now_unix: u64,
) -> Result<EnterpriseLicense, AppError> {
    lilyco_license::verify_license_at(secret, token, now_unix).map_err(map_err)
}

pub fn check_plugin_entitlement(
    secret: &str,
    token: Option<&str>,
    plugin_id: &str,
) -> Result<EnterpriseLicense, AppError> {
    lilyco_license::check_plugin_entitlement(secret, token, plugin_id).map_err(map_err)
}
