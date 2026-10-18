//! The smallest useful tool set: read, list, and exact-match edit.
//!
//! Each tool is **sandboxed to a workspace root**: arguments are relative paths,
//! resolved against that root and re-checked for traversal. Three tools are
//! enough to prove the loop does real work.

use ::std::path::{Path, PathBuf};
use ::std::string::String;

use crate::adapters::fs_engine::{
    count_lines, plan_replace, render_numbered, sanitize_rel, split_at, Window,
};
use crate::agent::tool::{ObsBuf, ObsKind, Tool, ToolError, ToolSpec};

/// Read a UTF-8 file with line numbers, relative to `root`.
pub struct ReadFile {
    root: PathBuf,
}

impl ReadFile {
    /// Sandbox the tool to `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        ReadFile { root: root.into() }
    }
}

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new("read_file", "read a UTF-8 file with line numbers", false)
    }

    fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let full = resolve(&self.root, args)?;
        let src = std::fs::read_to_string(&full)
            .map_err(|_| ToolError::HostFailure("cannot read file"))?;
        let total = count_lines(&src);
        let window = Window::plan(total, 0, 120);
        render_numbered(&src, window, out).map_err(|_| ToolError::ObservationTooLarge)?;
        Ok(ObsKind::Text)
    }
}

/// List the workspace root, one entry per line.
pub struct ListDir {
    root: PathBuf,
}

impl ListDir {
    /// Sandbox the tool to `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        ListDir { root: root.into() }
    }
}

impl Tool for ListDir {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new("list_dir", "list the workspace root", false)
    }

    fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let dir = resolve(&self.root, args)?;
        let entries = std::fs::read_dir(&dir).map_err(|_| ToolError::HostFailure("cannot list dir"))?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let shown = name.to_string_lossy();
            if out.push_str(&shown).is_err() || out.push('\n').is_err() {
                return Err(ToolError::ObservationTooLarge);
            }
        }
        Ok(ObsKind::Text)
    }
}

/// Replace exactly one occurrence. Args are `path|old|new`.
pub struct EditFile {
    root: PathBuf,
}

impl EditFile {
    /// Sandbox the tool to `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        EditFile { root: root.into() }
    }
}

impl Tool for EditFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new("edit_file", "replace one exact occurrence: path|old|new", true)
    }

    fn call(&self, args: &str, _out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let (path, old, new) = split_args(args)?;
        let full = resolve(&self.root, path)?;
        let src = std::fs::read_to_string(&full)
            .map_err(|_| ToolError::HostFailure("cannot read file"))?;
        let span = plan_replace(&src, old).map_err(ToolError::InvalidArgument)?;
        let (head, tail) = split_at(&src, span).map_err(ToolError::InvalidArgument)?;
        let mut next = String::with_capacity(head.len() + new.len() + tail.len());
        next.push_str(head);
        next.push_str(new);
        next.push_str(tail);
        std::fs::write(&full, next).map_err(|_| ToolError::HostFailure("cannot write file"))?;
        Ok(ObsKind::Diff)
    }
}

/// Join a relative argument onto the sandbox root, refusing escapes.
fn resolve(root: &Path, args: &str) -> Result<PathBuf, ToolError> {
    let raw = if args.trim().is_empty() { "." } else { args.trim() };
    let rel = sanitize_rel(raw).map_err(ToolError::NotPermitted)?;
    Ok(root.join(rel))
}

fn split_args(args: &str) -> Result<(&str, &str, &str), ToolError> {
    let mut parts = args.splitn(3, '|');
    let path = parts.next().ok_or(ToolError::InvalidArgument("missing path"))?;
    let old = parts.next().ok_or(ToolError::InvalidArgument("missing old text"))?;
    let new = parts.next().ok_or(ToolError::InvalidArgument("missing new text"))?;
    if old.is_empty() {
        return Err(ToolError::InvalidArgument("old text is empty"));
    }
    Ok((path.trim(), old, new))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(::std::format!("tiny_lilyco_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_file_numbers_lines() {
        let dir = scratch("read");
        std::fs::write(dir.join("a.txt"), "alpha\nbeta\n").unwrap();
        let mut out = ObsBuf::new();
        let kind = ReadFile::new(&dir).call("a.txt", &mut out).unwrap();
        assert_eq!(kind, ObsKind::Text);
        assert!(out.as_str().contains("1| alpha"));
        assert!(out.as_str().contains("2| beta"));
    }

    #[test]
    fn read_file_rejects_traversal() {
        let dir = scratch("escape");
        let mut out = ObsBuf::new();
        assert_eq!(
            ReadFile::new(&dir).call("../secrets", &mut out),
            Err(ToolError::NotPermitted("path traversal is not allowed"))
        );
    }

    #[test]
    fn edit_file_replaces_exactly_once() {
        let dir = scratch("edit");
        std::fs::write(dir.join("b.txt"), "let a = 1;\n").unwrap();
        let mut out = ObsBuf::new();
        let kind = EditFile::new(&dir)
            .call("b.txt|a = 1|a = 2", &mut out)
            .unwrap();
        assert_eq!(kind, ObsKind::Diff);
        assert_eq!(
            std::fs::read_to_string(dir.join("b.txt")).unwrap(),
            "let a = 2;\n"
        );
    }

    #[test]
    fn edit_file_refuses_ambiguous_anchor() {
        let dir = scratch("amb");
        std::fs::write(dir.join("c.txt"), "x x\n").unwrap();
        let mut out = ObsBuf::new();
        assert_eq!(
            EditFile::new(&dir).call("c.txt|x|y", &mut out),
            Err(ToolError::InvalidArgument("needle is ambiguous"))
        );
    }

    #[test]
    fn list_dir_shows_entries() {
        let dir = scratch("list");
        std::fs::write(dir.join("keep.txt"), "x").unwrap();
        let mut out = ObsBuf::new();
        assert_eq!(ListDir::new(&dir).call("", &mut out).unwrap(), ObsKind::Text);
        assert!(out.as_str().contains("keep.txt"));
    }
}
