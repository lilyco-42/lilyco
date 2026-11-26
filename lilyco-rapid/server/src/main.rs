use std::{error::Error, path::PathBuf};

use rapid_server::config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().init();
    let path = std::env::var_os("RAPID_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("Rapid.toml"));
    let config = Config::load(&path)?;
    let app = rapid_server::project_router(&config)?;
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
