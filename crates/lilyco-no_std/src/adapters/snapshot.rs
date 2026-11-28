//! File snapshots: the thing that makes "let the model edit my code" survivable.
//!
//! Claude Code rewinds by keeping a shadow repository. We do the minimum that
//! buys the same safety without a git dependency: before a mutating call, copy
//! the target file under `.tiny-agent/snap/<seq>/<path>`; undo copies it back.
//!
//! The layout mirrors the workspace, so a restore needs no index.

use cap_std::fs::{Dir, OpenOptions};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    string::{String, ToString},
};

use super::workspace;

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
        self.root.join(snapshot_slot(seq))
    }

    /// Copy `rel` (a workspace-relative path) into slot `seq`.
    ///
    /// Returns `Ok(false)` when the file does not exist yet (a create), which is
    /// a legitimate case: undo then means "delete what was created".
    pub fn capture(&self, rel: &str, seq: u32) -> Result<bool, &'static str> {
        let root = workspace::root(&self.root).map_err(|_| "cannot open workspace")?;
        let rel = workspace::relative(rel).map_err(|_| "invalid snapshot path")?;
        let slot = reserve(&root, &snapshot_slot(seq))?;
        match workspace::open_regular(&root, rel) {
            Ok(file) => {
                prepare_parent(&slot, rel)?;
                workspace::copy_snapshot(file, &slot, rel)
                    .map_err(|_| "cannot save file snapshot")?;
                Ok(true)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // Keep creation markers outside the snapshot tree so filenames cannot collide.
                let marker = marker_path(seq, rel);
                prepare_parent(&root, &marker)?;
                root.open_with(marker, OpenOptions::new().write(true).create_new(true))
                    .and_then(|file| file.sync_all())
                    .map_err(|_| "cannot record that the file is absent")?;
                Ok(false)
            }
            Err(_) => Err("cannot open file to snapshot"),
        }
    }

    /// Restore the exact recorded snapshot. An absent file is removed only
    /// when capture previously recorded that absence.
    pub fn restore(&self, rel: &str, seq: u32) -> Result<(), &'static str> {
        let root = workspace::root(&self.root).map_err(|_| "cannot open workspace")?;
        let rel = workspace::relative(rel).map_err(|_| "invalid snapshot path")?;
        match workspace::open_regular(&root, &snapshot_slot(seq).join(rel)) {
            Ok(saved) => {
                prepare_parent(&root, rel)?;
                workspace::copy_snapshot(saved, &root, rel)
                    .map_err(|_| "cannot restore file snapshot")
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                remove_if_recorded(&root, rel, seq)
            }
            Err(_) => Err("cannot open saved snapshot"),
        }
    }

    fn reserve_next(&self, rel: &str, from: u32) -> Result<u32, &'static str> {
        let root = workspace::root(&self.root).map_err(|_| "cannot open workspace")?;
        let path = workspace::relative(rel).map_err(|_| "invalid snapshot path")?;
        let file =
            workspace::open_regular(&root, path).map_err(|_| "cannot open file to snapshot")?;
        let mut id = from;
        let base = snapshot_slot(0);
        prepare_parent(&root, &base)?;
        loop {
            if let Some(slot) = try_reserve(&root, &snapshot_slot(id))? {
                prepare_parent(&slot, path)?;
                workspace::copy_snapshot(file, &slot, path)
                    .map_err(|_| "cannot save file snapshot")?;
                return Ok(id);
            }
            id = id.checked_add(1).ok_or("snapshot sequence exhausted")?;
        }
    }

    /// Workspace root this store belongs to.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn snapshot_slot(id: u32) -> PathBuf {
    Path::new(SNAP_DIR).join("snap").join(id.to_string())
}

fn marker_path(id: u32, relative: &Path) -> PathBuf {
    Path::new(SNAP_DIR)
        .join("missing")
        .join(id.to_string())
        .join(relative)
}

fn prepare_parent(dir: &Dir, path: &Path) -> Result<(), &'static str> {
    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        dir.create_dir_all(parent)
            .map_err(|_| "cannot create snapshot or destination directory")?;
    }
    Ok(())
}

fn try_reserve(dir: &Dir, path: &Path) -> Result<Option<Dir>, &'static str> {
    match dir.create_dir(path) {
        Ok(()) => dir
            .open_dir(path)
            .map(Some)
            .map_err(|_| "cannot open new snapshot slot"),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(None),
        Err(_) => Err("cannot reserve snapshot slot"),
    }
}

fn reserve(dir: &Dir, path: &Path) -> Result<Dir, &'static str> {
    prepare_parent(dir, path)?;
    try_reserve(dir, path)?.ok_or("snapshot sequence already exists")
}

fn remove_if_recorded(root: &Dir, rel: &Path, seq: u32) -> Result<(), &'static str> {
    if workspace::open_regular(root, &marker_path(seq, rel)).is_err() {
        return match root.symlink_metadata(rel) {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            _ => Err("no snapshot for this file"),
        };
    }
    match root.remove_file(rel) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err("cannot remove created file"),
    }
}

/// Checkpoint helper shared by the application hosts that permit file edits.
pub struct EditJournal {
    store: Store,
    last: Option<(u32, String)>,
}

impl EditJournal {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            store: Store::new(root),
            last: None,
        }
    }

    pub fn before_edit(&mut self, args: &str) -> Result<(), &'static str> {
        let path = crate::agent::tools::edit_path(args).map_err(|error| error.message())?;
        let first = self.last.as_ref().map_or(1, |(id, _)| id.saturating_add(1));
        let id = self.store.reserve_next(path, first)?;
        self.last = Some((id, String::from(path)));
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), &'static str> {
        let (id, path) = self
            .last
            .as_ref()
            .ok_or("no completed checkpoint to restore")?;
        self.store.restore(path, *id)?;
        self.last = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(::std::format!("lilyco_no_std_snap_{tag}"));
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
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "before"
        );
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
    fn missing_backup_never_deletes_an_existing_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("existing.txt");
        std::fs::write(&path, "original").unwrap();
        assert!(Store::new(root.path()).restore("existing.txt", 20).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
    }

    #[test]
    fn independent_journals_preserve_separate_checkpoints() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        std::fs::write(&path, "one").unwrap();
        let mut first = EditJournal::new(root.path());
        first.before_edit("file.txt|one|two").unwrap();
        std::fs::write(&path, "two").unwrap();
        let mut second = EditJournal::new(root.path());
        second.before_edit("file.txt|two|three").unwrap();
        std::fs::write(&path, "three").unwrap();
        second.undo().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        first.undo().unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "one");
    }

    #[test]
    fn missing_snapshot_is_not_an_error() {
        let root = scratch("missing");
        let store = Store::new(&root);
        assert_eq!(store.restore("never.txt", 9), Ok(()));
    }
}
