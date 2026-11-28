#![forbid(unsafe_code)]

pub mod app;

use axum::Router;
use rapid_server::{Api, StartupError, config::Config};

pub async fn router(config: &Config) -> Result<Router, StartupError> {
    let mut api = Api::new();
    app_core::register_resources!(api);
    app::register(&mut api, config.resources.max_text_bytes)?;
    let app = api.connect(config).await?;
    let auth = rapid_server::auth::router(rapid_server::auth::AuthOptions::default())
        .await
        .map_err(|error| StartupError::Auth(error.to_string()))?;
    Ok(app.merge(auth))
}