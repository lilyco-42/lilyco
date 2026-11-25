//! The smallest useful tool set: read, list, and exact-match edit.
//!
//! Built-in file operations resolve relative paths through a workspace handle.
//! External subprocesses run under the host OS permissions.

use ::std::path::{Path, PathBuf};
use ::std::string::String;
use cap_std::fs::{Dir, DirEntry};

use crate::adapters::fs_engine::{plan_replace, render_numbered, sanitize_rel, split_at, Window};
use crate::adapters::workspace;
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
        let root = open_workspace(&self.root)?;
        let path = resolve(args)?;
        let file = workspace::open_regular(&root, path).map_err(file_error)?;
        let src = workspace::bounded_text(&file).map_err(file_error)?;
        render_numbered(&src, Window::plan(120, 0, 120), out)
            .map_err(|_| ToolError::ObservationTooLarge)?;
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
        let root = open_workspace(&self.root)?;
        let entries = root
            .read_dir(resolve(args)?)
            .map_err(|_| ToolError::HostFailure("cannot list directory"))?;
        for entry in entries {
            let entry = entry.map_err(|_| ToolError::HostFailure("cannot read directory entry"))?;
            push_line(out, &entry.file_name().to_string_lossy())?;
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
        ToolSpec::new(
            "edit_file",
            "replace one exact occurrence: path|old|new",
            true,
        )
    }

    fn call(&self, args: &str, _out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let (path, old, new) = split_args(args)?;
        let root = open_workspace(&self.root)?;
        let path = resolve(path)?;
        let file = workspace::open_regular(&root, path).map_err(file_error)?;
        let src = workspace::bounded_text(&file).map_err(file_error)?;
        let span = plan_replace(&src, old).map_err(ToolError::InvalidArgument)?;
        let (head, tail) = split_at(&src, span).map_err(ToolError::InvalidArgument)?;
        let mut next = String::with_capacity(head.len() + new.len() + tail.len());
        next.push_str(head);
        next.push_str(new);
        next.push_str(tail);
        let permissions = file.metadata().map_err(file_error)?.permissions();
        drop(file);
        workspace::replace(&root, path, next.as_bytes(), permissions)
            .map_err(|_| ToolError::HostFailure("cannot atomically replace file"))?;
        Ok(ObsKind::Diff)
    }
}

fn open_workspace(path: &Path) -> Result<Dir, ToolError> {
    workspace::root(path).map_err(|_| ToolError::HostFailure("cannot open workspace"))
}

fn resolve(args: &str) -> Result<&Path, ToolError> {
    let path = args.trim();
    sanitize_rel(if path.is_empty() { "." } else { path })
        .map(Path::new)
        .map_err(ToolError::NotPermitted)
}

fn file_error(error: std::io::Error) -> ToolError {
    match error.kind() {
        std::io::ErrorKind::InvalidData => {
            ToolError::InvalidArgument("file is too large or is not UTF-8")
        }
        _ => ToolError::HostFailure("cannot read regular workspace file"),
    }
}

/// Parse the same edit argument used by the tool before taking a checkpoint.
pub fn edit_path(args: &str) -> Result<&str, ToolError> {
    split_args(args).map(|(path, _, _)| path)
}

fn split_args(args: &str) -> Result<(&str, &str, &str), ToolError> {
    let mut parts = args.splitn(3, '|');
    let path = parts
        .next()
        .ok_or(ToolError::InvalidArgument("missing path"))?;
    let old = parts
        .next()
        .ok_or(ToolError::InvalidArgument("missing old text"))?;
    let new = parts
        .next()
        .ok_or(ToolError::InvalidArgument("missing new text"))?;
    if old.is_empty() {
        return Err(ToolError::InvalidArgument("old text is empty"));
    }
    Ok((path.trim(), old, new))
}

/// Commands the agent may run. Anything else is refused by name.
pub const ALLOWED_COMMANDS: &[&str] = &[
    "git", "cargo", "rustc", "ls", "dir", "cat", "rg", "grep", "find", "echo", "python", "python3",
    "node", "type", "wc", "head", "ast-grep", "sg",
];

/// Run a host command with the workspace as its working directory.
///
/// The working directory and executable allow-list are convenience controls.
/// Hosts that register this tool must supply their own process isolation and
/// approval policy.
pub struct RunCommand {
    root: PathBuf,
}

