use std::{error::Error, path::PathBuf, process::ExitCode};

use std::process::Command as Process;

use clap::{Parser, Subcommand, ValueEnum};

mod database;
mod frontend;
mod resource;
mod run;
mod runtime;
mod scaffold;

use runtime::ContainerRuntime;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a standalone project with a typed resources directory and a deployment recipe.
    New(NewProject),
    /// Generate a typed resource or preview frontend.
    Generate {
        #[command(subcommand)]
        kind: Generate,
    },
    /// Export records or import them into an empty target database.
    Data {
        #[command(subcommand)]
        action: Data,
        #[arg(long, default_value = ".", global = true)]
        dir: PathBuf,
        #[arg(long, global = true)]
        config: Option<PathBuf>,
        /// Choose the Compose runtime for a generated local database.
        #[arg(
            long,
            env = "RAPID_CONTAINER_RUNTIME",
            value_enum,
            default_value_t,
            global = true
        )]
        runtime: ContainerRuntime,
    },
    /// Start the API and, when present, the Vite development server.
    Dev {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        /// Skip the frontend and its dependency installation.
        #[arg(long)]
        api_only: bool,
        /// Run the frontend in web-next against the existing API and database.
        #[arg(long, conflicts_with = "api_only")]
        preview_ui: bool,
        /// Choose the Compose runtime for a generated local database.
        #[arg(long, env = "RAPID_CONTAINER_RUNTIME", value_enum, default_value_t)]
        runtime: ContainerRuntime,
    },
}

// CLI DTO groups the selectable project components.
#[derive(clap::Args)]
struct NewProject {
    name: String,
    #[arg(long, value_enum, default_value_t = Preset::Fullstack)]
    preset: Preset,
    #[arg(long, value_enum, default_value_t = database::Database::Sqlite)]
    db: database::Database,
    #[arg(long, value_enum, default_value_t = database::Driver::Auto)]
    db_driver: database::Driver,
    #[arg(long, value_enum)]
    gateway: Option<Gateway>,
    #[command(flatten)]
    frontend: frontend::Options,
    #[arg(long, default_value = ".")]
    dir: PathBuf,
    /// Include login/session/RBAC routes in the generated server.
    #[arg(long)]
    auth: bool,
}

#[derive(Subcommand)]
enum Generate {
    /// Generate a preview frontend in web-next beside the current frontend.
    Frontend {
        #[command(flatten)]
        options: frontend::Options,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    Resource {
        collection: String,
        /// Repeat --field for each field. Supported types: string, bool, i32.
        #[arg(long, value_name = "NAME:TYPE", required = true)]
        field: Vec<String>,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum Data {
    Export { path: PathBuf },
    Import { path: PathBuf },
}

#[derive(Clone, Copy, ValueEnum)]
enum Preset {
    Api,
    Fullstack,
}

#[derive(Clone, Copy, ValueEnum)]
enum Gateway {
    Direct,
    Caddy,
    Pingap,
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lilyco-rapid: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        Command::New(project) => {
            let created = scaffold::new(&project)?;
            println!("Created {}", created.display());
            println!("Start with: lilyco-rapid dev --dir {:?}", created);
        }
        Command::Generate {
            kind:
                Generate::Resource {
                    collection,
                    field,
                    dir,
                },
        } => {
            let file = resource::write(&dir, &collection, &field)?;
            println!("Created {} → /api/{collection}", file.display());
        }
        Command::Generate {
            kind: Generate::Frontend { options, dir },
        } => {
            let created = scaffold::preview_frontend(&dir, options)?;
            println!("Created {}", created.display());
            println!("Preview with: lilyco-rapid dev --preview-ui --dir {dir:?}");
        }
        Command::Dev {
            dir,
            api_only,
            preview_ui,
            runtime,
        } => {
            let web = match (api_only, preview_ui) {
                (true, _) => run::WebMode::Off,
                (_, true) => run::WebMode::Preview,
                _ => run::WebMode::Project,
            };
            run::dev(&dir, web, runtime)?;
        }
        Command::Data {
            action,
            dir,
            config,
            runtime,
        } => transfer(action, &dir, config, runtime)?,
    }
    Ok(())
}

fn transfer(
    action: Data,
    dir: &std::path::Path,
    config: Option<PathBuf>,
    runtime: ContainerRuntime,
) -> Result<()> {
    let mut command = Process::new("cargo");
    command
        .current_dir(dir)
        .args(["run", "--locked", "-p", "rapid-server", "--"]);
    if let Some(config) = config {
        command.arg("--config").arg(config);
    } else if let Some(connection) = database::dev_connection(dir, runtime)? {
        connection.apply(&mut command);
    }
    match action {
        Data::Export { path } => command.arg("export").arg(path),
        Data::Import { path } => command.arg("import").arg(path),
    };
    run::checked(&mut command)
}

fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 48
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.ends_with('-')
        && !value.contains("--")
}
