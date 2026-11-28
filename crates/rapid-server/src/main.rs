use std::{error::Error, path::PathBuf};

use clap::{Parser, Subcommand};
use rapid_server::config::Config;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    action: Option<Action>,
}

#[derive(Subcommand)]
enum Action {
    Export { path: PathBuf },
    Import { path: PathBuf },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().init();
    let cli = Cli::parse();
    let path = cli
        .config
        .or_else(|| std::env::var_os("RAPID_CONFIG").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("Rapid.toml"));
    let config = Config::load(&path)?;
    match cli.action {
        Some(Action::Export { path }) => {
            let count = rapid_server::data::export(&config.storage, &path).await?;
            println!("Exported {count} records to {}", path.display());
            return Ok(());
        }
        Some(Action::Import { path }) => {
            let count = rapid_server::data::import(&config.storage, &path).await?;
            println!("Imported and verified {count} records");
            return Ok(());
        }
        None => {}
    }

    let app = rapid_server::connect_project(&config).await?;
    let listener = tokio::net::TcpListener::bind(config.http.address).await?;
    tracing::info!(address = %listener.local_addr()?, project = %config.project.name, "API listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => report_signal(result),
                    _ = term.recv() => {}
                }
                return;
            }
            Err(error) => tracing::error!(%error, "failed to subscribe to SIGTERM"),
        }
    }
    report_signal(tokio::signal::ctrl_c().await);
}

fn report_signal(result: std::io::Result<()>) {
    if let Err(error) = result {
        tracing::error!(%error, "failed to subscribe to Ctrl-C");
    }
}
