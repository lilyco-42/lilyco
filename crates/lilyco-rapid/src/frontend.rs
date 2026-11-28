use std::{fs, path::Path};

use clap::ValueEnum;
use include_dir::{Dir, include_dir};

use crate::{
    Result,
    scaffold::{extract, put},
};

const REACT_SRC: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web/src");
const VUE_SRC: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ops/ui/vue/src");
const SVELTE_SRC: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ops/ui/svelte/src");

macro_rules! web {
    ($path:literal) => {
        include_bytes!(concat!("../web/", $path))
    };
}

macro_rules! spa_files {
    ($framework:literal) => {
        [
            include_bytes!(concat!("../ops/ui/", $framework, "/package.json")),
            include_bytes!(concat!("../ops/ui/", $framework, "/pnpm-lock.yaml")),
            include_bytes!(concat!("../ops/ui/", $framework, "/tsconfig.json")),
            include_bytes!(concat!("../ops/ui/", $framework, "/vite.config.ts")),
        ]
    };
}

#[derive(Clone, Copy, clap::Args)]
pub struct Options {
    #[arg(long, value_enum)]
    ui: Option<Framework>,
    #[arg(long, value_enum)]
    ui_kit: Option<UiKit>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Framework {
    React,
    Vue,
    Svelte,
}

#[derive(Clone, Copy, ValueEnum)]
enum UiKit {
    Shadcn,
    Native,
    Antd,
}

pub fn validate(options: Options, supports_web: bool) -> Result<()> {
    if !supports_web && (options.ui.is_some() || options.ui_kit.is_some()) {
        return Err("--ui and --ui-kit require --preset fullstack".into());
    }
    if matches!(
        (options.ui, options.ui_kit),
        (
            Some(Framework::Vue | Framework::Svelte),
            Some(UiKit::Shadcn)
        )
    ) {
        return Err("shadcn uses React in Rapid; choose --ui-kit native for Vue or Svelte".into());
    }
    if matches!(options.ui_kit, Some(UiKit::Antd))
        && !matches!(options.ui.unwrap_or(Framework::React), Framework::React)
    {
        return Err("antd uses React in Rapid; choose --ui react with --ui-kit antd".into());
    }
    Ok(())
}

pub fn write(path: &Path, options: Options) -> Result<()> {
    put(path, "pnpm-workspace.yaml", web!("pnpm-workspace.yaml"))?;
    put(path, "vite.rapid.ts", web!("vite.rapid.ts"))?;
    match options.ui.unwrap_or(Framework::React) {
        Framework::React => react(path, options.ui_kit.unwrap_or(UiKit::Shadcn)),
        Framework::Vue => write_spa(path, &VUE_SRC, spa_files!("vue")),
        Framework::Svelte => {
            write_spa(path, &SVELTE_SRC, spa_files!("svelte"))?;
            put(
                path,
                "svelte.config.js",
                include_bytes!("../ops/ui/svelte/svelte.config.js"),
            )
        }
    }
}

fn react(path: &Path, kit: UiKit) -> Result<()> {
    put(path, "package.json", web!("package.json"))?;
    if !matches!(kit, UiKit::Antd) {
        put(path, "pnpm-lock.yaml", web!("pnpm-lock.yaml"))?;
    }
    put(path, "tsconfig.json", web!("tsconfig.json"))?;
    put(path, "vite.config.ts", web!("vite.config.ts"))?;
    put(path, "index.html", web!("index.html"))?;
    extract(&REACT_SRC, &path.join("src"))?;
    if matches!(kit, UiKit::Native) {
        put(
            path,
            "src/components/button.tsx",
            include_bytes!("../ops/ui/react-button.tsx"),
        )?;
    }
    if matches!(kit, UiKit::Antd) {
        for file in ["src/components/button.tsx", "src/components/Workbench.tsx"] {
            let file = path.join(file);
            if file.is_file() {
                fs::remove_file(file)?;
            }
        }
        put(
            path,
            "package.json",
            include_bytes!("../templates/web-antd/package.json"),
        )?;
        put(
            path,
            "src/App.tsx",
            include_bytes!("../templates/web-antd/App.tsx"),
        )?;
    }
    Ok(())
}

fn write_spa(path: &Path, source: &Dir<'_>, files: [&[u8]; 4]) -> Result<()> {
    let names = [
        "package.json",
        "pnpm-lock.yaml",
        "tsconfig.json",
        "vite.config.ts",
    ];
    for (name, bytes) in names.into_iter().zip(files) {
        put(path, name, bytes)?;
    }
    let index = include_str!("../web/index.html").replace("main.tsx", "main.ts");
    put(path, "index.html", index.as_bytes())?;
    extract(source, &path.join("src"))?;
    put(path, "src/lib/api.ts", web!("src/lib/api.ts"))?;
    put(path, "src/strings.ts", web!("src/strings.ts"))?;
    put(path, "src/strings.toml", web!("src/strings.toml"))?;
    put(
        path,
        "src/strings.zh-CN.toml",
        web!("src/strings.zh-CN.toml"),
    )?;
    put(path, "src/styles.css", web!("src/styles.css"))
}