impl RunCommand {
    /// Sandbox the tool to `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        RunCommand { root: root.into() }
    }
}

impl Tool for RunCommand {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new("run_command", "run an allow-listed command", true)
    }

    fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let mut parts = args.split_whitespace();
        let program = parts
            .next()
            .ok_or(ToolError::InvalidArgument("empty command"))?;
        if !ALLOWED_COMMANDS.contains(&program) {
            return Err(ToolError::NotPermitted("command is not allow-listed"));
        }
        let output = std::process::Command::new(program)
            .args(parts)
            .current_dir(&self.root)
            .output()
            .map_err(|_| ToolError::HostFailure("cannot spawn command"))?;
        let code = output.status.code().unwrap_or(-1);
        for line in ::std::string::String::from_utf8_lossy(&output.stdout)
            .lines()
            .take(40)
        {
            push_line(out, line)?;
        }
        for line in ::std::string::String::from_utf8_lossy(&output.stderr)
            .lines()
            .take(20)
        {
            push_line(out, line)?;
        }
        Ok(ObsKind::Exit { code })
    }
}

/// Substring search across the workspace.
pub struct Grep {
    root: PathBuf,
}

impl Grep {
    /// Sandbox the tool to `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Grep { root: root.into() }
    }
}

impl Tool for Grep {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new("grep", "find a substring in workspace files", false)
    }

    fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let needle = args.trim();
        if needle.is_empty() {
            return Err(ToolError::InvalidArgument("empty pattern"));
        }
        let root = open_workspace(&self.root)?;
        let mut hits = 0;
        collect(&root, Path::new(""), 0, &mut |path| {
            let Ok(file) = workspace::open_regular(&root, path) else {
                return Ok(true);
            };
            let Ok(text) = workspace::bounded_text(&file) else {
                return Ok(true);
            };
            scan(path, &text, needle, out, &mut hits)?;
            Ok(hits < 40)
        })?;
        Ok(ObsKind::Text)
    }
}

/// List workspace paths whose name ends with the given suffix.
pub struct Glob {
    root: PathBuf,
}

impl Glob {
    /// Sandbox the tool to `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Glob { root: root.into() }
    }
}

impl Tool for Glob {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new("glob", "list files whose name ends with a suffix", false)
    }

    fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let suffix = args.trim();
        if suffix.is_empty() {
            return Err(ToolError::InvalidArgument("empty suffix"));
        }
        let root = open_workspace(&self.root)?;
        let mut seen = 0;
        collect(&root, Path::new(""), 0, &mut |path| {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(suffix))
            {
                push_line(out, &path.to_string_lossy())?;
                seen += 1;
            }
            Ok(seen < 40)
        })?;
        Ok(ObsKind::Text)
    }
}

fn scan(
    path: &Path,
    text: &str,
    needle: &str,
    out: &mut ObsBuf,
    hits: &mut usize,
) -> Result<(), ToolError> {
    use core::fmt::Write as _;
    for (n, line) in text.lines().enumerate() {
        if !line.contains(needle) {
            continue;
        }
        writeln!(out, "{}:{}: {}", path.display(), n + 1, line.trim())
            .map_err(|_| ToolError::ObservationTooLarge)?;
        *hits += 1;
        if *hits >= 40 {
            break;
        }
    }
    Ok(())
}

/// Structural search, delegated to ast-grep.
///
/// The engine is *not* reimplemented here: the tool shells out to the
/// `ast-grep` binary (bundled next to the app, or found on PATH). Arguments are
/// `lang|pattern`, so no shell quoting is involved.
pub struct AstGrep {
    root: PathBuf,
    bin: PathBuf,
}

impl AstGrep {
    /// `bin` is the ast-grep executable to use.
    pub fn new(root: impl Into<PathBuf>, bin: impl Into<PathBuf>) -> Self {
        AstGrep {
            root: root.into(),
            bin: bin.into(),
        }
    }

    /// Resolve the executable: explicit env var, then the vendor folder next to
    /// the current executable, then PATH.
    pub fn locate() -> PathBuf {
        if let Ok(explicit) = std::env::var("AST_GREP_BIN") {
            return PathBuf::from(explicit);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let candidate = dir.join("vendor").join("ast-grep.exe");
                if candidate.exists() {
                    return candidate;
                }
            }
        }
        PathBuf::from("ast-grep")
    }
}

