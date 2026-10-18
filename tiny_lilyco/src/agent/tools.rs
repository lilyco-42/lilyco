//! The smallest useful tool set: read, list, and exact-match edit.
//!
//! Three tools are enough to prove the loop does real work, and they are the
//! same primitives the future agent needs: precise observation, safe paths, and
//! a replace that refuses to guess.

use ::std::string::String;

use crate::adapters::fs_engine::{
    count_lines, plan_replace, render_numbered, sanitize_rel, split_at, Window,
};
use crate::agent::tool::{ObsBuf, ObsKind, Tool, ToolError, ToolSpec};

/// Read a UTF-8 file with line numbers.
pub struct ReadFile;

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new("read_file", "read a UTF-8 file with line numbers", false)
    }

    fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let path = sanitize_rel(args.trim()).map_err(ToolError::NotPermitted)?;
        let src = std::fs::read_to_string(path).map_err(|_| ToolError::HostFailure("cannot read file"))?;
        let total = count_lines(&src);
        let window = Window::plan(total, 0, 120);
        render_numbered(&src, window, out).map_err(|_| ToolError::ObservationTooLarge)?;
        Ok(ObsKind::Text)
    }
}

/// List a directory, one entry per line.
pub struct ListDir;

impl Tool for ListDir {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new("list_dir", "list a directory", false)
    }

    fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let raw = if args.trim().is_empty() { "." } else { args.trim() };
        let dir = sanitize_rel(raw).map_err(ToolError::NotPermitted)?;
        let entries = std::fs::read_dir(dir).map_err(|_| ToolError::HostFailure("cannot list dir"))?;
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
pub struct EditFile;

impl Tool for EditFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new("edit_file", "replace one exact occurrence: path|old|new", true)
    }

    fn call(&self, args: &str, _out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let (path, old, new) = split_args(args)?;
        let rel = sanitize_rel(path).map_err(ToolError::NotPermitted)?;
        let src = std::fs::read_to_string(rel).map_err(|_| ToolError::HostFailure("cannot read file"))?;
        let span = plan_replace(&src, old).map_err(ToolError::InvalidArgument)?;
        let (head, tail) = split_at(&src, span).map_err(ToolError::InvalidArgument)?;
        let mut next = String::with_capacity(head.len() + new.len() + tail.len());
        next.push_str(head);
        next.push_str(new);
        next.push_str(tail);
        std::fs::write(rel, next).map_err(|_| ToolError::HostFailure("cannot write file"))?;
        Ok(ObsKind::Diff)
    }
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

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("tiny_lilyco_tools");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn read_file_numbers_lines() {
        let path = scratch("read.txt");
        std::fs::write(&path, "alpha\nbeta\n").unwrap();
        let mut out = ObsBuf::new();
        let kind = ReadFile.call(path.to_str().unwrap(), &mut out).unwrap();
        assert_eq!(kind, ObsKind::Text);
        assert!(out.as_str().contains("1| alpha"));
        assert!(out.as_str().contains("2| beta"));
    }

    #[test]
    fn read_file_rejects_traversal() {
        let mut out = ObsBuf::new();
        assert_eq!(
            ReadFile.call("../secrets", &mut out),
            Err(ToolError::NotPermitted("path traversal is not allowed"))
        );
    }

    #[test]
    fn edit_file_replaces_exactly_once() {
        let path = scratch("edit.txt");
        std::fs::write(&path, "let a = 1;\n").unwrap();
        let args = ::std::format!("{}|a = 1|a = 2", path.to_str().unwrap());
        let mut out = ObsBuf::new();
        assert_eq!(EditFile.call(&args, &mut out).unwrap(), ObsKind::Diff);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "let a = 2;\n");
    }

    #[test]
    fn edit_file_refuses_ambiguous_anchor() {
        let path = scratch("amb.txt");
        std::fs::write(&path, "x x\n").unwrap();
        let args = ::std::format!("{}|x|y", path.to_str().unwrap());
        let mut out = ObsBuf::new();
        assert_eq!(
            EditFile.call(&args, &mut out),
            Err(ToolError::InvalidArgument("needle is ambiguous"))
        );
    }

    #[test]
    fn list_dir_shows_entries() {
        let path = scratch("keep.txt");
        std::fs::write(&path, "x").unwrap();
        let mut out = ObsBuf::new();
        let dir = path.parent().unwrap().to_str().unwrap();
        assert_eq!(ListDir.call(dir, &mut out).unwrap(), ObsKind::Text);
        assert!(out.as_str().contains("keep.txt"));
    }
}
