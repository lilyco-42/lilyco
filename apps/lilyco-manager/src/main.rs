use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, Database, DatabaseConnection,
    DbBackend, EntityTrait, QueryOrder, QuerySelect, Statement,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::net::SocketAddr;

const PAGE: &str = include_str!("../web/index.html");

mod runs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "runs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub tool: String,
        pub output: String,
        pub created_at: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

#[derive(Clone)]
struct App {
    db: DatabaseConnection,
    token: String,
}

#[derive(Serialize)]
struct ToolInfo {
    name: &'static str,
    description: &'static str,
    read_only: bool,
}

fn tools() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: "manager.health",
            description: "管理器健康状态",
            read_only: true,
        },
        ToolInfo {
            name: "manager.tools",
            description: "已注册的工具列表",
            read_only: true,
        },
    ]
}

fn execute(name: &str) -> Result<Value, &'static str> {
    match name {
        "manager.health" => Ok(json!({"status":"ok","version":env!("CARGO_PKG_VERSION")})),
        "manager.tools" => Ok(json!({"tools":tools()})),
        _ => Err("unknown_tool"),
    }
}

fn authorised(headers: &HeaderMap, token: &str) -> bool {
    headers.get("x-lilyco-token").and_then(|v| v.to_str().ok()) == Some(token)
}

async fn index() -> Html<&'static str> {
    Html(PAGE)
}

async fn list(State(state): State<App>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(json!({"tools":tools()})))
}

async fn run(
    State(state): State<App>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let output = execute(&name).map_err(|_| StatusCode::NOT_FOUND)?;
    let record = runs::ActiveModel {
        tool: Set(name),
        output: Set(output.to_string()),
        created_at: Set(String::new()),
        ..Default::default()
    };
    record
        .insert(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok":true,"output":output})))
}

async fn history(State(state): State<App>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let records = runs::Entity::find()
        .order_by_desc(runs::Column::Id)
        .limit(50)
        .all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let values: Vec<Value> = records
        .iter()
        .map(|row| {
            json!({"id":row.id,"tool":row.tool,"output":row.output,"created_at":row.created_at})
        })
        .collect();
    Ok(Json(json!({"runs":values})))
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

fn session_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)?;
    Ok(bytes.iter().map(|x| format!("{x:02x}")).collect())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path =
        std::env::var("LILYCO_MANAGER_DB").unwrap_or_else(|_| "lilyco-manager.sqlite".to_string());
    let url = format!("sqlite://{db_path}?mode=rwc");
    let db = Database::connect(url).await?;
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY AUTOINCREMENT, tool TEXT NOT NULL, output TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);".to_string(),
    ))
    .await?;
    let token = std::env::var("LILYCO_MANAGER_TOKEN").unwrap_or(session_token()?);
    if token.len() < 24 {
        return Err("LILYCO_MANAGER_TOKEN must be at least 24 characters".into());
    }
    let app = App {
        db,
        token: token.clone(),
    };
    let router = Router::new()
        .route("/", get(index))
        .route("/api/tools", get(list))
        .route("/api/tools/:name/run", post(run))
        .route("/api/history", get(history))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr: SocketAddr = listener.local_addr()?;
    let url = format!("http://{addr}/#token={token}");
    println!("Lilyco Manager listening on http://{addr}/");
    if std::env::var_os("LILYCO_MANAGER_NO_BROWSER").is_none() {
        open_browser(&url);
    }
    axum::serve(listener, router).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tools_are_safe_read_only() {
        assert!(tools().iter().all(|tool| tool.read_only));
        assert_eq!(execute("manager.health").unwrap()["status"], "ok");
        assert!(execute("unregistered").is_err());
    }
    #[test]
    fn tokens_are_random() {
        let a = session_token().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, session_token().unwrap());
    }
}
