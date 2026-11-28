#![cfg(feature = "auth")]

use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use axum_login::{AuthManagerLayerBuilder, AuthSession, AuthUser, AuthnBackend, UserId};
use casbin::{CoreApi, DefaultModel, Enforcer, MemoryAdapter, MgmtApi};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tower_sessions::{MemoryStore, SessionManagerLayer};

const MODEL: &str = r#"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = r.sub == p.sub && r.obj == p.obj && r.act == p.act
"#;

#[derive(Debug, Clone)]
pub struct User {
    id: i64,
    username: String,
    role: String,
    display_name: String,
    password_hash: String,
}

impl AuthUser for User {
    type Id = i64;

    fn id(&self) -> Self::Id {
        self.id
    }

    fn session_auth_hash(&self) -> &[u8] {
        self.password_hash.as_bytes()
    }
}

impl User {
    fn view(&self) -> UserView {
        UserView {
            id: self.id,
            username: self.username.clone(),
            role: self.role.clone(),
            display_name: self.display_name.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UserView {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("hash password: {0}")]
    Password(String),
    #[error("password hash is invalid")]
    HashFormat,
    #[error("configure rate limiter")]
    RateLimit,
    #[error(transparent)]
    Casbin(#[from] casbin::Error),
}

#[derive(Clone)]
pub struct Backend {
    users: Arc<Vec<User>>,
}

impl AuthnBackend for Backend {
    type User = User;
    type Credentials = Credentials;
    type Error = AuthError;

    async fn authenticate(&self, credentials: Credentials) -> Result<Option<User>, AuthError> {
        let Some(user) = self
            .users
            .iter()
            .find(|user| user.username == credentials.username)
        else {
            return Ok(None);
        };
        let hash = PasswordHash::new(&user.password_hash).map_err(|_| AuthError::HashFormat)?;
        let valid = Argon2::default()
            .verify_password(credentials.password.as_bytes(), &hash)
            .is_ok();
        Ok(valid.then(|| user.clone()))
    }

    async fn get_user(&self, id: &UserId<Self>) -> Result<Option<User>, AuthError> {
        Ok(self.users.iter().find(|user| user.id == *id).cloned())
    }
}

#[derive(Clone)]
struct Authz {
    enforcer: Arc<Mutex<Enforcer>>,
}

impl Authz {
    async fn allowed(&self, role: &str, object: &str, action: &str) -> bool {
        let enforcer = self.enforcer.lock().await;
        enforcer
            .enforce((role, object, action))
            .is_ok_and(|allowed| allowed)
    }
}

#[derive(Clone)]
struct AppState {
    authz: Authz,
}

type Session = AuthSession<Backend>;

pub struct AuthOptions {
    pub users: Vec<(String, String, String)>,
    pub requests_per_second: u64,
    pub burst: u32,
}

impl Default for AuthOptions {
    fn default() -> Self {
        Self {
            users: vec![
                (
                    "admin".to_owned(),
                    "admin123".to_owned(),
                    "admin".to_owned(),
                ),
                (
                    "viewer".to_owned(),
                    "viewer123".to_owned(),
                    "viewer".to_owned(),
                ),
            ],
            requests_per_second: 1,
            burst: 3,
        }
    }
}

pub async fn router(options: AuthOptions) -> Result<Router, AuthError> {
    build_router(options, false).await
}

pub async fn demo_router(options: AuthOptions) -> Result<Router, AuthError> {
    build_router(options, true).await
}

async fn build_router(options: AuthOptions, demo: bool) -> Result<Router, AuthError> {
    let backend = Backend {
        users: Arc::new(hash_users(options.users)?),
    };
    let session = SessionManagerLayer::new(MemoryStore::default()).with_secure(false);
    let auth = AuthManagerLayerBuilder::new(backend, session).build();
    let authz = Authz {
        enforcer: Arc::new(Mutex::new(build_enforcer().await?)),
    };
    let governor = GovernorConfigBuilder::default()
        .per_second(options.requests_per_second)
        .burst_size(options.burst)
        .finish()
        .ok_or(AuthError::RateLimit)?;
    let mut router = Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me));
    if demo {
        router = router
            .route("/api/tasks", get(read_tasks))
            .route("/api/tasks/{id}", delete(delete_task));
    }
    Ok(router
        .with_state(AppState { authz })
        .layer(auth)
        .layer(GovernorLayer::new(governor)))
}

fn hash_users(seed: Vec<(String, String, String)>) -> Result<Vec<User>, AuthError> {
    seed.into_iter()
        .enumerate()
        .map(|(index, (username, password, role))| {
            let salt = SaltString::generate(&mut OsRng);
            let hash = Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map_err(|error| AuthError::Password(error.to_string()))?;
            Ok(User {
                id: i64::try_from(index)
                    .map_err(|_| AuthError::Password("too many users".into()))?
                    + 1,
                username: username.clone(),
                role,
                display_name: username,
                password_hash: hash.to_string(),
            })
        })
        .collect()
}

async fn build_enforcer() -> Result<Enforcer, AuthError> {
    let model = DefaultModel::from_str(MODEL).await?;
    let adapter = MemoryAdapter::default();
    let mut enforcer = Enforcer::new(model, adapter).await?;
    for (role, object, action) in [
        ("admin", "/api/tasks", "GET"),
        ("admin", "/api/tasks", "DELETE"),
        ("viewer", "/api/tasks", "GET"),
    ] {
        enforcer
            .add_policy(vec![role.to_owned(), object.to_owned(), action.to_owned()])
            .await?;
    }
    Ok(enforcer)
}

async fn login(session: Session, Json(credentials): Json<Credentials>) -> Response {
    let user = match session.authenticate(credentials).await {
        Ok(Some(user)) => user,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    match session.login(&user).await {
        Ok(()) => (StatusCode::OK, Json(user.view())).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn logout(session: Session) -> Response {
    match session.logout().await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn me(session: Session) -> Response {
    match session.user().await {
        Some(user) => (StatusCode::OK, Json(user.view())).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn read_tasks(session: Session, State(state): State<AppState>) -> Response {
    authorize(&session, &state, "GET", "/api/tasks").await
}

async fn delete_task(
    session: Session,
    State(state): State<AppState>,
    Path(_id): Path<i64>,
) -> Response {
    authorize(&session, &state, "DELETE", "/api/tasks").await
}

async fn authorize(session: &Session, state: &AppState, action: &str, object: &str) -> Response {
    let Some(user) = session.user().await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if state.authz.allowed(&user.role, object, action).await {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}
