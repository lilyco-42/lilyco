//! 分类资料库（Vault）与政务/企业登记表单语义自动映射引擎。
//!
//! 职责：
//! 1. 按主体（Profile，如 `acme-corp`）和分类（`company` / `legal_rep` / `finance` / `contact` / 自定义）管理结构化资料
//! 2. 管理证照与申报材料附件（身份证、营业执照、公司章程、租赁合同等，带分类与文件路径校验）
//! 3. 根据表单字段的 `name` / `<label>` / `placeholder` / `type` 自动将资料库内容映射到表单参数，生成可审核的映射说明

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use lilyco::__core::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::spec::{WebArgKind, WebCommandSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentEntry {
    pub category: String,
    pub label: String,
    pub path: String,
    #[serde(default)]
    pub mime_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VaultProfile {
    pub name: String,
    /// category -> (field_key -> field_value)
    /// 推荐分类：company / legal_rep / finance / contact / custom
    #[serde(default)]
    pub categories: BTreeMap<String, BTreeMap<String, String>>,
    /// attachment_key -> AttachmentEntry
    #[serde(default)]
    pub attachments: BTreeMap<String, AttachmentEntry>,
}

#[derive(Debug, Clone)]
pub struct VaultStore {
    root_dir: PathBuf,
}

impl VaultStore {
    pub fn new() -> Self {
        Self {
            root_dir: default_vault_dir(),
        }
    }

    pub fn with_dir(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn load(&self, profile_name: &str) -> Result<VaultProfile, AppError> {
        let name = sanitize_name(profile_name);
        let path = self.profile_path(&name);
        if !path.exists() {
            return Ok(VaultProfile {
                name,
                ..Default::default()
            });
        }
        let raw = fs::read_to_string(&path)
            .map_err(|e| AppError::Runtime(format!("读取资料库 `{name}` 失败: {e}")))?;
        serde_json::from_str(&raw)
            .map_err(|e| AppError::InvalidInput(format!("解析资料库 `{name}` JSON 失败: {e}")))
    }

    pub fn save(&self, profile: &VaultProfile) -> Result<PathBuf, AppError> {
        fs::create_dir_all(&self.root_dir)
            .map_err(|e| AppError::Runtime(format!("创建资料库目录失败: {e}")))?;
        let name = sanitize_name(&profile.name);
        let path = self.profile_path(&name);
        let json = serde_json::to_string_pretty(profile)
            .map_err(|e| AppError::Runtime(format!("序列化资料库失败: {e}")))?;
        fs::write(&path, json)
            .map_err(|e| AppError::Runtime(format!("写入资料库 `{name}` 失败: {e}")))?;
        Ok(path)
    }

    pub fn list_profiles(&self) -> Vec<String> {
        let Ok(rd) = fs::read_dir(&self.root_dir) else {
            return Vec::new();
        };
        let mut names = Vec::new();
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        names
    }

    pub fn set_field(
        &self,
        profile_name: &str,
        category: &str,
        key: &str,
        value: &str,
    ) -> Result<VaultProfile, AppError> {
        let mut profile = self.load(profile_name)?;
        profile.name = sanitize_name(profile_name);
        profile
            .categories
            .entry(sanitize_name(category))
            .or_default()
            .insert(key.trim().to_string(), value.to_string());
        self.save(&profile)?;
        Ok(profile)
    }

    pub fn add_attachment(
        &self,
        profile_name: &str,
        category: &str,
        key: &str,
        label: &str,
        file_path: &Path,
    ) -> Result<VaultProfile, AppError> {
        if !file_path.is_file() {
            return Err(AppError::InvalidInput(format!(
                "附件文件不存在: {}",
                file_path.display()
            )));
        }
        let abs = fs::canonicalize(file_path).unwrap_or_else(|_| file_path.to_path_buf());
        let mime_hint = guess_mime(&abs);
        let mut profile = self.load(profile_name)?;
        profile.name = sanitize_name(profile_name);
        profile.attachments.insert(
            key.trim().to_string(),
            AttachmentEntry {
                category: sanitize_name(category),
                label: if label.is_empty() {
                    key.trim().to_string()
                } else {
                    label.to_string()
                },
                path: abs.display().to_string(),
                mime_hint: Some(mime_hint),
            },
        );
        self.save(&profile)?;
        Ok(profile)
    }

    pub fn import_json(
        &self,
        profile_name: &str,
        json_str: &str,
    ) -> Result<VaultProfile, AppError> {
        let incoming: VaultProfile = serde_json::from_str(json_str)
            .map_err(|e| AppError::InvalidInput(format!("导入资料 JSON 失败: {e}")))?;
        let mut existing = self.load(profile_name)?;
        existing.name = sanitize_name(profile_name);
        for (cat, fields) in incoming.categories {
            let entry = existing.categories.entry(cat).or_default();
            for (k, v) in fields {
                entry.insert(k, v);
            }
        }
        for (k, att) in incoming.attachments {
            existing.attachments.insert(k, att);
        }
        self.save(&existing)?;
        Ok(existing)
    }

    fn profile_path(&self, name: &str) -> PathBuf {
        self.root_dir.join(format!("{name}.json"))
    }
}

impl Default for VaultStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoMapResult {
    pub args: Map<String, Value>,
    pub mapping_notes: Vec<String>,
    pub missing_required: Vec<String>,
    pub unmapped_optional: Vec<String>,
}

/// 政务/企业设立登记常用字段语义同义词表：(canonical_category, canonical_key, keywords)
const SEMANTIC_RULES: &[(&str, &str, &[&str])] = &[
    (
        "company",
        "name",
        &[
            "企业名称",
            "公司名称",
            "单位名称",
            "商户名称",
            "entname",
            "company_name",
            "companyname",
            "org_name",
            "corp_name",
        ],
    ),
    (
        "company",
        "credit_code",
        &[
            "统一社会信用代码",
            "社会信用代码",
            "信用代码",
            "税号",
            "uscc",
            "uniscid",
            "credit_code",
            "creditcode",
            "tax_id",
        ],
    ),
    (
        "company",
        "capital",
        &[
            "注册资本",
            "注册资金",
            "出资额",
            "regcap",
            "registered_capital",
            "capital",
        ],
    ),
    (
        "company",
        "address",
        &[
            "住所",
            "注册地址",
            "经营场所",
            "办公地址",
            "企业地址",
            "dom",
            "address",
            "reg_address",
            "company_address",
        ],
    ),
    (
        "company",
        "scope",
        &["经营范围", "业务范围", "opscope", "business_scope", "scope"],
    ),
    (
        "company",
        "type",
        &[
            "企业类型",
            "公司类型",
            "组织形式",
            "enttype",
            "company_type",
        ],
    ),
    (
        "legal_rep",
        "name",
        &[
            "法定代表人",
            "法人姓名",
            "法人代表",
            "负责人姓名",
            "lerep",
            "legal_rep",
            "legal_person",
            "legalname",
        ],
    ),
    (
        "legal_rep",
        "id_card",
        &[
            "身份证号",
            "证件号码",
            "法人身份证",
            "cerno",
            "id_card",
            "idcard",
            "id_number",
            "cert_no",
        ],
    ),
    (
        "legal_rep",
        "phone",
        &[
            "法人手机",
            "联系电话",
            "手机号",
            "移动电话",
            "tel",
            "phone",
            "mobile",
            "contact_phone",
        ],
    ),
    ("contact", "email", &["电子邮箱", "邮箱", "email", "mail"]),
    (
        "contact",
        "name",
        &["经办人", "联络员", "联系人", "contact_name", "handler"],
    ),
    (
        "finance",
        "bank_name",
        &["开户银行", "开户行", "bank_name", "bank"],
    ),
    (
        "finance",
        "bank_account",
        &["银行账号", "基本户账号", "bank_account", "account_no"],
    ),
];

/// 附件材料语义同义词表：(attachment_key_hint, keywords)
const ATTACHMENT_RULES: &[(&str, &[&str])] = &[
    (
        "business_license",
        &["营业执照", "license", "biz_license", "business_license"],
    ),
    (
        "id_card_front",
        &[
            "身份证正面",
            "人像面",
            "id_front",
            "id_card_front",
            "身份证",
        ],
    ),
    (
        "id_card_back",
        &["身份证反面", "国徽面", "id_back", "id_card_back"],
    ),
    ("articles", &["公司章程", "章程", "articles", "bylaws"]),
    (
        "lease_contract",
        &[
            "租赁合同",
            "产权证明",
            "住所证明",
            "场地证明",
            "lease",
            "address_proof",
        ],
    ),
    (
        "power_of_attorney",
        &["授权委托书", "委托书", "attorney", "authorization"],
    ),
];

/// 将资料库（分类字段 + 附件）自动映射到目标表单命令的参数中。
pub fn auto_map_form(
    cmd: &WebCommandSpec,
    profile: &VaultProfile,
    overrides: &Map<String, Value>,
) -> AutoMapResult {
    let mut args = Map::new();
    let mut mapping_notes = Vec::new();
    let mut missing_required = Vec::new();
    let mut unmapped_optional = Vec::new();

    for arg in &cmd.args {
        // 1. Explicit override wins
        if let Some(ov) = overrides
            .get(&arg.name)
            .or_else(|| overrides.get(&arg.param_name))
        {
            args.insert(arg.name.clone(), ov.clone());
            mapping_notes.push(format!("{} <- [人工/显式指定]", arg.name));
            continue;
        }

        let haystack = format!(
            "{} {} {}",
            arg.name.to_ascii_lowercase(),
            arg.param_name.to_ascii_lowercase(),
            arg.about.to_ascii_lowercase()
        );

        // 2. File attachment matching
        if matches!(arg.kind, WebArgKind::Path) {
            if let Some((att_key, att)) = match_attachment(&haystack, profile) {
                args.insert(arg.name.clone(), Value::String(att.path.clone()));
                mapping_notes.push(format!(
                    "{} <- attachments.{att_key} [{}] ({})",
                    arg.name, att.category, att.path
                ));
                continue;
            }
        } else {
            // 3. Direct key match across categories
            if let Some((cat, key, val)) = match_field(&haystack, &arg.param_name, profile) {
                let typed_val = coerce_value(&arg.kind, &val);
                args.insert(arg.name.clone(), typed_val);
                mapping_notes.push(format!("{} <- {cat}.{key}", arg.name));
                continue;
            }
        }

        // 4. Fallback to form default
        if let Some(def) = &arg.default {
            args.insert(arg.name.clone(), def.clone());
            mapping_notes.push(format!("{} <- [表单默认值]", arg.name));
            continue;
        }

        if arg.required {
            missing_required.push(format!("{} ({})", arg.name, arg.about));
        } else {
            unmapped_optional.push(arg.name.clone());
        }
    }

    AutoMapResult {
        args,
        mapping_notes,
        missing_required,
        unmapped_optional,
    }
}

fn match_field(
    haystack: &str,
    param_name: &str,
    profile: &VaultProfile,
) -> Option<(String, String, String)> {
    // 1. Semantic dictionary match (strictly scoped to the matched category + key)
    for &(cat, key, keywords) in SEMANTIC_RULES {
        if keywords
            .iter()
            .any(|kw| haystack.contains(&kw.to_ascii_lowercase()))
        {
            if let Some(val) = profile.categories.get(cat).and_then(|m| m.get(key)) {
                return Some((cat.to_string(), key.to_string(), val.clone()));
            }
        }
    }

    // 2. Exact key match in any category
    let param_lc = param_name.to_ascii_lowercase();
    for (cat, fields) in &profile.categories {
        for (k, v) in fields {
            let k_lc = k.to_ascii_lowercase();
            if k_lc == param_lc || haystack.contains(&k_lc) && k_lc.len() >= 3 {
                return Some((cat.clone(), k.clone(), v.clone()));
            }
        }
    }
    None
}

fn match_attachment<'a>(
    haystack: &str,
    profile: &'a VaultProfile,
) -> Option<(String, &'a AttachmentEntry)> {
    // 1. Semantic attachment rules
    for &(att_key, keywords) in ATTACHMENT_RULES {
        if keywords
            .iter()
            .any(|kw| haystack.contains(&kw.to_ascii_lowercase()))
        {
            if let Some(att) = profile.attachments.get(att_key) {
                return Some((att_key.to_string(), att));
            }
        }
    }
    // 2. Direct key / label substring match
    for (k, att) in &profile.attachments {
        if haystack.contains(&k.to_ascii_lowercase())
            || haystack.contains(&att.label.to_ascii_lowercase())
        {
            return Some((k.clone(), att));
        }
    }
    None
}

fn coerce_value(kind: &WebArgKind, raw: &str) -> Value {
    match kind {
        WebArgKind::Flag => Value::Bool(matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes" | "on" | "是"
        )),
        WebArgKind::Number { .. } => raw
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(raw.to_string())),
        WebArgKind::Enum { values } => {
            // Try exact or case-insensitive match against enum choices
            if let Some(found) = values.iter().find(|v| v.eq_ignore_ascii_case(raw.trim())) {
                Value::String(found.clone())
            } else {
                Value::String(raw.to_string())
            }
        }
        _ => Value::String(raw.to_string()),
    }
}

