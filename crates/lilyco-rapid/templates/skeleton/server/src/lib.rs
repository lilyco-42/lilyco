#![forbid(unsafe_code)]

pub mod app;

use axum::Router;
use rapid_server::{Api, StartupError, config::Config};

pub async fn router(config: &Config) -> Result<Router, StartupError> {
    let mut api = Api::new();
    app_core::register_resources!(api);
    app::register(&mut api, config.resources.max_text_bytes)?;
    api.connect(config).await
}