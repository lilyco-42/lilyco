//! Capability-relative filesystem operations for the optional host adapters.

#[cfg(unix)]
use cap_fs_ext::OpenOptionsSyncExt;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, OpenOptions, Permissions},
};
use std::{
    io::{self, Read, Write},
    path::Path,
    string::String,
};

use super::fs_engine::sanitize_rel;

// Bound host memory per text file. The search adapter already used this limit.
const TEXT_FILE_BYTES: u64 = 256 * 1024;

pub(crate) fn root(path: &Path) -> io::Result<Dir> {
    Dir::open_ambient_dir(path, ambient_authority())
}

pub(crate) fn relative(input: &str) -> io::Result<&Path> {
    sanitize_rel(input)
        .map(Path::new)
        .map_err(|reason| io::Error::new(io::ErrorKind::InvalidInput, reason))
}

pub(crate) fn open_regular(dir: &Dir, path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.nonblock(true);
    let file = dir.open_with(path, &options)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "regular file required",
        ));
    }
    Ok(file)
}

pub(crate) fn bounded_text(file: &File) -> io::Result<String> {
    let mut text = String::new();
    file.take(TEXT_FILE_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > TEXT_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "text file exceeds size limit",
        ));
    }
    Ok(text)
}

pub(crate) fn replace(
    root: &Dir,
    path: &Path,
    bytes: &[u8],
    permissions: Permissions,
) -> io::Result<()> {
    if bytes.len() as u64 > TEXT_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "edited file exceeds size limit",
        ));
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file name required"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let dir = root.open_dir(parent)?;
    let mut file = cap_tempfile::TempFile::new(&dir)?;
    file.as_file().set_permissions(permissions)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.replace(name)
}

pub(crate) fn copy_snapshot(source: File, dir: &Dir, target: &Path) -> io::Result<()> {
    let content = bounded_text(&source)?;
    replace(
        dir,
        target,
        content.as_bytes(),
        source.metadata()?.permissions(),
    )
}