pub fn guess_mime(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("txt") => "text/plain",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn default_vault_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg)
                .join("lilyco")
                .join("web2cli")
                .join("vault");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".config")
                .join("lilyco")
                .join("web2cli")
                .join("vault");
        }
    }
    std::env::temp_dir().join("lilyco-web2cli-vault")
}

fn sanitize_name(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "default".to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::discover_from_html;

    #[test]
    fn maps_enterprise_registration_form_and_attachments() {
        let dir = tempfile::tempdir().unwrap();
        let store = VaultStore::with_dir(dir.path().join("vault"));
        let license_file = dir.path().join("license.pdf");
        fs::write(&license_file, b"%PDF-1.4 mock").unwrap();

        store
            .set_field("acme", "company", "name", "星火科技有限公司")
            .unwrap();
        store
            .set_field("acme", "company", "credit_code", "91310000MA1K12345X")
            .unwrap();
        store
            .set_field("acme", "legal_rep", "name", "张三")
            .unwrap();
        store
            .set_field("acme", "legal_rep", "id_card", "310101199001011234")
            .unwrap();
        store
            .add_attachment(
                "acme",
                "证照",
                "business_license",
                "营业执照扫描件",
                &license_file,
            )
            .unwrap();

        let html = r#"
        <form id="ent_register" action="/gov/register" method="post" enctype="multipart/form-data">
          <label for="f1">企业名称</label>
          <input id="f1" type="text" name="entName" required>
          <label for="f2">统一社会信用代码</label>
          <input id="f2" type="text" name="uniscid" required>
          <label for="f3">法定代表人</label>
          <input id="f3" type="text" name="lerep" required>
          <label for="f4">营业执照</label>
          <input id="f4" type="file" name="licenseFile" required>
          <label for="f5">紧急联系人备注</label>
          <input id="f5" type="text" name="emergencyNote" required>
        </form>
        "#;

        let spec = discover_from_html("https://zwfw.gov.example/", html);
        let cmd = spec
            .commands
            .iter()
            .find(|c| c.name == "ent-register")
            .unwrap();
        let profile = store.load("acme").unwrap();
        let mapped = auto_map_form(cmd, &profile, &Map::new());

        assert_eq!(mapped.args["entname"], "星火科技有限公司");
        assert_eq!(mapped.args["uniscid"], "91310000MA1K12345X");
        assert_eq!(mapped.args["lerep"], "张三");
        assert!(mapped.args["licensefile"]
            .as_str()
            .unwrap()
            .ends_with("license.pdf"));
        assert_eq!(mapped.missing_required.len(), 1);
        assert!(mapped.missing_required[0].contains("emergencynote"));
    }
}
