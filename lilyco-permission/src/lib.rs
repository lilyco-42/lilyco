//! `lilyco-permission`: capability + resource + field policy for Lilyco tools.
//!
//! The default is deny. A caller must match an explicit allow rule; deny rules
//! always win. Rules can be scoped by agent, role, capability, resource and
//! field, which lets an agent list users without reading their conversations:
//!
//! ```toml
//! default = "deny"
//!
//! [[rules]]
//! agents = ["codearts"]
//! capabilities = ["contacts.list", "groups.list", "members.list"]
//! resources = ["*"]
//! fields = ["id", "name", "kind"]
//! effect = "allow"
//!
//! [[rules]]
//! agents = ["codearts"]
//! capabilities = ["messages.read"]
//! resources = ["group:1025622267"]
//! fields = ["id", "time", "sender", "text"]
//! effect = "allow"
//! ```
//!
//! The crate is independent of any protocol or UI. `lilyco-qq` maps OneBot
//! actions to capabilities and resources, then uses [`Policy::check`] before
//! touching the network or serializing a result.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PermissionError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid permission TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid permission value: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, PermissionError>;

/// The caller asking for access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

impl Principal {
    pub fn agent(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            roles: vec!["agent".into()],
        }
    }

    pub fn anonymous() -> Self {
        Self {
            id: "anonymous".into(),
            roles: vec![],
        }
    }

    fn matches_rule(&self, rule: &Rule) -> bool {
        let agent_ok = rule.agents.is_empty()
            || rule.agents.iter().any(|agent| {
                agent == "*" || agent == &self.id || self.roles.iter().any(|role| role == agent)
            });
        let role_ok = rule.roles.is_empty()
            || rule
                .roles
                .iter()
                .any(|role| role == "*" || self.roles.iter().any(|mine| mine == role));
        agent_ok && role_ok
    }
}

/// A capability such as `contacts.list`, `messages.read`, `messages.content`,
/// `actions.send` or `system.admin`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capability(pub String);

impl Capability {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn matches(&self, pattern: &str) -> bool {
        if pattern == "*" || pattern == self.0 {
            return true;
        }
        pattern
            .strip_suffix(".*")
            .is_some_and(|prefix| self.0 == prefix || self.0.starts_with(&format!("{prefix}.")))
    }
}

/// A resource such as `group:1025622267`, `user:1957374829` or `message:42`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Resource {
    pub kind: String,
    pub id: String,
}

impl Resource {
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
        }
    }

    pub fn any() -> Self {
        Self::new("*", "*")
    }

    pub fn parse(value: &str) -> Result<Self> {
        let (kind, id) = value.split_once(':').ok_or_else(|| {
            PermissionError::Invalid(format!("resource must be kind:id, got {value}"))
        })?;
        if kind.is_empty() || id.is_empty() {
            return Err(PermissionError::Invalid(format!(
                "resource must be kind:id, got {value}"
            )));
        }
        Ok(Self::new(kind, id))
    }

    pub fn matches(&self, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        if let Ok(resource) = Self::parse(pattern) {
            return (resource.kind == "*" || resource.kind == self.kind)
                && (resource.id == "*" || resource.id == self.id);
        }
        false
    }
}

impl std::fmt::Display for Resource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.kind, self.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Deny,
}

fn deny_effect() -> Effect {
    Effect::Deny
}

fn all_patterns() -> Vec<String> {
    vec!["*".into()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    pub capabilities: Vec<String>,
    #[serde(default = "all_patterns")]
    pub resources: Vec<String>,
    #[serde(default = "all_patterns")]
    pub fields: Vec<String>,
    pub effect: Effect,
    #[serde(default)]
    pub require_confirm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default = "deny_effect")]
    pub default: Effect,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            default: Effect::Deny,
            rules: Vec::new(),
        }
    }
}

impl Policy {
    pub fn from_toml(raw: &str) -> Result<Self> {
        let policy: Self = toml::from_str(raw)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::from_toml(&fs::read_to_string(path)?)
    }

