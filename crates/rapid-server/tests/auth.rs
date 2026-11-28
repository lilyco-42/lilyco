#![cfg(feature = "auth")]

use std::net::SocketAddr;

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{Method, Request, StatusCode, header},
};
use rapid_server::auth::{AuthOptions, demo_router};
use tower::ServiceExt;

async fn app() -> Router {
    demo_router(AuthOptions {
        requests_per_second: 100,
        burst: 100,
        ..AuthOptions::default()
    })
    .await
    .expect("auth router")
}

async fn limited_app(burst: u32) -> Router {
    demo_router(AuthOptions {
        burst,
        ..AuthOptions::default()
    })
    .await
    .expect("auth router")
}

fn request(method: Method, path: &str, cookie: Option<&str>, body: Option<Body>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let mut request = builder
        .body(body.unwrap_or_else(Body::empty))
        .expect("request");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50_000))));
    request
}

async fn status(app: &Router, request: Request<Body>) -> StatusCode {
    app.clone()
        .oneshot(request)
        .await
        .expect("response")
        .status()
}

async fn login(app: &Router, username: &str, password: &str) -> (StatusCode, Option<String>) {
    let body = format!(r#"{{"username":"{username}","password":"{password}"}}"#);
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/auth/login",
            None,
            Some(Body::from(body)),
        ))
        .await
        .expect("login response");
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::to_owned);
    (response.status(), cookie)
}

#[tokio::test]
async fn login_role_authorization_and_session_renewal() {
    let app = app().await;

    let unauthenticated = status(&app, request(Method::GET, "/api/tasks", None, None)).await;
    assert_eq!(unauthenticated, StatusCode::UNAUTHORIZED);

    let (login_code, cookie) = login(&app, "admin", "admin123").await;
    assert_eq!(login_code, StatusCode::OK);
    let cookie = cookie.expect("login sets a session cookie");

    assert_eq!(
        status(
            &app,
            request(Method::GET, "/api/tasks", Some(&cookie), None)
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(
            &app,
            request(Method::DELETE, "/api/tasks/1", Some(&cookie), None)
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(&app, request(Method::GET, "/auth/me", Some(&cookie), None)).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, request(Method::GET, "/auth/me", Some(&cookie), None)).await,
        StatusCode::OK
    );

    let (viewer_code, viewer) = login(&app, "viewer", "viewer123").await;
    assert_eq!(viewer_code, StatusCode::OK);
    let viewer = viewer.expect("viewer session cookie");
    assert_eq!(
        status(
            &app,
            request(Method::GET, "/api/tasks", Some(&viewer), None)
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(
            &app,
            request(Method::DELETE, "/api/tasks/1", Some(&viewer), None)
        )
        .await,
        StatusCode::FORBIDDEN
    );

    let (wrong_code, _) = login(&app, "admin", "wrong").await;
    assert_eq!(wrong_code, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn repeated_requests_are_rate_limited() {
    let app = limited_app(1).await;
    let mut limited = false;
    for _ in 0..6 {
        let code = status(&app, request(Method::GET, "/api/tasks", None, None)).await;
        if code == StatusCode::TOO_MANY_REQUESTS {
            limited = true;
        }
    }
    assert!(
        limited,
        "tower_governor should return 429 for repeated requests"
    );
}
