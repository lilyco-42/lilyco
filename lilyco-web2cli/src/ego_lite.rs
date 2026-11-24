use std::io::{Read, Write};
use std::process::{Command, Stdio};

use lilyco::prelude::{AppError, ValueEnum};
use serde::Deserialize;
use url::Url;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum EgoLiteMode {
    Off,
    Handoff,
    Resume,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum EgoLiteResult {
    WaitingForHuman { task_id: u64, reason: String },
    Page { url: String, html: String },
}

pub(crate) fn inspect(url: &str, mode: EgoLiteMode) -> Result<EgoLiteResult, AppError> {
    let mode = match mode {
        EgoLiteMode::Handoff => "handoff",
        EgoLiteMode::Resume => "resume",
        EgoLiteMode::Off => return Err(AppError::InvalidArg("ego-lite 模式未启用".into())),
    };
    let url = Url::parse(url)
        .map_err(|e| AppError::InvalidInput(format!("ego-lite 需要有效的网页 URL: {e}")))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::InvalidInput(
            "ego-lite 需要不带 URL 用户名或密码的 HTTP/HTTPS 地址".into(),
        ));
    }
    let mut result = tempfile::NamedTempFile::new()
        .map_err(|e| AppError::Runtime(format!("创建 ego-lite 临时响应文件失败: {e}")))?;
    let mut child = Command::new("ego-browser")
        .arg("nodejs")
        .env("LWEB2CLI_EGO_URL", url.as_str())
        .env("LWEB2CLI_EGO_OUT", result.path())
        .env("LWEB2CLI_EGO_MODE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            AppError::Runtime(format!("启动 ego-browser 失败，请确认 ego-lite 可用: {e}"))
        })?;

    let input_result = child
        .stdin
        .take()
        .ok_or_else(|| AppError::Runtime("无法连接 ego-browser 的标准输入".into()))?
        .write_all(include_bytes!("ego_lite_handoff.js"));
    let output = child
        .wait_with_output()
        .map_err(|e| AppError::Runtime(format!("等待 ego-browser 失败: {e}")))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Runtime(format!("ego-lite: {}", error.trim())));
    }
    input_result.map_err(|e| AppError::Runtime(format!("写入 ego-browser 指令失败: {e}")))?;

    let mut json = String::new();
    result
        .read_to_string(&mut json)
        .map_err(|e| AppError::Runtime(format!("读取 ego-lite 响应失败: {e}")))?;
    let response: EgoLiteResult = serde_json::from_str(&json)
        .map_err(|e| AppError::Runtime(format!("解析 ego-lite 响应失败: {e}")))?;
    if let EgoLiteResult::Page { url: final_url, .. } = &response {
        if Url::parse(final_url)
            .map(|final_url| final_url.origin())
            .ok()
            != Some(url.origin())
        {
            return Err(AppError::Runtime("ego-lite 页面已离开请求的站点".into()));
        }
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ineligible_targets_before_launching_ego() {
        for target in [
            "./spec.json",
            "file:///tmp/site.html",
            "https://x:y@example.com/",
        ] {
            assert!(matches!(
                inspect(target, EgoLiteMode::Handoff),
                Err(AppError::InvalidInput(_))
            ));
        }
    }
}
