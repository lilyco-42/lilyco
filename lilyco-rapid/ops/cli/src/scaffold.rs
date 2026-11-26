use std::{
    fs,
    path::{Path, PathBuf},
};

use include_dir::{Dir, include_dir};

use crate::{Preset, Result, slug};

const CORE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../core");
const SERVER: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../server");
const WEB_SRC: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web/src");
const LIB_SRC: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../../lilyco-no_std/src");
const WORKSPACE: &str = include_str!("../../../Cargo.toml");

macro_rules! contents {
    ($path:literal) => {
        include_bytes!(concat!("../../../", $path))
    };
}

pub fn new(parent: &Path, name: &str, preset: Preset) -> Result<PathBuf> {
    if !slug(name) {
        return Err(
            "project name must be lowercase, with up to 48 letters, digits or hyphens".into(),
        );
    }
    let path = parent.join(name);
    fs::create_dir(&path).map_err(|error| format!("create {}: {error}", path.display()))?;
    if let Err(error) = populate(&path, name, preset) {
        if let Err(cleanup) = fs::remove_dir_all(&path) {
            eprintln!(
                "could not remove incomplete project {}: {cleanup}",
                path.display()
            );
        }
        return Err(error);
    }
    Ok(path)
}

fn populate(path: &Path, name: &str, preset: Preset) -> Result<()> {
    let workspace = WORKSPACE.replace(
        "path = \"../lilyco-no_std\"",
        "path = \"ops/lilyco-no_std\"",
    );
    put(path, "Cargo.toml", workspace.as_bytes())?;
    put(path, "Cargo.lock", contents!("Cargo.lock"))?;
    let settings = include_str!("../../../Rapid.toml").replace("Lilyco Rapid", name);
    put(path, "Rapid.toml", settings.as_bytes())?;
    put(path, ".gitignore", contents!(".gitignore"))?;
    put(path, ".dockerignore", contents!(".dockerignore"))?;
    put(path, "LICENSE", include_bytes!("../../../../LICENSE"))?;
    put(path, "README.md", contents!("ops/PROJECT.md"))?;
    put(path, "ops/Dockerfile.api", contents!("ops/Dockerfile.api"))?;
    extract(&CORE, &path.join("core"))?;
    extract(&SERVER, &path.join("server"))?;
    extract(&LIB_SRC, &path.join("ops/lilyco-no_std/src"))?;
    let manifest = include_str!("../../../../lilyco-no_std/Cargo.toml");
    put(path, "ops/lilyco-no_std/Cargo.toml", manifest.as_bytes())?;

    match preset {
        Preset::Api => put(path, "compose.yaml", contents!("ops/compose.api.yml"))?,
        Preset::Fullstack => write_web(path)?,
    }
    Ok(())
}

fn write_web(path: &Path) -> Result<()> {
    put(path, "compose.yaml", contents!("ops/compose.fullstack.yml"))?;
    put(path, "ops/Dockerfile.web", contents!("ops/Dockerfile.web"))?;
    put(path, "ops/Caddyfile", contents!("ops/Caddyfile"))?;
    put(path, "web/package.json", contents!("web/package.json"))?;
    put(path, "web/pnpm-lock.yaml", contents!("web/pnpm-lock.yaml"))?;
    put(path, "web/tsconfig.json", contents!("web/tsconfig.json"))?;
    put(path, "web/vite.config.ts", contents!("web/vite.config.ts"))?;
    put(path, "web/index.html", contents!("web/index.html"))?;
    extract(&WEB_SRC, &path.join("web/src"))?;
    Ok(())
}

fn put(path: &Path, relative: &str, bytes: &[u8]) -> Result<()> {
    let file = path.join(relative);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(file, bytes)?;
    Ok(())
}

fn extract(template: &Dir<'_>, path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    template.extract(path)?;
    Ok(())
}
