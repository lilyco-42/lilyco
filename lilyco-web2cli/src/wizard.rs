//! 多步骤向导表单引擎（Multi-Step Wizard + 截图入审闸门）。
//!
//! 借鉴 `Skyvern-AI/skyvern` 与 `citrolabs/ego-lite`：
//! - 支持多步骤政务申报流程（Step 1 导航 → Step 2 填基础信息 → Step 3 点击下一步 → Step 4 填法人信息 → 人类审核闸门暂停并截图 → 批准后执行最终提交）
//! - 支持 `@vault:<category>.<key>` 与 `@attach:<key>` 占位符自动从分类资料库取值
//! - 遇到 `HumanReviewGate` 时强制暂停、截图落盘并等待人类批准后才继续执行后续提交步骤

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use lilyco::__core::error::AppError;
use serde::{Deserialize, Serialize};

use crate::cdp::CdpClient;
use crate::vault::VaultProfile;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum WizardStep {
    Navigate {
        url: String,
        #[serde(default = "default_settle")]
        settle_ms: u64,
    },
    Snapshot,
    Fill {
        ref_id: String,
        /// 支持字面量或 `@vault:<category>.<key>` / `@attach:<key>`
        value: String,
    },
    Click {
        ref_id: String,
    },
    Wait {
        ms: u64,
    },
    Screenshot {
        out: String,
    },
    /// 人类审核闸门：默认在此暂停并截取页面 PNG 供人类核对，批准后才允许继续执行后续步骤
    HumanReviewGate {
        message: String,
        #[serde(default)]
        screenshot_out: Option<String>,
    },
}

fn default_settle() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WizardPlan {
    pub name: String,
    #[serde(default)]
    pub about: String,
    pub steps: Vec<WizardStep>,
}

