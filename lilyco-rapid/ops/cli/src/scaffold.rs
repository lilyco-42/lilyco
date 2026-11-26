use std::{
    fs,
    path::{Path, PathBuf},
};

use include_dir::{Dir, include_dir};

use crate::{Gateway, NewProject, Preset, Result, database::StorageChoice, slug};

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

pub fn new(project: &NewProject) -> Result<PathBuf> {
    let name = &project.name;
    let storage = StorageChoice {
        database: project.db,
        driver: project.db_driver,
    };
    storage.check()?;
    let gateway = gateway(project)?;
    if !slug(name) {
        return Err(
            "project name must be lowercase, with up to 48 letters, digits or hyphens".into(),
        );
    }
    let path = project.dir.join(name);
    fs::create_dir(&path).map_err(|error| format!("create {}: {error}", path.display()))?;
    if let Err(error) = populate(&path, project, storage, gateway) {
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

fn populate(
    path: &Path,
    project: &NewProject,
    storage: StorageChoice,
    gateway: Gateway,
) -> Result<()> {
    let workspace = WORKSPACE.replace(
        "path = \"../lilyco-no_std\"",
        "path = \"ops/lilyco-no_std\"",
    );
    put(path, "Cargo.toml", workspace.as_bytes())?;
    put(path, "Cargo.lock", contents!("Cargo.lock"))?;
    let settings = include_str!("../../../Rapid.toml").replace("Lilyco Rapid", &project.name);
    put(path, "Rapid.toml", storage.config(&settings).as_bytes())?;
    put(path, ".gitignore", contents!(".gitignore"))?;
    put(path, ".dockerignore", contents!(".dockerignore"))?;
    put(path, "LICENSE", include_bytes!("../../../../LICENSE"))?;
    put(path, "README.md", contents!("ops/PROJECT.md"))?;
    put(path, "ops/Dockerfile.api", contents!("ops/Dockerfile.api"))?;
    extract(&CORE, &path.join("core"))?;
    extract(&SERVER, &path.join("server"))?;
    storage.write(path)?;
    extract(&LIB_SRC, &path.join("ops/lilyco-no_std/src"))?;
    let manifest = include_str!("../../../../lilyco-no_std/Cargo.toml");
    put(path, "ops/lilyco-no_std/Cargo.toml", manifest.as_bytes())?;

    match (project.preset, gateway) {
        (Preset::Api, Gateway::Direct) => {
            put(path, "compose.yaml", contents!("ops/compose.api.yml"))
        }
        (Preset::Api, Gateway::Caddy | Gateway::Pingap) => write_api_gateway(path, gateway),
        (Preset::Fullstack, Gateway::Caddy | Gateway::Pingap) => write_web(path, gateway),
        _ => Err("invalid gateway for project preset".into()),
    }
}

fn gateway(project: &NewProject) -> Result<Gateway> {
    match (project.preset, project.gateway) {
        (Preset::Api, None | Some(Gateway::Direct)) => Ok(Gateway::Direct),
        (Preset::Fullstack, None) | (_, Some(Gateway::Caddy)) => Ok(Gateway::Caddy),
        (_, Some(Gateway::Pingap)) => Ok(Gateway::Pingap),
        (Preset::Fullstack, Some(Gateway::Direct)) => {
            Err("fullstack requires Caddy or Pingap to serve the web build".into())
        }
    }
}

fn write_api_gateway(path: &Path, gateway: Gateway) -> Result<()> {
    let recipe = match gateway {
        Gateway::Caddy => {
            put(path, "ops/Caddyfile", contents!("ops/caddy-api.Caddyfile"))?;
            contents!("ops/compose.api-caddy.yml").as_slice()
        }
        Gateway::Pingap => {
            put(path, "ops/Pingap.toml", contents!("ops/pingap-api.toml"))?;
            contents!("ops/compose.api-pingap.yml").as_slice()
        }
        Gateway::Direct => return Err("a proxy gateway is required".into()),
    };
    put(path, "compose.yaml", recipe)
}

fn write_web(path: &Path, gateway: Gateway) -> Result<()> {
    put(path, "compose.yaml", contents!("ops/compose.fullstack.yml"))?;
    let runtime = match gateway {
        Gateway::Caddy => {
            put(path, "ops/Caddyfile", contents!("ops/Caddyfile"))?;
            include_str!("../../../ops/caddy-runtime.Dockerfile")
        }
        Gateway::Pingap => {
            put(path, "ops/Pingap.toml", contents!("ops/pingap-web.toml"))?;
            include_str!("../../../ops/pingap-runtime.Dockerfile")
        }
        Gateway::Direct => return Err("a proxy gateway is required".into()),
    };
    let dockerfile = [include_str!("../../../ops/Dockerfile.web"), runtime].join("\n");
    put(path, "ops/Dockerfile.web", dockerfile.as_bytes())?;
    put(path, "web/package.json", contents!("web/package.json"))?;
    put(path, "web/pnpm-lock.yaml", contents!("web/pnpm-lock.yaml"))?;
    put(
        path,
        "web/pnpm-workspace.yaml",
        contents!("web/pnpm-workspace.yaml"),
    )?;
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
