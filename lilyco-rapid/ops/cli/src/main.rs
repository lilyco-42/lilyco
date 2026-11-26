use std::{error::Error, path::PathBuf, process::ExitCode};

use std::process::Command as Process;

use clap::{Parser, Subcommand, ValueEnum};

mod database;
mod resource;
mod run;
mod scaffold;

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
    /// Generate a typed CRUD resource and register it at the next build.
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
    },
    /// Start the API and, when present, the Vite development server.
    Dev {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
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
    #[arg(long, default_value = ".")]
    dir: PathBuf,
}

#[derive(Subcommand)]
enum Generate {
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
        Command::Dev { dir } => run::dev(&dir)?,
        Command::Data {
            action,
            dir,
            config,
        } => transfer(action, &dir, config)?,
    }
    Ok(())
}

fn transfer(action: Data, dir: &std::path::Path, config: Option<PathBuf>) -> Result<()> {
    let mut command = Process::new("cargo");
    command
        .current_dir(dir)
        .args(["run", "--locked", "-p", "rapid-server", "--"]);
    if let Some(config) = config {
        command.arg("--config").arg(config);
    } else if let Some(connection) = database::dev_connection(dir)? {
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
