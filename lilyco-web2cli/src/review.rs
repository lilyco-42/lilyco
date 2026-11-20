//! 人类审核闸门（Human-in-the-Loop Draft → Review → Approve → Submit）。
//!
//! 专用于政务填表、企业设立登记等高风险申报场景：
//! 1. **AI 预填草稿（`prepare_draft`）**：从网页发现表单 + 资料库自动语义映射 → 产出 `PendingReview` 草稿（不发包）
//! 2. **人类核对与修正（`patch_arg` / `approve` / `reject`）**：逐项核对字段值与上传附件；任何修改都会重置为 `PendingReview`
//! 3. **受控最终提交（`commit_draft`）**：强制校验 `status == Approved` 且无缺失必填项，发包后落盘回执

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use lilyco::__core::context::Context;
use lilyco::__core::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::exec::execute_web_command;
use crate::session::SessionManager;
use crate::spec::WebCommandSpec;
use crate::vault::{auto_map_form, VaultProfile};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DraftStatus {
    PendingReview,
    Approved,
    Rejected,
    Submitted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionDraft {
    pub id: String,
    pub created_unix: u64,
    pub status: DraftStatus,
    pub profile: String,
    pub session: String,
    pub base_url: String,
    pub command: WebCommandSpec,
    pub args: Map<String, Value>,
    pub mapping_notes: Vec<String>,
    pub missing_required: Vec<String>,
    #[serde(default)]
    pub reviewer_note: Option<String>,
    #[serde(default)]
    pub screenshot_path: Option<String>,
    #[serde(default)]
    pub recipe_id: Option<String>,
    #[serde(default)]
    pub receipt: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct DraftStore {
    root_dir: PathBuf,
}

impl DraftStore {
    pub fn new() -> Self {
        Self {
            root_dir: default_draft_dir(),
        }
    }

    pub fn with_dir(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn load(&self, id: &str) -> Result<SubmissionDraft, AppError> {
        let path = self.draft_path(id);
        let raw = fs::read_to_string(&path)
            .map_err(|e| AppError::InvalidInput(format!("读取草稿 `{id}` 失败: {e}")))?;
        serde_json::from_str(&raw)
            .map_err(|e| AppError::InvalidInput(format!("解析草稿 `{id}` JSON 失败: {e}")))
    }

    pub fn save(&self, draft: &SubmissionDraft) -> Result<PathBuf, AppError> {
        fs::create_dir_all(&self.root_dir)
            .map_err(|e| AppError::Runtime(format!("创建草稿目录失败: {e}")))?;
        let path = self.draft_path(&draft.id);
        let json = serde_json::to_string_pretty(draft)
            .map_err(|e| AppError::Runtime(format!("序列化草稿失败: {e}")))?;
        fs::write(&path, json)
            .map_err(|e| AppError::Runtime(format!("写入草稿 `{}` 失败: {e}", draft.id)))?;
        Ok(path)
    }

    pub fn list(&self) -> Vec<SubmissionDraft> {
        let Ok(rd) = fs::read_dir(&self.root_dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(raw) = fs::read_to_string(&p) {
                    if let Ok(d) = serde_json::from_str::<SubmissionDraft>(&raw) {
                        out.push(d);
                    }
                }
            }
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.created_unix));
        out
    }

    /// AI 自动从表单 + 资料库生成待人类审核的填报草稿（绝不直接提交）。
    pub fn prepare_draft(
        &self,
        profile: &VaultProfile,
        session_name: &str,
        base_url: &str,
        command: WebCommandSpec,
        overrides: &Map<String, Value>,
    ) -> Result<SubmissionDraft, AppError> {
        let mapped = auto_map_form(&command, profile, overrides);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let id = format!("{}-{now}", command.name);
        let draft = SubmissionDraft {
            id,
            created_unix: now,
            status: DraftStatus::PendingReview,
            profile: profile.name.clone(),
            session: session_name.to_string(),
            base_url: base_url.to_string(),
            command,
            args: mapped.args,
            mapping_notes: mapped.mapping_notes,
            missing_required: mapped.missing_required,
            reviewer_note: None,
            screenshot_path: None,
            recipe_id: None,
            receipt: None,
        };
        self.save(&draft)?;
        Ok(draft)
    }

    /// 优先应用已沉淀的 `SiteRecipe`（确定性复放），再补充语义映射与人工覆盖，生成待审核草稿。
    pub fn prepare_draft_with_recipe(
        &self,
        profile: &VaultProfile,
        session_name: &str,
        base_url: &str,
        command: WebCommandSpec,
        recipe: Option<&crate::recipe::SiteRecipe>,
        manual_overrides: &Map<String, Value>,
    ) -> Result<SubmissionDraft, AppError> {
        let mut combined_overrides = Map::new();
        let mut recipe_notes = Vec::new();
        let mut used_recipe_id = None;

        if let Some(rec) = recipe {
            let (rec_map, notes) = crate::recipe::RecipeStore::apply_recipe(rec, profile);
            combined_overrides.extend(rec_map);
            recipe_notes = notes;
            used_recipe_id = Some(rec.id.clone());
        }
        for (k, v) in manual_overrides {
            combined_overrides.insert(k.clone(), v.clone());
        }

        let mut draft = self.prepare_draft(
            profile,
            session_name,
            base_url,
            command,
            &combined_overrides,
        )?;
        if !recipe_notes.is_empty() {
            recipe_notes.extend(draft.mapping_notes);
            draft.mapping_notes = recipe_notes;
        }
        draft.recipe_id = used_recipe_id;
        self.save(&draft)?;
        Ok(draft)
    }

    /// 为草稿附加真实网页截图路径（供人类审核核对）。
    pub fn attach_screenshot(
        &self,
        id: &str,
        screenshot_path: impl Into<String>,
    ) -> Result<SubmissionDraft, AppError> {
        let mut draft = self.load(id)?;
        draft.screenshot_path = Some(screenshot_path.into());
        self.save(&draft)?;
        Ok(draft)
    }

    /// 审核阶段修正或补齐字段值；修改后自动重置为 `PendingReview`，防止审批后篡改。
    pub fn patch_arg(
        &self,
        id: &str,
        field: &str,
        value: Value,
    ) -> Result<SubmissionDraft, AppError> {
        let mut draft = self.load(id)?;
        if draft.status == DraftStatus::Submitted {
            return Err(AppError::InvalidInput("已提交的草稿不可修改".into()));
        }
        draft.args.insert(field.to_string(), value);
        draft
            .mapping_notes
            .push(format!("{field} <- [人工审核修正]"));
        draft
            .missing_required
            .retain(|m| !m.starts_with(&format!("{field} ")));
        draft.status = DraftStatus::PendingReview;
        self.save(&draft)?;
        Ok(draft)
    }

    /// 人类确认核对无误后批准草稿。
    pub fn approve(&self, id: &str, note: Option<String>) -> Result<SubmissionDraft, AppError> {
        let mut draft = self.load(id)?;
        if draft.status == DraftStatus::Submitted {
            return Err(AppError::InvalidInput("草稿已提交，无需重复批准".into()));
        }
        if !draft.missing_required.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "仍有必填项未填写，无法批准: {}",
                draft.missing_required.join(", ")
            )));
        }
        draft
            .command
            .to_command_schema()
            .validate_args(&Value::Object(draft.args.clone()))?;
        draft.status = DraftStatus::Approved;
        draft.reviewer_note = note;
        self.save(&draft)?;
        Ok(draft)
    }

    /// 人类驳回草稿。
    pub fn reject(&self, id: &str, note: Option<String>) -> Result<SubmissionDraft, AppError> {
        let mut draft = self.load(id)?;
        if draft.status == DraftStatus::Submitted {
            return Err(AppError::InvalidInput("草稿已提交，无法驳回".into()));
        }
        draft.status = DraftStatus::Rejected;
        draft.reviewer_note = note;
        self.save(&draft)?;
        Ok(draft)
    }

    /// 执行最终提交：强制校验 `DraftStatus::Approved`，提交成功后将回执归档。
    pub fn commit_draft(
        &self,
        id: &str,
        ctx: &Context,
        session: &SessionManager,
    ) -> Result<SubmissionDraft, AppError> {
        let mut draft = self.load(id)?;
        if draft.status != DraftStatus::Approved {
            return Err(AppError::InvalidInput(format!(
                "安全拦截：草稿 `{id}` 当前状态为 {:?}，必须先经人类审核并 `approve` 后才允许提交",
                draft.status
            )));
        }
        let args_val = Value::Object(draft.args.clone());
        draft.command.to_command_schema().validate_args(&args_val)?;
        let receipt =
            execute_web_command(ctx, session, &draft.base_url, &draft.command, &args_val)?;
        draft.status = DraftStatus::Submitted;
        draft.receipt = Some(receipt);
        self.save(&draft)?;
        Ok(draft)
    }

    fn draft_path(&self, id: &str) -> PathBuf {
        let safe: String = id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.root_dir.join(format!("{safe}.json"))
    }
}

impl Default for DraftStore {
    fn default() -> Self {
        Self::new()
    }
}

fn default_draft_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg)
                .join("lilyco")
                .join("web2cli")
                .join("drafts");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".config")
                .join("lilyco")
                .join("web2cli")
                .join("drafts");
        }
    }
    std::env::temp_dir().join("lilyco-web2cli-drafts")
}
