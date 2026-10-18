//! File snapshots: the thing that makes "let the model edit my code" survivable.
//!
//! Claude Code rewinds by keeping a shadow repository. We do the minimum that
//! buys the same safety without a git dependency: before a mutating call, copy
//! the target file under `.tiny-agent/snap/<seq>/<path>`; undo copies it back.
//!
//! The layout mirrors the workspace, so a restore needs no index.

use std::path::{Path, PathBuf};
use std::string::ToString;

/// Snapshot area name, kept out of the way but easy to find and delete.
pub const SNAP_DIR: &str = ".tiny-agent";

/// One snapshot store rooted at a workspace.
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Bind to a workspace root.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    /// Where a snapshot for `seq` lives on disk.
    pub fn slot(&self, seq: u32) -> PathBuf {
        self.root.join(SNAP_DIR).join("snap").join(seq.to_string())
    }

    /// Copy `rel` (a workspace-relative path) into slot `seq`.
    ///
    /// Returns `Ok(false)` when the file does not exist yet (a create), which is
    /// a legitimate case: undo then means "delete what was created".
    pub fn capture(&self, rel: &str, seq: u32) -> Result<bool, &'static str> {
        let source = self.root.join(rel);
        if !source.exists() {
            return Ok(false);
        }
        let target = self.slot(seq).join(rel);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|_| "cannot create snapshot dir")?;
        }
        std::fs::copy(&source, &target).map_err(|_| "cannot copy into snapshot")?;
        Ok(true)
    }

    /// Restore `rel` from slot `seq`; when the capture recorded a create, the
    /// file did not exist before, so restoring removes it.
    pub fn restore(&self, rel: &str, seq: u32) -> Result<(), &'static str> {
        let saved = self.slot(seq).join(rel);
        let target = self.root.join(rel);
        if !saved.exists() {
            if target.exists() {
                std::fs::remove_file(&target).map_err(|_| "cannot remove created file")?;
            }
            return Ok(());
        }
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|_| "cannot recreate parent")?;
        }
        std::fs::copy(&saved, &target).map_err(|_| "cannot restore snapshot")?;
        Ok(())
    }

    /// Workspace root this store belongs to.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tiny_lilyco_snap_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trip_restores_the_original_bytes() {
        let root = scratch("roundtrip");
        std::fs::write(root.join("a.txt"), "before").unwrap();
        let store = Store::new(&root);
        assert_eq!(store.capture("a.txt", 1), Ok(true));
        std::fs::write(root.join("a.txt"), "after").unwrap();
        assert_eq!(store.restore("a.txt", 1), Ok(()));
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "before");
    }

    #[test]
    fn restoring_a_created_file_removes_it() {
        let root = scratch("created");
        let store = Store::new(&root);
        assert_eq!(store.capture("new.txt", 2), Ok(false));
        std::fs::write(root.join("new.txt"), "made").unwrap();
        assert_eq!(store.restore("new.txt", 2), Ok(()));
        assert!(!root.join("new.txt").exists());
    }

    #[test]
    fn nested_paths_are_mirrored() {
        let root = scratch("nested");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "x").unwrap();
        let store = Store::new(&root);
        store.capture("src/lib.rs", 3).unwrap();
        assert!(store.slot(3).join("src/lib.rs").exists());
    }

    #[test]
    fn missing_snapshot_is_not_an_error() {
        let root = scratch("missing");
        let store = Store::new(&root);
        assert_eq!(store.restore("never.txt", 9), Ok(()));
    }
}
