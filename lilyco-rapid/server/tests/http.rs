use std::num::NonZeroU16;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use rapid_server::{Api, config::Config, project_router};
use serde_json::{Value, json};
use tower::ServiceExt;

fn settings(directory: &tempfile::TempDir) -> Config {
    let mut config: Config = toml::from_str(include_str!("../../Rapid.toml")).unwrap();
    config.storage.path = directory.path().join("persistent.sqlite3");
    config.resources.page_size = NonZeroU16::new(1).unwrap();
    config
}

async fn request(router: &Router, method: Method, path: &str, body: Option<&str>) -> Response {
    let mut req = Request::builder().method(method).uri(path);
    if body.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    router
        .clone()
        .oneshot(req.body(Body::from(body.unwrap_or("").to_owned())).unwrap())
        .await
        .unwrap()
}

async fn as_json(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let bytes = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn typed_api_persists_across_restarts_and_exposes_its_contract() {
    let directory = tempfile::tempdir().unwrap();
    let config = settings(&directory);
    let app = project_router(&config).unwrap();
    assert_eq!(
        request(&app, Method::GET, "/readyz", None).await.status(),
        204
    );
    let route = "/api/tasks";
    let first = as_json(
        request(
            &app,
            Method::POST,
            route,
            Some(r#"{"title":"第一条","done":false}"#),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(first["data"]["title"], "第一条");
    let second = as_json(
        request(
            &app,
            Method::POST,
            route,
            Some(r#"{"title":"Second","done":true}"#),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let first_id = first["id"].as_u64().unwrap();
    let id_route = format!("{route}/{first_id}");

    let doc = as_json(
        request(&app, Method::GET, "/openapi.json", None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(doc["openapi"], "3.1.0");
    let schema =
        &doc["paths"][route]["post"]["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"]["done"]["type"], "boolean");
    assert_eq!(
        doc["paths"]["/api/tasks/{id}"]["parameters"][0]["required"],
        true
    );
    assert!(doc["paths"]["/api/tasks/{id}"]["delete"]["responses"]["204"].is_object());

    drop(app);
    let app = project_router(&config).unwrap();
    let page = as_json(
        request(&app, Method::GET, route, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(page, json!({"items":[first], "next": first_id}));
    let after = format!("{route}?after={first_id}");
    let page = as_json(
        request(&app, Method::GET, &after, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(page, json!({"items":[second], "next": null}));
    let patched = request(
        &app,
        Method::PUT,
        &id_route,
        Some(r#"{"title":"Changed","done":true}"#),
    )
    .await;
    assert_eq!(as_json(patched, StatusCode::OK).await["data"]["done"], true);
    assert_eq!(
        request(&app, Method::DELETE, &id_route, None)
            .await
            .status(),
        204
    );
    assert_eq!(
        request(&app, Method::GET, &id_route, None).await.status(),
        404
    );
}

#[tokio::test]
async fn malformed_and_oversized_requests_leave_storage_empty() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = settings(&directory);
    config.http.max_body_bytes = 90.try_into().unwrap();
    let route = "/api/tasks";
    let app = project_router(&config).unwrap();
    for (body, status, code) in [
        (
            r#"{"title":"x","done":false,"unrecognized":1}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_json",
        ),
        (
            r#"{"title":"   ","done":false}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_field",
        ),
        (
            r#"{"title":"x","done":"false"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_json",
        ),
        ("{", StatusCode::BAD_REQUEST, "invalid_json"),
    ] {
        let response = request(&app, Method::POST, route, Some(body)).await;
        assert_eq!(as_json(response, status).await["code"], code);
    }
    let body = json!({"title": "x".repeat(100), "done": false}).to_string();
    assert_eq!(
        request(&app, Method::POST, route, Some(&body))
            .await
            .status(),
        413
    );
    assert_eq!(request(&app, Method::POST, route, None).await.status(), 415);
    assert_eq!(
        request(&app, Method::GET, "/api/tasks/0", None)
            .await
            .status(),
        400
    );
    assert_eq!(
        request(&app, Method::GET, "/api/tasks?bogus=1", None)
            .await
            .status(),
        400
    );
    assert_eq!(
        request(&app, Method::PATCH, route, None).await.status(),
        405
    );
    let page = as_json(
        request(&app, Method::GET, route, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(page, json!({"items":[], "next":null}));
}

#[test]
fn duplicate_resources_fail_without_panicking() {
    let mut api = Api::new();
    api.register::<rapid_core::resource_tasks::Model>().unwrap();
    assert!(api.register::<rapid_core::resource_tasks::Model>().is_err());
}
