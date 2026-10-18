//! Provider catalogue, read straight out of cc-switch's SQLite database.
//!
//! cc-switch keeps every provider in `~/.cc-switch/cc-switch.db`, table
//! `providers`, column `settings_config` (a JSON `env` block). Reading that file
//! directly means Tiny Agent can offer the same list even when the cc-switch UI
//! is not running.
//!
//! The user's *choice* is stored in our own file, never written back into
//! cc-switch's database: a viewer must not mutate the thing it reads.

use std::path::{Path, PathBuf};
use std::string::String;
use std::vec::Vec;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

/// One selectable provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// Stable id from cc-switch.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Base URL for the Anthropic Messages API.
    pub base_url: String,
    /// Bearer / `x-api-key`.
    pub token: String,
    /// Model id.
    pub model: String,
    /// True when cc-switch marks it as current.
    pub is_current: bool,
}

impl Provider {
    /// Label for the UI: name plus a hint of the host.
    pub fn label(&self) -> String {
        let host = self
            .base_url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let short: String = host.chars().take(28).collect();
        format!("{} · {short}", self.name)
    }
}

/// `~/.cc-switch/cc-switch.db`
pub fn db_path() -> Option<PathBuf> {
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok()?;
    Some(PathBuf::from(home).join(".cc-switch").join("cc-switch.db"))
}

/// Our own record of the user's choice.
fn choice_path() -> Option<PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    Some(Path::new(&base).join("Tiny Agent").join("provider.json"))
}

/// Read every Claude provider cc-switch knows about.
pub fn list() -> Result<Vec<Provider>, String> {
    let path = db_path().ok_or("cannot locate the home directory")?;
    if !path.exists() {
        return Err(format!("cc-switch database not found: {}", path.display()));
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let con = Connection::open_with_flags(&path, flags)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut stmt = con
        .prepare(
            "select id, name, settings_config, is_current \
             from providers where app_type = 'claude' order by sort_index",
        )
        .map_err(|e| format!("query failed: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3).unwrap_or(0),
            ))
        })
        .map_err(|e| format!("query failed: {e}"))?;
    let mut out = Vec::new();
    for row in rows.flatten() {
        if let Some(provider) = from_row(row.0, row.1, &row.2, row.3 != 0) {
            out.push(provider);
        }
    }
    Ok(out)
}

fn from_row(id: String, name: String, config: &str, is_current: bool) -> Option<Provider> {
    let parsed: Value = serde_json::from_str(config).ok()?;
    let env = parsed.get("env")?;
    let base_url = text(env, &["ANTHROPIC_BASE_URL"])?;
    let token = text(env, &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"])?;
    let model = text(
        env,
        &[
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
        ],
    )?;
    Some(Provider {
        id,
        name,
        base_url,
        token,
        model,
        is_current,
    })
}

fn text(env: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = env.get(*key).and_then(Value::as_str) {
            if !value.trim().is_empty() {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// The id the user picked in Tiny Agent, if any.
pub fn chosen_id() -> Option<String> {
    let path = choice_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    value.get("id").and_then(Value::as_str).map(String::from)
}

/// Remember a choice for the next run.
pub fn choose(id: &str) -> Result<(), String> {
    let path = choice_path().ok_or("cannot locate LOCALAPPDATA")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let body = serde_json::json!({ "id": id }).to_string();
    std::fs::write(&path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Resolve which provider a run should use: our choice, else cc-switch's current.
pub fn resolve(providers: &[Provider]) -> Option<Provider> {
    if let Some(id) = chosen_id() {
        if let Some(hit) = providers.iter().find(|p| p.id == id) {
            return Some(hit.clone());
        }
    }
    providers
        .iter()
        .find(|p| p.is_current)
        .cloned()
        .or_else(|| providers.first().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Provider {
        Provider {
            id: "a".into(),
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com/anthropic".into(),
            token: "sk-x".into(),
            model: "m".into(),
            is_current: false,
        }
    }

    #[test]
    fn label_shows_name_and_host() {
        let label = sample().label();
        assert!(label.starts_with("DeepSeek · api.deepseek.com"));
    }

    #[test]
    fn row_parsing_reads_the_env_block() {
        let cfg = r#"{"env":{"ANTHROPIC_BASE_URL":"http://x","ANTHROPIC_AUTH_TOKEN":"t","ANTHROPIC_DEFAULT_SONNET_MODEL":"m"}}"#;
        let p = from_row("id".into(), "n".into(), cfg, true).unwrap();
        assert_eq!(p.base_url, "http://x");
        assert_eq!(p.token, "t");
        assert!(p.is_current);
    }

    #[test]
    fn incomplete_rows_are_skipped() {
        assert!(from_row("i".into(), "n".into(), r#"{"env":{}}"#, false).is_none());
        assert!(from_row("i".into(), "n".into(), "not json", false).is_none());
    }

    #[test]
    fn resolve_prefers_the_current_provider() {
        let mut a = sample();
        a.id = "a".into();
        let mut b = sample();
        b.id = "b".into();
        b.is_current = true;
        let picked = resolve(&[a, b]).unwrap();
        assert_eq!(picked.id, "b");
    }
}
