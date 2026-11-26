//! Same-origin Web assets and the transport adapter for the core UI catalog.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderValue};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use lilyco_core::web_ui::{UiCatalog, UiCommand};
use lilyco_core::UI_PROTOCOL_VERSION;
use serde::Deserialize;

use crate::files::MAX_UPLOAD_BYTES;
use crate::i18n::script_catalog;
use crate::render::pick_command;
use crate::state::AppState;
use crate::util::html_escape;

#[derive(Deserialize)]
pub(crate) struct CatalogQuery {
    cmd: Option<String>,
}

pub(crate) async fn catalog(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CatalogQuery>,
) -> impl IntoResponse {
    let (commands, selected) = match &state.registry {
        Some(registry) => (
            registry.visible().map(UiCommand::from).collect(),
            pick_command(registry, query.cmd.as_deref()).name.clone(),
        ),
        None => (
            vec![UiCommand {
                name: state.schema.name.clone(),
                about: state.schema.about.clone(),
                args: state.schema.args.clone(),
                views: Vec::new(),
            }],
            state.schema.name.clone(),
        ),
    };
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(UiCatalog {
            commands,
            selected,
            can_cancel: state.registry.is_some(),
            protocol_version: UI_PROTOCOL_VERSION.to_owned(),
        }),
    )
}

pub(crate) async fn index(State(state): State<Arc<AppState>>) -> Response {
    let html = include_str!("../assets/shadcn/index.html")
        .replace("__UPLOAD_LIMIT__", &MAX_UPLOAD_BYTES.to_string())
        .replace("__PICK__", if cfg!(feature = "pick") { "1" } else { "0" })
        .replace("__CATALOG__", script_catalog())
        .replace("__TOKEN__", &html_escape(&state.token));
    let mut response = Html(html).into_response();
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             connect-src 'self'; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(crate) async fn javascript() -> impl IntoResponse {
    static_asset(
        "text/javascript; charset=utf-8",
        include_str!("../assets/shadcn/gui.js"),
    )
}

pub(crate) async fn stylesheet() -> impl IntoResponse {
    static_asset(
        "text/css; charset=utf-8",
        include_str!("../assets/shadcn/gui.css"),
    )
}

pub(crate) async fn notices() -> impl IntoResponse {
    static_asset(
        "text/plain; charset=utf-8",
        include_str!("../assets/shadcn/THIRD-PARTY-NOTICES.txt"),
    )
}

fn static_asset(content_type: &'static str, data: &'static str) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        data,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fixture::registry_state;

    #[tokio::test]
    async fn entry_point_uses_shadcn_and_keeps_session_uncached() {
        let response = index(State(registry_state())).await;
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains(r#"src="/ui/gui.js""#));
        assert!(html.contains(r#"href="/ui/gui.css""#));
        assert!(!html.contains("__TOKEN__"));
    }

    #[tokio::test]
    async fn catalog_hides_internal_commands_and_falls_back_to_visible_commands() {
        let response = catalog(
            State(registry_state()),
            Query(CatalogQuery {
                cmd: Some("secret".into()),
            }),
        )
        .await
        .into_response();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let actual: UiCatalog = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(actual.selected, "ping");
        assert_eq!(actual.commands.len(), 1);
        assert_eq!(actual.commands[0].name, "ping");
        assert_eq!(actual.protocol_version, UI_PROTOCOL_VERSION);
        assert!(actual.can_cancel);
    }

    #[tokio::test]
    async fn catalog_names_match_the_registry_even_when_the_schema_name_differs() {
        use crate::state::fixture::{registry_state_with, schema_of};
        use lilyco_core::{RegisteredCommand, Registry};

        let mut registry = Registry::new();
        registry
            .register(
                RegisteredCommand::new("check", schema_of("internal_name", "", vec![])).alias("c"),
            )
            .unwrap();
        let response = catalog(
            State(registry_state_with(registry)),
            Query(CatalogQuery {
                cmd: Some("c".into()),
            }),
        )
        .await
        .into_response();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let actual: UiCatalog = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(actual.selected, "check");
        assert_eq!(actual.commands[0].name, "check");
    }
}
