//! Read the provider that cc-switch has activated.
//!
//! cc-switch keeps its catalogue in a SQLite file (`~/.cc-switch/cc-switch.db`),
//! but it also **materialises the active provider** into JSON that Claude Code
//! itself consumes. Reading that JSON is strictly better for us: no SQLite
//! engine, no schema drift, and it is by definition the config the user is
//! actually running.
//!
//! * `~/.claude/settings.json` → `env.ANTHROPIC_BASE_URL` / `AUTH_TOKEN` / model
//! * `~/.cc-switch/settings.json` → `currentProviderClaude` (for reporting)

use ::std::path::{Path, PathBuf};
use ::std::string::String;

use serde_json::Value;

/// What cc-switch currently points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Active {
    /// Base URL, e.g. `http://127.0.0.1:15721` (its local proxy) or upstream.
    pub base_url: String,
    /// Bearer / `x-api-key` value.
    pub api_key: String,
    /// Model id as the proxy expects it.
    pub model: String,
    /// Provider id from cc-switch's own settings, for the trace.
    pub provider: String,
}

/// Locate the user's home directory without pulling in a crate.
pub fn home_dir() -> Option<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    ::std::env::var(key).ok().map(PathBuf::from)
}

fn read_json(path: &Path) -> Result<Value, String> {
    let raw = ::std::fs::read_to_string(path)
        .map_err(|e| ::std::format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| ::std::format!("bad json in {}: {e}", path.display()))
}

/// Load the active provider, preferring the files cc-switch writes.
pub fn load() -> Result<Active, String> {
    let home = home_dir().ok_or("cannot locate the home directory")?;
    let claude = read_json(&home.join(".claude").join("settings.json"))?;
    let env = claude
        .get("env")
        .ok_or("no `env` block in ~/.claude/settings.json")?;
    let base_url = pick(env, &["ANTHROPIC_BASE_URL"])?;
    let api_key = pick(env, &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"])?;
    let model = pick(
        env,
        &[
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
        ],
    )?;
    let provider = read_json(&home.join(".cc-switch").join("settings.json"))
        .ok()
        .and_then(|v| {
            v.get("currentProviderClaude")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_else(|| "cc-switch".to_string());
    Ok(Active {
        base_url,
        api_key,
        model,
        provider,
    })
}

fn pick(env: &Value, keys: &[&str]) -> Result<String, String> {
    for key in keys {
        if let Some(value) = env.get(key).and_then(Value::as_str) {
            if !value.trim().is_empty() {
                return Ok(value.trim().to_string());
            }
        }
    }
    Err(::std::format!("missing {} in env", keys.first().copied().unwrap_or("")))
}

/// Redact a secret for logs: keep a prefix, drop the rest.
pub fn mask(secret: &str) -> String {
    let keep = secret.len().min(6);
    let head = secret.get(..keep).unwrap_or("");
    ::std::format!("{head}****")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_keep_only_a_prefix() {
        assert_eq!(mask("PROXY_abcdef123456"), "PROXY_****");
        assert_eq!(mask("short"), "short****");
    }

    #[test]
    fn missing_keys_are_reported_by_name() {
        let env: Value = serde_json::from_str(r#"{"ANTHROPIC_MODEL":"m"}"#).unwrap();
        let err = pick(&env, &["ANTHROPIC_BASE_URL"]).unwrap_err();
        assert!(err.contains("ANTHROPIC_BASE_URL"));
    }

    #[test]
    fn fallback_keys_are_tried_in_order() {
        let env: Value =
            serde_json::from_str(r#"{"ANTHROPIC_API_KEY":"second"}"#).unwrap();
        assert_eq!(
            pick(&env, &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]).unwrap(),
            "second"
        );
    }

    #[test]
    fn empty_values_are_skipped() {
        let env: Value =
            serde_json::from_str(r#"{"ANTHROPIC_BASE_URL":"  ","ANTHROPIC_MODEL":"m"}"#).unwrap();
        assert_eq!(pick(&env, &["ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL"]).unwrap(), "m");
    }
}
