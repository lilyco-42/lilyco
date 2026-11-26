use std::{error::Error, path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand, ValueEnum};

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
    New {
        name: String,
        #[arg(long, value_enum, default_value_t = Preset::Fullstack)]
        preset: Preset,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Generate a typed CRUD resource and register it at the next build.
    Generate {
        #[command(subcommand)]
        kind: Generate,
    },
    /// Start the API and, when present, the Vite development server.
    Dev {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
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

#[derive(Clone, Copy, ValueEnum)]
enum Preset {
    Api,
    Fullstack,
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
        Command::New { name, preset, dir } => {
            let created = scaffold::new(&dir, &name, preset)?;
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
    }
    Ok(())
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
