use std::{
    fs,
    path::{Path, PathBuf},
};

use include_dir::{Dir, include_dir};

use crate::{Gateway, NewProject, Preset, Result, database::StorageChoice, frontend, slug};

const SKELETON: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates/skeleton");
const WORKSPACE: &str = include_str!("../templates/workspace-Cargo.toml");

macro_rules! contents {
    ($path:literal) => {
        include_bytes!(concat!("../", $path))
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
    frontend::validate(
        project.frontend,
        matches!(project.preset, Preset::Fullstack),
    )?;
    if !slug(name) {
        return Err(
            "project name must be lowercase, with up to 48 letters, digits or hyphens".into(),
        );
    }
    let path = project.dir.join(name);
    create(&path, || populate(&path, project, storage, gateway))?;
    Ok(path)
}

pub fn preview_frontend(project: &Path, options: frontend::Options) -> Result<PathBuf> {
    if !project.join("Rapid.toml").is_file() {
        return Err("project directory must contain Rapid.toml".into());
    }
    frontend::validate(options, true)?;
    let path = project.join("web-next");
    create(&path, || frontend::write(&path, options))?;
    Ok(path)
}

fn create(path: &Path, populate: impl FnOnce() -> Result<()>) -> Result<()> {
    fs::create_dir(path).map_err(|error| format!("create {}: {error}", path.display()))?;
    if let Err(error) = populate() {
        if let Err(cleanup) = fs::remove_dir_all(path) {
            eprintln!(
                "could not remove incomplete directory {}: {cleanup}",
                path.display()
            );
        }
        return Err(error);
    }
    Ok(())
}

fn populate(
    path: &Path,
    project: &NewProject,
    storage: StorageChoice,
    gateway: Gateway,
) -> Result<()> {
    put(path, "Cargo.toml", WORKSPACE.as_bytes())?;
    let settings = include_str!("../Rapid.toml").replace("Lilyco Rapid", &project.name);
    put(path, "Rapid.toml", storage.config(&settings).as_bytes())?;
    put(path, ".gitignore", contents!(".gitignore"))?;
    put(path, ".dockerignore", contents!(".dockerignore"))?;
    put(path, "LICENSE", include_bytes!("../templates/LICENSE"))?;
    put(path, "README.md", contents!("ops/PROJECT.md"))?;
    put(path, "ops/Dockerfile.api", contents!("ops/Dockerfile.api"))?;
    extract(&SKELETON, path)?;
    storage.write(path)?;

    match (project.preset, gateway) {
        (Preset::Api, Gateway::Direct) => {
            put(path, "compose.yaml", contents!("ops/compose.api.yml"))
        }
        (Preset::Api, Gateway::Caddy | Gateway::Pingap) => write_api_gateway(path, gateway),
        (Preset::Fullstack, Gateway::Caddy | Gateway::Pingap) => write_web(path, gateway, project),
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

fn write_web(path: &Path, gateway: Gateway, project: &NewProject) -> Result<()> {
    put(path, "compose.yaml", contents!("ops/compose.fullstack.yml"))?;
    let runtime = match gateway {
        Gateway::Caddy => {
            put(path, "ops/Caddyfile", contents!("ops/Caddyfile"))?;
            include_str!("../ops/caddy-runtime.Dockerfile")
        }
        Gateway::Pingap => {
            put(path, "ops/Pingap.toml", contents!("ops/pingap-web.toml"))?;
            include_str!("../ops/pingap-runtime.Dockerfile")
        }
        Gateway::Direct => return Err("a proxy gateway is required".into()),
    };
    let dockerfile = [include_str!("../ops/Dockerfile.web"), runtime].join("\n");
    put(path, "ops/Dockerfile.web", dockerfile.as_bytes())?;
    frontend::write(&path.join("web"), project.frontend)
}

pub(super) fn put(path: &Path, relative: &str, bytes: &[u8]) -> Result<()> {
    let file = path.join(relative);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(file, bytes)?;
    Ok(())
}

pub(super) fn extract(template: &Dir<'_>, path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    template.extract(path)?;
    Ok(())
}