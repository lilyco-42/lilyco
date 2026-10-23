use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, Database, DatabaseConnection, EntityTrait,
    QueryOrder, QuerySelect,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

const PAGE: &str = include_str!("../frontend/dist/index.html");
const ECOSYSTEM: &str = include_str!("../ecosystem.json");

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

async fn ecosystem(
    State(state): State<App>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let projects: Value =
        serde_json::from_str(ECOSYSTEM).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"projects": projects})))
}
async fn list(State(state): State<App>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(json!({"tools":tools()})))
}

/// Only these two fixed executable integrations are permitted. No arbitrary paths or arguments
/// supplied by API callers are ever executed.
fn executable_path(name: &str) -> Option<PathBuf> {
    let (key, default) = match name {
        "desktop.status" => ("LILYCO_DESKTOP_SWITCHER_EXE", "lyco-desktop-switcher"),
        "lyco.version" => ("LILYCO_CLI_EXE", "lyco"),
        _ => return None,
    };
    if let Some(path) = std::env::var_os(key) {
        let path = PathBuf::from(path);
        return (path.is_absolute() && path.is_file()).then_some(path);
    }
    let paths = std::env::split_paths(&std::env::var_os("PATH")?);
    for directory in paths {
        for suffix in if cfg!(windows) {
            vec![".exe"]
        } else {
            vec![""]
        } {
            let path = directory.join(format!("{default}{suffix}"));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

async fn adapter_call(name: &str) -> Result<Value, StatusCode> {
    let program = executable_path(name).ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let args: &[&str] = match name {
        "desktop.status" => &["status", "--json"],
        "lyco.version" => &["--version"],
        _ => return Err(StatusCode::NOT_FOUND),
    };
    let child = tokio::process::Command::new(program)
        .args(args)
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(4), child)
        .await
        .map_err(|_| StatusCode::GATEWAY_TIMEOUT)?
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !output.status.success() {
        return Err(StatusCode::BAD_GATEWAY);
    }
    if output.stdout.len() > 64 * 1024 {
        return Err(StatusCode::BAD_GATEWAY);
    }
    let stdout = String::from_utf8(output.stdout).map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(match name {
        "desktop.status" => serde_json::from_str(&stdout).map_err(|_| StatusCode::BAD_GATEWAY)?,
        "lyco.version" => json!({"version":stdout.trim()}),
        _ => return Err(StatusCode::NOT_FOUND),
    })
}

async fn adapters(State(state): State<App>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(json!({"adapters": [
        {"tool":"desktop.status","project":"lyco-desktop-switcher","installed":executable_path("desktop.status").is_some(),"read_only":true},
        {"tool":"lyco.version","project":"lyco-cli","installed":executable_path("lyco.version").is_some(),"read_only":true}
    ]})))
}
async fn run(
    State(state): State<App>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let output = match name.as_str() {
        "desktop.status" | "lyco.version" => adapter_call(&name).await?,
        _ => execute(&name).map_err(|_| StatusCode::NOT_FOUND)?,
    };
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
    db.execute_unprepared("CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY AUTOINCREMENT, tool TEXT NOT NULL, output TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")
    .await?;
    let token = std::env::var("LILYCO_MANAGER_TOKEN").unwrap_or(
        session_token().map_err(|err| format!("random token generation failed: {err}"))?,
    );
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
        .route("/api/ecosystem", get(ecosystem))
        .route("/api/adapters", get(adapters))
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
