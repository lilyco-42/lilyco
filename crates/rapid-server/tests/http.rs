use std::num::NonZeroU16;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use rapid_server::{Api, ApiError, config::Config};
use serde_json::{Value, json};
use tower::ServiceExt;

fn settings(directory: &tempfile::TempDir) -> Config {
    let mut config: Config = toml::from_str(include_str!("../../lilyco-rapid/Rapid.toml")).unwrap();
    config.storage.path = directory.path().join("persistent.sqlite3");
    config.resources.page_size = NonZeroU16::new(1).unwrap();
    config
}

#[derive(Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Task {
    title: String,
    done: bool,
}

impl rapid_core::Resource for Task {
    const COLLECTION: &'static str = "tasks";

    fn validate(&self, max_text_bytes: NonZeroU16) -> Result<(), rapid_core::FieldError> {
        rapid_core::validate_text("title", &self.title, max_text_bytes)
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Greeting {
    name: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct Reply {
    message: String,
}

async fn greet(input: Greeting) -> Result<Reply, ApiError> {
    let limit = NonZeroU16::new(200).unwrap();
    rapid_core::validate_text("name", &input.name, limit).map_err(ApiError::Invalid)?;
    Ok(Reply {
        message: format!("Hello, {}!", input.name.trim()),
    })
}

fn router(config: &Config) -> Router {
    let mut api = Api::new();
    api.register::<Task>().unwrap();
    api.get_query("/api/hello", greet).unwrap();
    api.router(config).unwrap()
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
    let app = router(&config);
    assert_eq!(
        request(&app, Method::GET, "/readyz", None).await.status(),
        204
    );
    let docs = request(&app, Method::GET, "/docs/", None).await;
    assert_eq!(docs.status(), StatusCode::OK);
    let body = to_bytes(docs.into_body(), 64 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("Swagger UI"));
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
    let app = router(&config);
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
    let app = router(&config);
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
    api.register::<Task>().unwrap();
    assert!(api.register::<Task>().is_err());
}

#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Count {
    count: u32,
}

#[tokio::test]
async fn handwritten_handlers_derive_requests_and_responses_from_rust_types() {
    let directory = tempfile::tempdir().unwrap();
    let config = settings(&directory);
    let mut api = Api::new();
    api.get("/api/build", || async { Ok(Count { count: 1 }) })
        .unwrap();
    api.get_path(
        "/api/build/{count}",
        |input: Count| async move { Ok(input) },
    )
    .unwrap();
    assert!(
        api.get_path(
            "/api/build/{other}",
            |input: Count| async move { Ok(input) }
        )
        .is_err()
    );
    assert!(
        api.get("/api/build", || async { Ok(Count { count: 0 }) })
            .is_err()
    );
    api.post_json("/api/build", |input: Count| async move { Ok(input) })
        .unwrap();
    let app = api.router(&config).unwrap();
    let accepted = request(&app, Method::POST, "/api/build", Some(r#"{"count":7}"#)).await;
    assert_eq!(as_json(accepted, StatusCode::OK).await["count"], 7);
    let invalid = request(&app, Method::POST, "/api/build", Some(r#"{"count":"7"}"#)).await;
    assert_eq!(
        as_json(invalid, StatusCode::UNPROCESSABLE_ENTITY).await["code"],
        "invalid_json"
    );
    let path = request(&app, Method::GET, "/api/build/5", None).await;
    assert_eq!(as_json(path, StatusCode::OK).await["count"], 5);
    let invalid = request(&app, Method::GET, "/api/build/abc", None).await;
    assert_eq!(
        as_json(invalid, StatusCode::BAD_REQUEST).await["code"],
        "invalid_path"
    );

    let doc = as_json(
        request(&app, Method::GET, "/openapi.json", None).await,
        StatusCode::OK,
    )
    .await;
    let request =
        &doc["paths"]["/api/build"]["post"]["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(request["properties"]["count"]["type"], "integer");
    assert_eq!(request["additionalProperties"], false);
    assert_eq!(
        doc["paths"]["/api/build/{count}"]["get"]["parameters"][0]["required"],
        true
    );
}

#[tokio::test]
async fn starter_query_example_is_live_and_documented() {
    let dir = tempfile::tempdir().unwrap();
    let app = router(&settings(&dir));
    let hello = request(&app, Method::GET, "/api/hello?name=Lilyco", None).await;
    assert_eq!(
        as_json(hello, StatusCode::OK).await["message"],
        "Hello, Lilyco!"
    );
    let missing = request(&app, Method::GET, "/api/hello", None).await;
    assert_eq!(
        as_json(missing, StatusCode::BAD_REQUEST).await["code"],
        "invalid_query"
    );
    let doc = as_json(
        request(&app, Method::GET, "/openapi.json", None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        doc["paths"]["/api/hello"]["get"]["parameters"][0]["name"],
        "name"
    );
}

#[cfg(feature = "sqlx-server")]
#[tokio::test]
async fn switching_sqlite_driver_reuses_records_and_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = settings(&dir);
    config.storage.path = dir.path().join("preview with spaces.sqlite3");
    let old_router = router(&config);
    let existing = request(
        &old_router,
        Method::POST,
        "/api/tasks",
        Some(r#"{"title":"Before","done":false}"#),
    )
    .await;
    assert_eq!(as_json(existing, StatusCode::CREATED).await["id"], 1);
    drop(old_router);

    for (offset, &driver) in relational_drivers().iter().enumerate() {
        config.storage.connection.driver = driver;
        let new_router = router(&config);
        let existing = request(&new_router, Method::GET, "/api/tasks/1", None).await;
        assert_eq!(
            as_json(existing, StatusCode::OK).await["data"]["title"],
            "Before"
        );
        let row = create_task(&new_router).await;
        assert_eq!(row["id"], offset + 2);
    }
}

#[tokio::test]
async fn snapshot_restores_ids_and_protects_existing_sqlite_data() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot = source_snapshot(&dir).await;
    let mut target = settings(&dir);
    target.storage.path = dir.path().join("target.sqlite3");
    let malformed = dir.path().join("malformed.json");
    std::fs::write(
        &malformed,
        r#"{"format":1,"watermark":2,"records":[{"id":1,"collection":"tasks","payload":"{bad"}]}"#,
    )
    .unwrap();
    assert!(
        rapid_server::data::import(&target.storage, &malformed)
            .await
            .is_err()
    );
    assert!(!target.storage.path.exists());
    check_snapshot(&target, &dir, &snapshot).await;
}

#[cfg(feature = "sqlx-server")]
#[tokio::test]
#[ignore = "requires separate test PostgreSQL and MySQL databases"]
async fn remote_sqlx_drivers_persist_and_enforce_the_same_http_contract() {
    use rapid_server::config::EngineKind;

    let dir = tempfile::tempdir().unwrap();
    let snapshot = source_snapshot(&dir).await;
    let mut config = settings(&dir);
    for (engine, env) in [
        (EngineKind::Postgres, "RAPID_TEST_POSTGRES_URL"),
        (EngineKind::Mysql, "RAPID_TEST_MYSQL_URL"),
    ] {
        config.storage.engine = engine;
        config.storage.connection.url_env = env.into();
        for &driver in relational_drivers() {
            config.storage.connection.driver = driver;
            check_snapshot(&config, &dir, &snapshot).await;
            let app = router(&config);
            assert_eq!(
                request(&app, Method::GET, "/readyz", None).await.status(),
                204
            );
            let doc = request(&app, Method::GET, "/openapi.json", None).await;
            assert_eq!(
                as_json(doc, StatusCode::OK).await["x-rapid-storage"],
                engine.to_string()
            );
        }
    }
}

const TRANSFER_TASK: &str = r#"{"title":"跨库迁移测试","done":false}"#;

#[cfg(feature = "mongodb")]
#[tokio::test]
#[ignore = "requires a dedicated MongoDB replica set"]
async fn remote_mongodb_preserves_records_and_identity() {
    use rapid_server::config::EngineKind;

    let dir = tempfile::tempdir().unwrap();
    let snapshot = source_snapshot(&dir).await;
    let mut config = settings(&dir);
    config.storage.engine = EngineKind::Mongodb;
    config.storage.connection.url_env = "RAPID_TEST_MONGODB_URL".into();
    check_snapshot(&config, &dir, &snapshot).await;
    check_concurrent_creates(&config).await;
}

#[cfg(feature = "surrealdb")]
#[tokio::test]
#[ignore = "requires a dedicated SurrealDB 4 database with a valid token"]
async fn remote_surrealdb_preserves_records_and_identity() {
    use rapid_server::config::EngineKind;

    let dir = tempfile::tempdir().unwrap();
    let snapshot = source_snapshot(&dir).await;
    let mut config = settings(&dir);
    config.storage.engine = EngineKind::Surrealdb;
    config.storage.connection.url_env = "RAPID_TEST_SURREALDB_URL".into();
    config.storage.connection.token_env = "RAPID_TEST_SURREALDB_TOKEN".into();
    check_snapshot(&config, &dir, &snapshot).await;
    check_concurrent_creates(&config).await;
}

#[cfg(feature = "turso")]
#[tokio::test]
#[ignore = "requires a dedicated libSQL server"]
async fn remote_turso_preserves_records_and_identity() {
    use rapid_server::config::EngineKind;

    let dir = tempfile::tempdir().unwrap();
    let snapshot = source_snapshot(&dir).await;
    let mut config = settings(&dir);
    config.storage.engine = EngineKind::Turso;
    config.storage.connection.url_env = "RAPID_TEST_TURSO_URL".into();
    if std::env::var_os("RAPID_TEST_TURSO_AUTH_REQUIRED").is_some() {
        config.storage.connection.token_env = "RAPID_TEST_TURSO_OMIT_TOKEN".into();
        let error = rapid_server::data::import(&config.storage, &snapshot)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("libSQL authorization failed"));
    }
    config.storage.connection.token_env = "RAPID_TEST_TURSO_TOKEN".into();
    check_snapshot(&config, &dir, &snapshot).await;
}

async fn create_task(app: &Router) -> Value {
    let created = request(app, Method::POST, "/api/tasks", Some(TRANSFER_TASK)).await;
    as_json(created, StatusCode::CREATED).await
}

async fn source_snapshot(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let config = settings(dir);
    let app = router(&config);
    assert_eq!(create_task(&app).await["id"], 1);
    assert_eq!(create_task(&app).await["id"], 2);
    assert_eq!(
        request(&app, Method::DELETE, "/api/tasks/2", None)
            .await
            .status(),
        204
    );
    let output = dir.path().join("source.json");
    assert_eq!(
        rapid_server::data::export(&config.storage, &output)
            .await
            .unwrap(),
        1
    );
    assert!(
        rapid_server::data::export(&config.storage, &output)
            .await
            .is_err()
    );
    output
}

async fn check_snapshot(config: &Config, dir: &tempfile::TempDir, snapshot: &std::path::Path) {
    use rapid_server::data;

    assert_eq!(data::import(&config.storage, snapshot).await.unwrap(), 1);
    let occupied = data::import(&config.storage, snapshot).await.unwrap_err();
    assert!(
        occupied
            .to_string()
            .contains("destination already contains Rapid records")
    );
    let app = router(config);
    let existing = as_json(
        request(&app, Method::GET, "/api/tasks/1", None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        existing["data"],
        serde_json::from_str::<Value>(TRANSFER_TASK).unwrap()
    );
    let update = request(&app, Method::PUT, "/api/tasks/1", Some(TRANSFER_TASK)).await;
    assert_eq!(as_json(update, StatusCode::OK).await, existing);

    let export_dir = tempfile::tempdir_in(dir).unwrap();
    let output = export_dir.path().join("roundtrip.json");
    assert_eq!(data::export(&config.storage, &output).await.unwrap(), 1);
    let actual: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    let original: Value = serde_json::from_slice(&std::fs::read(snapshot).unwrap()).unwrap();
    assert_eq!(actual["records"], original["records"]);
    let watermark = actual["watermark"].as_u64().unwrap();
    assert!(watermark >= original["watermark"].as_u64().unwrap());
    let row = create_task(&app).await;
    let new_id = row["id"].as_u64().unwrap();
    assert!(new_id > watermark);
    for path in ["/api/tasks/1".to_owned(), format!("/api/tasks/{new_id}")] {
        assert_eq!(
            request(&app, Method::DELETE, &path, None).await.status(),
            204
        );
        assert_eq!(request(&app, Method::GET, &path, None).await.status(), 404);
    }
}

#[cfg(any(feature = "mongodb", feature = "surrealdb"))]
async fn check_concurrent_creates(config: &Config) {
    use std::collections::BTreeSet;

    let app = router(config);
    let mut concurrent = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let app = app.clone();
        concurrent.spawn(async move { create_task(&app).await });
    }
    let mut ids = BTreeSet::new();
    while let Some(row) = concurrent.join_next().await {
        let id = row.unwrap()["id"].as_u64().unwrap();
        assert!(ids.insert(id));
    }
    assert_eq!(ids.len(), 8);
    let page = as_json(
        request(&app, Method::GET, "/api/tasks", None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(page["items"][0]["id"], *ids.first().unwrap());
    for id in ids {
        let url = format!("/api/tasks/{id}");
        assert_eq!(
            request(&app, Method::DELETE, &url, None).await.status(),
            204
        );
    }
}

#[cfg(feature = "sqlx-server")]
fn relational_drivers() -> &'static [rapid_server::config::Driver] {
    use rapid_server::config::Driver;

    &[
        Driver::Sqlx,
        #[cfg(feature = "seaorm")]
        Driver::Seaorm,
    ]
}