impl Tool for AstGrep {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new("ast_grep", "structural search: lang|pattern", false)
    }

    fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let (lang, pattern) = args
            .split_once('|')
            .ok_or(ToolError::InvalidArgument("expected lang|pattern"))?;
        if pattern.trim().is_empty() {
            return Err(ToolError::InvalidArgument("empty pattern"));
        }
        let output = std::process::Command::new(&self.bin)
            .arg("run")
            .arg("-p")
            .arg(pattern)
            .arg("-l")
            .arg(lang.trim())
            .arg(".")
            .current_dir(&self.root)
            .output()
            .map_err(|_| ToolError::HostFailure("cannot spawn ast-grep"))?;
        let code = output.status.code().unwrap_or(-1);
        for line in ::std::string::String::from_utf8_lossy(&output.stdout)
            .lines()
            .take(60)
        {
            push_line(out, line)?;
        }
        if code != 0 && out.as_str().is_empty() {
            for line in ::std::string::String::from_utf8_lossy(&output.stderr)
                .lines()
                .take(10)
            {
                push_line(out, line)?;
            }
        }
        Ok(ObsKind::Exit { code })
    }
}

fn push_line(out: &mut ObsBuf, text: &str) -> Result<(), ToolError> {
    out.push_str(text)
        .map_err(|_| ToolError::ObservationTooLarge)?;
    out.push('\n').map_err(|_| ToolError::ObservationTooLarge)
}

/// Skip hidden directories and links. Propagate the stop signal through every directory.
fn collect(
    dir: &Dir,
    prefix: &Path,
    depth: usize,
    visit: &mut impl FnMut(&Path) -> Result<bool, ToolError>,
) -> Result<bool, ToolError> {
    let entries = dir
        .entries()
        .map_err(|_| ToolError::HostFailure("cannot search directory"))?;
    for entry in entries.flatten() {
        if !visit_entry(prefix, depth, &entry, visit)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn visit_entry(
    prefix: &Path,
    depth: usize,
    entry: &DirEntry,
    visit: &mut impl FnMut(&Path) -> Result<bool, ToolError>,
) -> Result<bool, ToolError> {
    let name = entry.file_name();
    let shown = name.to_string_lossy();
    if shown.starts_with('.') || shown == "target" || shown == "node_modules" {
        return Ok(true);
    }
    let path = prefix.join(name);
    let Ok(kind) = entry.file_type() else {
        return Ok(true);
    };
    if kind.is_file() {
        return visit(&path);
    }
    if kind.is_dir() && depth < 3 {
        if let Ok(child) = entry.open_dir() {
            return collect(&child, &path, depth + 1, visit);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(::std::format!("lilyco_no_std_{tag}"));
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

    #[cfg(unix)]
    #[test]
    fn file_capabilities_do_not_follow_links_out_of_a_workspace() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let outside = other.path().join("outside.txt");
        std::fs::write(&outside, "private").unwrap();
        symlink(other.path(), root.path().join("linked")).unwrap();
        symlink(&outside, root.path().join("shortcut")).unwrap();

        let mut out = ObsBuf::new();
        for arg in ["linked/outside.txt", "shortcut"] {
            assert!(ReadFile::new(root.path()).call(arg, &mut out).is_err());
            let edit = std::format!("{arg}|private|changed");
            assert!(EditFile::new(root.path()).call(&edit, &mut out).is_err());
        }
        assert!(ListDir::new(root.path()).call("linked", &mut out).is_err());
        Grep::new(root.path()).call("private", &mut out).unwrap();
        assert!(out.is_empty());
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "private");
    }

    #[test]
    fn read_and_edit_refuse_large_files_before_modification() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("large.txt");
        let body = "x".repeat(256 * 1024 + 1);
        std::fs::write(&path, &body).unwrap();
        let mut out = ObsBuf::new();
        assert!(ReadFile::new(root.path())
            .call("large.txt", &mut out)
            .is_err());
        assert!(EditFile::new(root.path())
            .call("large.txt|x|y", &mut out)
            .is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), body);
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
        assert_eq!(
            ListDir::new(&dir).call("", &mut out).unwrap(),
            ObsKind::Text
        );
        assert!(out.as_str().contains("keep.txt"));
    }
}
