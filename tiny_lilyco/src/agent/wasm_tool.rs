//! WASM tool host: run someone else's tool instead of re-implementing it.
//!
//! The rule this module exists to enforce: **the agent should not hand-write
//! grep/glob/rewrite logic**. Established tools (ast-grep, ripgrep wrappers,
//! formatters, language servers) already solved those problems better than a
//! weekend reimplementation, so the framework's job is to *host* them.
//!
//! A plugin is a `wasm32` module exporting `run(input: String) -> String`
//! (the Extism PDK convention). The host loads the bytes, calls `run`, and
//! wraps the result as an [`ObsKind::Text`] observation.
//!
//! Wiring ast-grep specifically: compile `ast-grep-core` plus a small PDK
//! wrapper for `wasm32-unknown-unknown` and register it with
//! `--wasm-tool ast_grep=<path>.wasm`. The tree-sitter parsers it needs are C,
//! so that build wants `wasm32-wasi` (or a C-to-wasm cross toolchain); that is a
//! packaging task, not an architecture change.

use std::path::Path;

use extism::{Manifest, Plugin, Wasm};

use crate::agent::tool::{ObsBuf, ObsKind, Tool, ToolError, ToolSpec};

/// A tool implemented outside this repository.
pub struct WasmTool {
    name: &'static str,
    bytes: Vec<u8>,
}

impl WasmTool {
    /// Load a plugin from disk. The bytes are kept, so one file can back several
    /// instances without re-reading.
    pub fn load(name: &'static str, path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path)
            .map_err(|e| std::format!("cannot read {}: {e}", path.display()))?;
        if bytes.len() < 8 || &bytes[0..4] != b"\0asm" {
            return Err(std::format!("{} is not a wasm module", path.display()));
        }
        Ok(WasmTool { name, bytes })
    }

    /// Call the plugin's `run` export.
    ///
    /// A fresh instance per call keeps plugins from smuggling state between
    /// turns; tool calls are milliseconds, so the cost is irrelevant here.
    pub fn invoke(&self, args: &str) -> Result<String, String> {
        let manifest = Manifest::new([Wasm::data(self.bytes.clone())]);
        let mut plugin = Plugin::new(&manifest, [], true).map_err(|e| e.to_string())?;
        plugin
            .call::<&str, String>("run", args)
            .map_err(|e| e.to_string())
    }
}

impl Tool for WasmTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(self.name, "external wasm tool", true)
    }

    fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let text = self.invoke(args).map_err(ToolError::HostFailure)?;
        out.push_str(&text)
            .map_err(|_| ToolError::ObservationTooLarge)?;
        Ok(ObsKind::Text)
    }
}

/// Parse `name=path` as used by `--wasm-tool`.
pub fn parse_spec(raw: &str) -> Result<(String, String), String> {
    let (name, path) = raw
        .split_once('=')
        .ok_or("expected --wasm-tool name=path/to/tool.wasm")?;
    if name.is_empty() || path.is_empty() {
        return Err("both name and path are required".to_string());
    }
    Ok((name.to_string(), path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_parsing_rejects_malformed_input() {
        assert!(parse_spec("nope").is_err());
        assert!(parse_spec("=x").is_err());
        assert!(parse_spec("x=").is_err());
        assert_eq!(
            parse_spec("ast_grep=tools/ag.wasm").unwrap(),
            ("ast_grep".to_string(), "tools/ag.wasm".to_string())
        );
    }

    #[test]
    fn non_wasm_files_are_rejected_before_instantiation() {
        let path = std::env::temp_dir().join("tiny_lilyco_not_wasm.wasm");
        std::fs::write(&path, b"hello").unwrap();
        assert!(WasmTool::load("t", &path).is_err());
        let _ = std::fs::remove_file(path);
    }
}
