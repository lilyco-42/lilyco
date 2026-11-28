#![forbid(unsafe_code)]

#[cfg(feature = "auth")]
pub mod auth;
pub mod config;
pub mod data;
mod endpoint;
mod error;
pub use error::ApiError;
mod openapi;
mod store;

use std::collections::BTreeSet;

use axum::{
    Router,
    body::Bytes,
    extract::DefaultBodyLimit,
    http::{StatusCode, header},
    routing::get,
};
use rapid_core::Resource;
use schemars::JsonSchema;

use crate::{config::Config, store::Store};

mod routes;

pub struct Api {
    router: Router<routes::State>,
    openapi: openapi::Routes,
    collections: BTreeSet<&'static str>,
}

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error(transparent)]
    Store(#[from] store::StoreError),
    #[error("generate OpenAPI contract: {0}")]
    Schema(#[from] serde_json::Error),
    #[error("http.max_in_flight exceeds the runtime limit")]
    InvalidConcurrency,
    #[error("invalid resource collection name {0:?}")]
    InvalidCollection(&'static str),
    #[error("endpoint path {0:?} must start with /api/ and use valid segments")]
    InvalidEndpoint(String),
    #[error("endpoint parameter model must match {0:?}")]
    InvalidParameters(String),
    #[error("endpoint conflicts with an existing route at {0}")]
    EndpointConflict(String),
    #[error("resource collection {0:?} registered twice")]
    Duplicate(&'static str),
    #[cfg(feature = "auth")]
    #[error("initialize auth: {0}")]
    Auth(String),
}

impl Api {
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            openapi: openapi::Routes::default(),
            collections: BTreeSet::new(),
        }
    }

    pub fn register<T: JsonSchema + Resource>(&mut self) -> Result<(), StartupError> {
        let name = T::COLLECTION;
        let valid = name.starts_with(|ch: char| ch.is_ascii_lowercase())
            && name
                .bytes()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-');
        if !valid {
            return Err(StartupError::InvalidCollection(name));
        }
        if self.collections.contains(name) {
            return Err(StartupError::Duplicate(name));
        }
        self.openapi.resource::<T>(name)?;
        self.collections.insert(name);
        let collection = format!("/api/{name}");
        let router = Router::new()
            .route(
                &collection,
                get(routes::list::<T>).post(routes::create::<T>),
            )
            .route(
                &format!("{collection}/{{id}}"),
                get(routes::read::<T>)
                    .put(routes::replace::<T>)
                    .delete(routes::delete::<T>),
            );
        self.router = std::mem::take(&mut self.router).merge(router);
        Ok(())
    }

    pub fn router(self, config: &Config) -> Result<Router, StartupError> {
        self.make_router(config, Store::open(&config.storage)?)
    }

    pub async fn connect(self, config: &Config) -> Result<Router, StartupError> {
        self.make_router(config, Store::connect(&config.storage).await?)
    }

    fn make_router(self, config: &Config, store: Store) -> Result<Router, StartupError> {
        let max_in_flight = config.http.max_in_flight.get();
        if max_in_flight > tokio::sync::Semaphore::MAX_PERMITS {
            return Err(StartupError::InvalidConcurrency);
        }
        let api = Bytes::from(serde_json::to_vec(
            &self
                .openapi
                .document(&config.project.name, config.storage.engine)?,
        )?);
        let state = routes::State {
            store,
            rules: config.resources,
        };
        Ok(self
            .router
            .merge(
                utoipa_swagger_ui::SwaggerUi::new("/docs")
                    .config(utoipa_swagger_ui::Config::from("/openapi.json")),
            )
            .route("/healthz", get(|| async { StatusCode::NO_CONTENT }))
            .route("/readyz", get(routes::ready))
            .route(
                "/openapi.json",
                get(move || async move { ([(header::CONTENT_TYPE, "application/json")], api) }),
            )
            .fallback(|| async { ApiError::NotFound })
            .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
            .layer(DefaultBodyLimit::max(config.http.max_body_bytes.get()))
            .layer(tower::limit::GlobalConcurrencyLimitLayer::new(
                max_in_flight,
            ))
            .with_state(state))
    }
}

impl Default for Api {
    fn default() -> Self {
        Self::new()
    }
}

pub fn project_router(config: &Config) -> Result<Router, StartupError> {
    Api::new().router(config)
}

pub async fn connect_project(config: &Config) -> Result<Router, StartupError> {
    Api::new().connect(config).await
}