    fn validate(&self) -> Result<()> {
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.capabilities.is_empty() {
                return Err(PermissionError::Invalid(format!(
                    "rule {index} has no capabilities"
                )));
            }
            for resource in &rule.resources {
                if resource != "*" {
                    Resource::parse(resource)?;
                }
            }
        }
        Ok(())
    }

    pub fn check(
        &self,
        principal: &Principal,
        capability: &Capability,
        resource: &Resource,
        requested_fields: &BTreeSet<String>,
    ) -> Decision {
        let mut allowed_fields = BTreeSet::new();
        let mut denied_fields = BTreeSet::new();
        let mut wildcard_fields = false;
        let mut deny_all_fields = false;
        let mut require_confirm = false;
        let mut matched_allow = false;

        let mut apply = |rule: &Rule, allow: bool| {
            let field_wildcard = rule.fields.iter().any(|field| field == "*");
            if allow {
                matched_allow = true;
                wildcard_fields |= field_wildcard;
                if !field_wildcard {
                    allowed_fields.extend(rule.fields.iter().cloned());
                }
                require_confirm |= rule.require_confirm;
            } else if field_wildcard {
                deny_all_fields = true;
            } else {
                denied_fields.extend(rule.fields.iter().cloned());
            }
        };

        for rule in &self.rules {
            if !principal.matches_rule(rule)
                || !rule
                    .capabilities
                    .iter()
                    .any(|item| capability.matches(item))
                || !rule.resources.iter().any(|item| resource.matches(item))
            {
                continue;
            }
            match rule.effect {
                Effect::Allow => apply(rule, true),
                Effect::Deny => apply(rule, false),
            }
        }

        if !matched_allow || deny_all_fields {
            return Decision::deny(
                format!("no allow rule for {} on {resource}", capability.0),
                require_confirm,
            );
        }

        if wildcard_fields && denied_fields.is_empty() {
            return Decision {
                allowed: true,
                fields: ["*".to_string()].into_iter().collect(),
                denied_fields,
                require_confirm,
                reason: String::new(),
            };
        }

        if wildcard_fields {
            let effective: BTreeSet<String> = if requested_fields.is_empty() {
                allowed_fields.clone()
            } else {
                requested_fields
                    .iter()
                    .filter(|field| !denied_fields.contains(*field))
                    .cloned()
                    .collect()
            };
            if !requested_fields.is_empty() && effective.is_empty() {
                return Decision {
                    allowed: false,
                    fields: effective,
                    denied_fields,
                    require_confirm,
                    reason: format!("field access denied for {} on {resource}", capability.0),
                };
            }
            return Decision {
                allowed: true,
                fields: effective,
                denied_fields,
                require_confirm,
                reason: String::new(),
            };
        }

        allowed_fields.retain(|field| !denied_fields.contains(field));
        let effective_allowed = requested_fields.is_empty()
            || requested_fields
                .iter()
                .all(|field| allowed_fields.contains(field));

        if !effective_allowed {
            return Decision {
                allowed: false,
                fields: allowed_fields,
                denied_fields,
                require_confirm,
                reason: format!("field access denied for {} on {resource}", capability.0),
            };
        }

        Decision {
            allowed: true,
            fields: allowed_fields,
            denied_fields,
            require_confirm,
            reason: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub allowed: bool,
    pub fields: BTreeSet<String>,
    #[serde(default)]
    pub denied_fields: BTreeSet<String>,
    pub require_confirm: bool,
    pub reason: String,
}

impl Decision {
    pub fn deny(reason: impl Into<String>, require_confirm: bool) -> Self {
        Self {
            allowed: false,
            fields: BTreeSet::new(),
            denied_fields: BTreeSet::new(),
            require_confirm,
            reason: reason.into(),
        }
    }

    pub fn allows_field(&self, field: &str) -> bool {
        !self.denied_fields.contains(field)
            && (self.fields.contains("*") || self.fields.contains(field))
    }

    pub fn allows_all_fields(&self) -> bool {
        self.fields.contains("*") && self.denied_fields.is_empty()
    }

    /// Keep only allowed keys on a flat JSON object. Unknown keys are dropped
    /// when the decision is not a wildcard, which makes the default fail-closed.
    pub fn retain_fields(&self, value: &mut serde_json::Value) {
        if self.allows_all_fields() {
            return;
        }
        if let serde_json::Value::Object(object) = value {
            object.retain(|key, _| {
                !self.denied_fields.contains(key)
                    && (self.fields.contains("*") || self.fields.contains(key))
            });
        }
    }
}

/// One append-only audit record. Callers decide where to persist it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub at_unix: u64,
    pub principal: Principal,
    pub capability: Capability,
    pub resource: Resource,
    pub requested_fields: BTreeSet<String>,
    pub decision: Decision,
}

impl AuditRecord {
    pub fn new(
        principal: Principal,
        capability: Capability,
        resource: Resource,
        requested_fields: BTreeSet<String>,
        decision: Decision,
    ) -> Self {
        Self {
            at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
            principal,
            capability,
            resource,
            requested_fields,
            decision,
        }
    }

