use axum::{extract::{Path, State}, http::{HeaderMap, StatusCode}, response::Html, routing::{get, post}, Json, Router};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::{net::SocketAddr, path::PathBuf, sync::{Arc, Mutex}};

const PAGE: &str = include_str!("../web/index.html");

#[derive(Clone)]
struct App {
    db: Arc<Mutex<Connection>>,
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
        ToolInfo { name: "manager.health", description: "管理器健康状态", read_only: true },
        ToolInfo { name: "manager.tools", description: "已注册的工具列表", read_only: true },
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

async fn index() -> Html<&'static str> { Html(PAGE) }

async fn list(State(state): State<App>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) { return Err(StatusCode::UNAUTHORIZED); }
    Ok(Json(json!({"tools":tools()})))
}

async fn run(State(state): State<App>, headers: HeaderMap, Path(name): Path<String>) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) { return Err(StatusCode::UNAUTHORIZED); }
    let output = execute(&name).map_err(|_| StatusCode::NOT_FOUND)?;
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute("INSERT INTO runs(tool, output) VALUES (?1, ?2)", params![name, output.to_string()])
      .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok":true,"output":output})))
}

async fn history(State(state): State<App>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) { return Err(StatusCode::UNAUTHORIZED); }
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = db.prepare("SELECT id, tool, output, created_at FROM runs ORDER BY id DESC LIMIT 50")
      .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = stmt.query_map([], |row| Ok(json!({
        "id":row.get::<_, i64>(0)?,
        "tool":row.get::<_, String>(1)?,
        "output":row.get::<_, String>(2)?,
        "created_at":row.get::<_, String>(3)?
    }))).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"runs":rows.filter_map(Result::ok).collect::<Vec<_>>()})))
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = std::env::var_os("LILYCO_MANAGER_DB").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("lilyco-manager.sqlite"));
    let db = Connection::open(db_path)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY, tool TEXT NOT NULL, output TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")?;
    let token = std::env::var("LILYCO_MANAGER_TOKEN").unwrap_or_else(|_| {
        use std::io::Read;
        let mut random = [0u8; 24];
        if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
            if file.read_exact(&mut random).is_ok() {
                return random.iter().map(|x| format!("{x:02x}")).collect();
            }
        }
        // Windows fallback: reject without an explicit token instead of using a weak token.
        String::new()
    });
    if token.is_empty() {
        return Err("Set LILYCO_MANAGER_TOKEN to a strong random value".into());
    }
    let app = App { db: Arc::new(Mutex::new(db)), token: token.clone() };
    let router = Router::new()
        .route("/", get(index))
        .route("/api/tools", get(list))
        .route("/api/tools/:name/run", post(run))
        .route("/api/history", get(history))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr: SocketAddr = listener.local_addr()?;
    let url = format!("http://{addr}/#token={token}");
    println!("Lilyco Manager running at http://{addr}/ (token in browser URL fragment)");
    if std::env::var_os("LILYCO_MANAGER_NO_BROWSER").is_none() { open_browser(&url); }
    axum::serve(listener, router).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_tools_are_read_only() {
        assert!(tools().iter().all(|tool| tool.read_only));
        assert_eq!(execute("manager.health").unwrap()["status"], "ok");
        assert!(execute("unregistered").is_err());
    }
}
