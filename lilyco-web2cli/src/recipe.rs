//! 成功填报模板沉淀引擎（Recipe / Learnings）。
//!
//! 借鉴 `browserbase/stagehand`（确定性动作缓存）与 `citrolabs/ego-lite/learnings`（经验沉淀）：
//! - 当人类在 `review` 中核对/修正并批准或提交某政务表单后，自动沉淀出 `SiteRecipe`
//! - 记录每个表单字段与资料库（`category.key` / `attachments.key`）或固定值的确定性绑定
//! - 下次为第二家企业填报同一网站时，优先 100% 确定性复放模板，零猜测、零重复试错

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use lilyco::__core::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::review::SubmissionDraft;
use crate::spec::sanitize_ident;
use crate::vault::VaultProfile;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum FieldBinding {
    VaultField { category: String, key: String },
    VaultAttachment { key: String },
    StaticValue { value: Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteRecipe {
    pub id: String,
    pub host: String,
    pub command_name: String,
    pub target_url: String,
    pub bindings: BTreeMap<String, FieldBinding>,
    pub success_count: u32,
    pub last_used_unix: u64,
}

#[derive(Debug, Clone)]
pub struct RecipeStore {
    root_dir: PathBuf,
}

impl RecipeStore {
    pub fn new() -> Self {
        Self {
            root_dir: default_recipe_dir(),
        }
    }

    pub fn with_dir(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn list(&self) -> Vec<SiteRecipe> {
        let Ok(rd) = fs::read_dir(&self.root_dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(raw) = fs::read_to_string(&p) {
                    if let Ok(r) = serde_json::from_str::<SiteRecipe>(&raw) {
                        out.push(r);
                    }
                }
            }
        }
        out.sort_by_key(|r| std::cmp::Reverse(r.last_used_unix));
        out
    }

    pub fn load(&self, id: &str) -> Result<SiteRecipe, AppError> {
        let path = self.recipe_path(id);
        let raw = fs::read_to_string(&path)
            .map_err(|e| AppError::InvalidInput(format!("读取模板 `{id}` 失败: {e}")))?;
        serde_json::from_str(&raw)
            .map_err(|e| AppError::InvalidInput(format!("解析模板 `{id}` JSON 失败: {e}")))
    }

    pub fn save(&self, recipe: &SiteRecipe) -> Result<PathBuf, AppError> {
        fs::create_dir_all(&self.root_dir)
            .map_err(|e| AppError::Runtime(format!("创建模板目录失败: {e}")))?;
        let path = self.recipe_path(&recipe.id);
        let json = serde_json::to_string_pretty(recipe)
            .map_err(|e| AppError::Runtime(format!("序列化模板失败: {e}")))?;
        fs::write(&path, json)
            .map_err(|e| AppError::Runtime(format!("写入模板 `{}` 失败: {e}", recipe.id)))?;
        Ok(path)
    }

    /// 查找匹配指定网站 URL + 表单命令名的已沉淀模板。
    pub fn find_matching(&self, url: &str, command_name: &str) -> Option<SiteRecipe> {
        let id = recipe_id_for(url, command_name);
        if let Ok(r) = self.load(&id) {
            return Some(r);
        }
        let host = extract_host(url);
        self.list()
            .into_iter()
            .find(|r| r.host == host && r.command_name == command_name)
    }

    /// 从经人类审核/提交成功的草稿中自动沉淀可复用模板。
    pub fn learn_from_draft(
        &self,
        draft: &SubmissionDraft,
        profile: &VaultProfile,
    ) -> Result<SiteRecipe, AppError> {
        let id = recipe_id_for(&draft.command.url, &draft.command.name);
        let host = extract_host(&draft.command.url);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut bindings = BTreeMap::new();
        for (field, val) in &draft.args {
            if let Some(binding) = infer_binding(val, profile) {
                bindings.insert(field.clone(), binding);
            } else {
                bindings.insert(
                    field.clone(),
                    FieldBinding::StaticValue { value: val.clone() },
                );
            }
        }

        let prev_count = self.load(&id).map(|r| r.success_count).unwrap_or(0);
        let recipe = SiteRecipe {
            id,
            host,
            command_name: draft.command.name.clone(),
            target_url: draft.command.url.clone(),
            bindings,
            success_count: prev_count + 1,
            last_used_unix: now,
        };
        self.save(&recipe)?;
        Ok(recipe)
    }

    /// 将已沉淀的模板应用到新的企业资料库，返回确定性参数映射与审计说明。
    pub fn apply_recipe(
        recipe: &SiteRecipe,
        profile: &VaultProfile,
    ) -> (Map<String, Value>, Vec<String>) {
        let mut overrides = Map::new();
        let mut notes = Vec::new();
        for (field, binding) in &recipe.bindings {
            match binding {
                FieldBinding::VaultField { category, key } => {
                    if let Some(v) = profile.categories.get(category).and_then(|m| m.get(key)) {
                        overrides.insert(field.clone(), Value::String(v.clone()));
                        notes.push(format!("{field} <- [模板 {}] {category}.{key}", recipe.id));
                    }
                }
                FieldBinding::VaultAttachment { key } => {
                    if let Some(att) = profile.attachments.get(key) {
                        overrides.insert(field.clone(), Value::String(att.path.clone()));
                        notes.push(format!(
                            "{field} <- [模板 {}] attachments.{key} ({})",
                            recipe.id, att.path
                        ));
                    }
                }
                FieldBinding::StaticValue { value } => {
                    overrides.insert(field.clone(), value.clone());
                    notes.push(format!("{field} <- [模板 {}] 固定默认值", recipe.id));
                }
            }
        }
        (overrides, notes)
    }

    fn recipe_path(&self, id: &str) -> PathBuf {
        self.root_dir.join(format!("{id}.json"))
    }
}

impl Default for RecipeStore {
    fn default() -> Self {
        Self::new()
    }
}

fn infer_binding(val: &Value, profile: &VaultProfile) -> Option<FieldBinding> {
    let s = val.as_str()?;
    for (att_key, att) in &profile.attachments {
        if att.path == s {
            return Some(FieldBinding::VaultAttachment {
                key: att_key.clone(),
            });
        }
    }
    for (cat, fields) in &profile.categories {
        for (k, v) in fields {
            if v == s {
                return Some(FieldBinding::VaultField {
                    category: cat.clone(),
                    key: k.clone(),
                });
            }
        }
    }
    None
}

pub fn recipe_id_for(url: &str, command_name: &str) -> String {
    let host = extract_host(url);
    format!(
        "{}--{}",
        sanitize_ident(&host, "site"),
        sanitize_ident(command_name, "form")
    )
}

fn extract_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
        .unwrap_or_else(|| "site".to_string())
}

fn default_recipe_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg)
                .join("lilyco")
                .join("web2cli")
                .join("recipes");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".config")
                .join("lilyco")
                .join("web2cli")
                .join("recipes");
        }
    }
    std::env::temp_dir().join("lilyco-web2cli-recipes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::discover_from_html;
    use crate::review::DraftStore;
    use crate::vault::VaultStore;

    #[test]
    fn learns_recipe_from_company_a_and_replays_for_company_b() {
        let dir = tempfile::tempdir().unwrap();
        let vaults = VaultStore::with_dir(dir.path().join("vault"));
        let drafts = DraftStore::with_dir(dir.path().join("drafts"));
        let recipes = RecipeStore::with_dir(dir.path().join("recipes"));

        vaults
            .set_field("comp_a", "company", "name", "甲公司")
            .unwrap();
        vaults
            .set_field("comp_a", "custom", "special_code", "SC-A")
            .unwrap();
        vaults
            .set_field("comp_b", "company", "name", "乙公司")
            .unwrap();
        vaults
            .set_field("comp_b", "custom", "special_code", "SC-B")
            .unwrap();

        let html = r#"
        <form id="gov_apply" action="/apply" method="post">
          <input type="text" name="entName" required>
          <input type="text" name="x_weird_field" required>
          <input type="text" name="region" required>
        </form>
        "#;
        let spec = discover_from_html("https://zwfw.gov.example/", html);
        let cmd = spec
            .commands
            .iter()
            .find(|c| c.name == "gov-apply")
            .unwrap()
            .clone();

        // Company A: human patches x_weird_field -> SC-A and region -> 上海
        let prof_a = vaults.load("comp_a").unwrap();
        let d_a = drafts
            .prepare_draft(&prof_a, "gov", &spec.base_url, cmd.clone(), &Map::new())
            .unwrap();
        drafts
            .patch_arg(&d_a.id, "x-weird-field", Value::String("SC-A".into()))
            .unwrap();
        drafts
            .patch_arg(&d_a.id, "region", Value::String("上海".into()))
            .unwrap();
        let approved_a = drafts.approve(&d_a.id, Some("ok".into())).unwrap();

        // Learn recipe!
        let recipe = recipes.learn_from_draft(&approved_a, &prof_a).unwrap();
        assert_eq!(recipe.success_count, 1);
        assert_eq!(
            recipe.bindings.get("x-weird-field"),
            Some(&FieldBinding::VaultField {
                category: "custom".into(),
                key: "special_code".into(),
            })
        );

        // Company B: replay recipe deterministically!
        let prof_b = vaults.load("comp_b").unwrap();
        let (overrides_b, notes_b) = RecipeStore::apply_recipe(&recipe, &prof_b);
        assert_eq!(overrides_b["entname"], "乙公司");
        assert_eq!(overrides_b["x-weird-field"], "SC-B");
        assert_eq!(overrides_b["region"], "上海");
        assert_eq!(notes_b.len(), 3);
    }
}