    pub fn append_jsonl(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        serde_json::to_writer(&mut file, self)?;
        file.write_all(b"\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    fn policy() -> Policy {
        Policy::from_toml(
            r#"
default = "deny"

[[rules]]
agents = ["codearts"]
capabilities = ["contacts.list", "groups.list", "members.list", "identity.read"]
resources = ["*"]
fields = ["id", "name", "kind", "user_id", "group_id", "nickname", "group_name"]
effect = "allow"

[[rules]]
agents = ["codearts"]
capabilities = ["messages.read"]
resources = ["group:1025622267"]
fields = ["id", "time", "sender", "text", "parts"]
effect = "allow"

[[rules]]
agents = ["codearts"]
capabilities = ["actions.send"]
resources = ["group:1025622267"]
fields = ["*"]
effect = "allow"
require_confirm = true
"#,
        )
        .unwrap()
    }

    #[test]
    fn default_is_deny() {
        let decision = policy().check(
            &Principal::agent("codearts"),
            &Capability::new("files.read"),
            &Resource::any(),
            &fields(&["url"]),
        );
        assert!(!decision.allowed);
    }

    #[test]
    fn contact_list_is_allowed_but_chat_is_not() {
        let p = Principal::agent("codearts");
        let contacts = policy().check(
            &p,
            &Capability::new("contacts.list"),
            &Resource::any(),
            &fields(&["id", "name"]),
        );
        assert!(contacts.allowed);
        let chat = policy().check(
            &p,
            &Capability::new("messages.read"),
            &Resource::new("group", "1048332900"),
            &fields(&["id", "text"]),
        );
        assert!(!chat.allowed);
    }

    #[test]
    fn resource_allow_is_scoped() {
        let p = Principal::agent("codearts");
        let allowed = policy().check(
            &p,
            &Capability::new("messages.read"),
            &Resource::new("group", "1025622267"),
            &fields(&["text"]),
        );
        assert!(allowed.allowed);
        let denied = policy().check(
            &p,
            &Capability::new("messages.read"),
            &Resource::new("group", "1025622268"),
            &fields(&["text"]),
        );
        assert!(!denied.allowed);
    }

    #[test]
    fn fields_are_filtered() {
        let p = Principal::agent("codearts");
        let decision = policy().check(
            &p,
            &Capability::new("contacts.list"),
            &Resource::any(),
            &fields(&["id", "name", "text"]),
        );
        assert!(!decision.allowed, "text is not an allowed contact field");
        let decision = policy().check(
            &p,
            &Capability::new("contacts.list"),
            &Resource::any(),
            &fields(&["id", "name"]),
        );
        let mut value = json!({
            "id": "1957374829",
            "name": "兰雀",
            "text": "must not leak",
            "token": "must not leak"
        });
        decision.retain_fields(&mut value);
        assert_eq!(value["id"], "1957374829");
        assert!(value.get("text").is_none());
        assert!(value.get("token").is_none());
    }

    #[test]
    fn wildcard_fields_are_allowed() {
        let p = Principal::agent("codearts");
        let decision = policy().check(
            &p,
            &Capability::new("actions.send"),
            &Resource::new("group", "1025622267"),
            &fields(&["anything"]),
        );
        assert!(decision.allowed);
        assert!(decision.require_confirm);
        assert!(decision.allows_all_fields());
    }

    #[test]
    fn unknown_agent_is_denied() {
        let decision = policy().check(
            &Principal::agent("other"),
            &Capability::new("contacts.list"),
            &Resource::any(),
            &fields(&["id"]),
        );
        assert!(!decision.allowed);
    }

    #[test]
    fn malformed_resource_is_rejected() {
        assert!(Resource::parse("group").is_err());
        assert!(Resource::parse(":123").is_err());
    }

    #[test]
    fn deny_rule_overrides_allow() {
        let policy = Policy::from_toml(
            r#"
default = "deny"
[[rules]]
agents = ["codearts"]
capabilities = ["messages.*"]
resources = ["group:*"]
fields = ["*"]
effect = "allow"
[[rules]]
agents = ["codearts"]
capabilities = ["messages.content"]
resources = ["group:1025622267"]
fields = ["text"]
effect = "deny"
"#,
        )
        .unwrap();
        let p = Principal::agent("codearts");
        let metadata = policy.check(
            &p,
            &Capability::new("messages.metadata"),
            &Resource::new("group", "1025622267"),
            &fields(&["id", "time"]),
        );
        assert!(metadata.allowed);
        let content = policy.check(
            &p,
            &Capability::new("messages.content"),
            &Resource::new("group", "1025622267"),
            &fields(&["text"]),
        );
        assert!(!content.allowed);
    }
}