impl WizardPlan {
    pub fn from_str_auto(content: &str) -> Result<Self, AppError> {
        let trimmed = content.trim_start();
        if trimmed.starts_with('{') {
            serde_json::from_str(trimmed)
                .map_err(|e| AppError::InvalidInput(format!("解析 WizardPlan JSON 失败: {e}")))
        } else {
            toml::from_str(trimmed)
                .map_err(|e| AppError::InvalidInput(format!("解析 WizardPlan TOML 失败: {e}")))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WizardStatus {
    Completed,
    PausedForHumanReview,
    PausedForHumanVerification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WizardRunReport {
    pub plan_name: String,
    pub status: WizardStatus,
    pub executed_steps: usize,
    pub next_step_index: usize,
    #[serde(default)]
    pub gate_message: Option<String>,
    #[serde(default)]
    pub screenshot_path: Option<String>,
    pub logs: Vec<String>,
}

/// 在真实 CDP 浏览器会话中执行多步骤向导计划。
///
/// - 当 `allow_past_gate == false` 时，遇到 `HumanReviewGate` 会自动截图并暂停返回 `PausedForHumanReview`
/// - 人类核对截图与页面后，传入 `start_step = report.next_step_index` 且 `allow_past_gate = true` 继续执行最终提交
pub fn run_wizard_cdp(
    client: &mut CdpClient,
    plan: &WizardPlan,
    profile: &VaultProfile,
    start_step: usize,
    allow_past_gate: bool,
) -> Result<WizardRunReport, AppError> {
    let mut logs = Vec::new();
    let mut executed = 0usize;
    let mut last_screenshot: Option<String> = None;

    for (idx, step) in plan.steps.iter().enumerate().skip(start_step) {
        // 在任何填表/点击操作前，被动检测是否出现验证码/人机校验/UKey 提示；
        // 绝不尝试绕过，而是立即暂停并提示操作员在可见浏览器窗口中亲手完成验证。
        if matches!(step, WizardStep::Fill { .. } | WizardStep::Click { .. }) {
            if let Ok(Some(reason)) = client.detect_verification_challenge() {
                logs.push(format!(
                    "[step {idx}] PAUSED for human verification: {reason}"
                ));
                return Ok(WizardRunReport {
                    plan_name: plan.name.clone(),
                    status: WizardStatus::PausedForHumanVerification,
                    executed_steps: executed,
                    next_step_index: idx,
                    gate_message: Some(format!(
                        "{reason}。请操作员在可见浏览器窗口中完成验证后，从 step {idx} 继续执行。"
                    )),
                    screenshot_path: last_screenshot,
                    logs,
                });
            }
        }

        match step {
            WizardStep::Navigate { url, settle_ms } => {
                client.navigate(url, *settle_ms)?;
                logs.push(format!("[step {idx}] navigate -> {url}"));
            }
            WizardStep::Snapshot => {
                let snap = client.snapshot_all_frames()?;
                logs.push(format!(
                    "[step {idx}] snapshot: {} elements across {} frame(s)",
                    snap.elements.len(),
                    snap.frame_count
                ));
            }
            WizardStep::Fill { ref_id, value } => {
                let resolved = resolve_vault_placeholder(value, profile)?;
                let ok = client.fill_ref(ref_id, &resolved)?;
                if !ok {
                    return Err(AppError::Runtime(format!(
                        "[step {idx}] 未找到元素 `{ref_id}`，请先执行 snapshot"
                    )));
                }
                logs.push(format!("[step {idx}] fill {ref_id} <- {resolved}"));
            }
            WizardStep::Click { ref_id } => {
                let ok = client.click_ref(ref_id)?;
                if !ok {
                    return Err(AppError::Runtime(format!(
                        "[step {idx}] 未找到可点击元素 `{ref_id}`"
                    )));
                }
                logs.push(format!("[step {idx}] click {ref_id}"));
            }
            WizardStep::Wait { ms } => {
                thread::sleep(Duration::from_millis(*ms));
                logs.push(format!("[step {idx}] wait {ms}ms"));
            }
            WizardStep::Screenshot { out } => {
                let path = PathBuf::from(out);
                let bytes = client.capture_screenshot(&path)?;
                last_screenshot = Some(path.display().to_string());
                logs.push(format!("[step {idx}] screenshot -> {out} ({bytes} bytes)"));
            }
            WizardStep::HumanReviewGate {
                message,
                screenshot_out,
            } => {
                if let Some(out) = screenshot_out {
                    let path = PathBuf::from(out);
                    let bytes = client.capture_screenshot(&path)?;
                    last_screenshot = Some(path.display().to_string());
                    logs.push(format!(
                        "[step {idx}] gate screenshot -> {out} ({bytes} bytes)"
                    ));
                }
                if !allow_past_gate {
                    logs.push(format!("[step {idx}] PAUSED for human review: {message}"));
                    return Ok(WizardRunReport {
                        plan_name: plan.name.clone(),
                        status: WizardStatus::PausedForHumanReview,
                        executed_steps: executed,
                        next_step_index: idx + 1,
                        gate_message: Some(message.clone()),
                        screenshot_path: last_screenshot,
                        logs,
                    });
                }
                logs.push(format!("[step {idx}] gate approved, continuing"));
            }
        }
        executed += 1;
    }

    Ok(WizardRunReport {
        plan_name: plan.name.clone(),
        status: WizardStatus::Completed,
        executed_steps: executed,
        next_step_index: plan.steps.len(),
        gate_message: None,
        screenshot_path: last_screenshot,
        logs,
    })
}

pub fn resolve_vault_placeholder(raw: &str, profile: &VaultProfile) -> Result<String, AppError> {
    if let Some(rest) = raw.strip_prefix("@vault:") {
        let Some((cat, key)) = rest.split_once('.') else {
            return Err(AppError::InvalidArg(format!(
                "无效 @vault 占位符 `{raw}`，应为 @vault:<category>.<key>"
            )));
        };
        return profile
            .categories
            .get(cat)
            .and_then(|m| m.get(key))
            .cloned()
            .ok_or_else(|| {
                AppError::InvalidInput(format!("资料库 `{}` 缺少 `{cat}.{key}`", profile.name))
            });
    }
    if let Some(key) = raw.strip_prefix("@attach:") {
        return profile
            .attachments
            .get(key)
            .map(|a| a.path.clone())
            .ok_or_else(|| {
                AppError::InvalidInput(format!("资料库 `{}` 缺少附件 `{key}`", profile.name))
            });
    }
    Ok(raw.to_string())
}
