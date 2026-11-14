use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, Database, DatabaseConnection, EntityTrait,
    QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PAGE: &str = include_str!("../frontend/dist/index.html");
const ECOSYSTEM: &str = include_str!("../ecosystem.json");
const MAX_SCHEMA_BYTES: usize = 512 * 1024;
/// 运行历史只保留最近 500 条，避免本地 SQLite 无限增长。
const HISTORY_LIMIT: i64 = 500;

mod dev;
mod manifest;
mod mcp;
mod release;
mod supervisor;

use manifest::Manifest;
use supervisor::Supervisor;

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
    apps: Arc<Vec<Manifest>>,
    warnings: Arc<Vec<String>>,
    supervisor: Supervisor,
    dev: dev::DevRunner,
    install_lock: Arc<tokio::sync::Mutex<()>>,
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

/// 状态变更接口额外校验同源，降低 DNS rebinding / 跨站请求风险。
fn authorised_mutation(headers: &HeaderMap, token: &str) -> bool {
    if !authorised(headers, token) {
        return false;
    }
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) else {
        return false;
    };
    origin == format!("http://{host}")
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
    manifest::which(default)
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
        tool: Set(name.clone()),
        output: Set(output.to_string()),
        created_at: Set(now_secs().to_string()),
        ..Default::default()
    };
    record
        .insert(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    prune_history(&state).await;
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

async fn app_list(State(state): State<App>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut apps = Vec::with_capacity(state.apps.len());
    for app in state.apps.iter() {
        let status = state.supervisor.status(&app.id).await;
        let installed = manifest::resolve_binary(app).is_some();
        apps.push(json!({
            "id": app.id,
            "name": app.name,
            "description": app.description,
            "repository": app.repository,
            "binary": app.binary,
            "installed": installed,
            "capabilities": {
                "run": app.capabilities.run,
                "mcp": app.capabilities.mcp,
                "schema": app.capabilities.schema,
            },
            "url": app.run.as_ref().and_then(|run| run.url.clone()),
            "release": app.release.is_some(),
            "dev_tasks": app.dev.as_ref().map(|dev| dev.tasks.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
            "status": status.map(|status| json!({
                "running": status.running,
                "pid": status.pid,
                "started_at": status.started_at,
                "exit_code": status.exit_code,
                "log_lines": status.log_lines,
            })),
        }));
    }
    Ok(Json(json!({"apps": apps, "warnings": *state.warnings})))
}

fn find_app<'a>(state: &'a App, id: &str) -> Result<&'a Manifest, StatusCode> {
    state
        .apps
        .iter()
        .find(|app| app.id == id)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn app_start(
    State(state): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised_mutation(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let app = find_app(&state, &id)?;
    if !app.capabilities.run {
        return Err(StatusCode::BAD_REQUEST);
    }
    let program = manifest::resolve_binary(app).ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let empty_env = std::collections::BTreeMap::new();
    let env = app.run.as_ref().map(|run| &run.env).unwrap_or(&empty_env);
    let pid = state
        .supervisor
        .start(
            &app.id,
            program,
            app.run_args(),
            env,
            app.source_dir.as_deref(),
        )
        .await
        .map_err(|err| match err {
            supervisor::StartError::AlreadyRunning(pid) => {
                eprintln!("lilyco-manager: {} 已在运行 (PID {pid})", app.id);
                StatusCode::CONFLICT
            }
            supervisor::StartError::Spawn(message) => {
                eprintln!("lilyco-manager: {}", message);
                StatusCode::BAD_GATEWAY
            }
        })?;
    record_run(
        &state,
        &format!("app.{}.start", app.id),
        json!({"pid": pid}),
    )
    .await;
    Ok(Json(json!({"ok": true, "pid": pid})))
}

async fn app_stop(
    State(state): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised_mutation(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let app = find_app(&state, &id)?;
    let code = state
        .supervisor
        .stop(&app.id)
        .await
        .map_err(|_| StatusCode::CONFLICT)?;
    record_run(
        &state,
        &format!("app.{}.stop", app.id),
        json!({"exit_code": code}),
    )
    .await;
    Ok(Json(json!({"ok": true, "exit_code": code})))
}

#[derive(Deserialize)]
struct LogQuery {
    limit: Option<usize>,
}

async fn app_logs(
    State(state): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let app = find_app(&state, &id)?;
    let limit = query.limit.unwrap_or(200).clamp(1, 500);
    let lines = state.supervisor.logs(&app.id, limit).await;
    Ok(Json(json!({"id": app.id, "lines": lines})))
}

async fn app_schema(
    State(state): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let app = find_app(&state, &id)?;
    if !app.capabilities.schema {
        return Err(StatusCode::BAD_REQUEST);
    }
    let program = manifest::resolve_binary(app).ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let child = tokio::process::Command::new(program)
        .args(app.schema_args())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(5), child)
        .await
        .map_err(|_| StatusCode::GATEWAY_TIMEOUT)?
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !output.status.success() || output.stdout.len() > MAX_SCHEMA_BYTES {
        return Err(StatusCode::BAD_GATEWAY);
    }
    let schema: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| StatusCode::BAD_GATEWAY)?;
    let commands = schema.as_array().map_or(0, Vec::len);
    record_run(
        &state,
        &format!("app.{}.schema", app.id),
        json!({"commands": commands}),
    )
    .await;
    Ok(Json(
        json!({"id": app.id, "commands": commands, "schema": schema}),
    ))
}

async fn app_release(
    State(state): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let app = find_app(&state, &id)?;
    Ok(Json(
        json!({"id": app.id, "release": release::status(app).await}),
    ))
}

async fn app_install(
    State(state): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !authorised_mutation(&headers, &state.token) {
        return Err((StatusCode::UNAUTHORIZED, Json(json!({"error": "未授权"}))));
    }
    let app = find_app(&state, &id).map_err(|code| (code, Json(json!({"error": "应用不存在"}))))?;
    if app.release.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "该应用未配置 release 发布源"})),
        ));
    }
    let _guard = state.install_lock.lock().await;
    if let Some(status) = state.supervisor.status(&app.id).await {
        if status.running {
            state.supervisor.stop(&app.id).await.map_err(|error| {
                (
                    StatusCode::CONFLICT,
                    Json(json!({"error": format!("安装前停止应用失败: {error}")})),
                )
            })?;
        }
    }
    match release::install(app).await {
        Ok(outcome) => {
            record_run(
                &state,
                &format!("app.{}.install", app.id),
                json!({"tag": outcome.tag, "updated": outcome.updated, "binary": outcome.binary}),
            )
            .await;
            Ok(Json(json!({
                "ok": true,
                "tag": outcome.tag,
                "binary": outcome.binary,
                "updated": outcome.updated,
            })))
        }
        Err(error) => {
            eprintln!("lilyco-manager: {} 安装失败: {error}", app.id);
            Err((StatusCode::BAD_GATEWAY, Json(json!({"error": error}))))
        }
    }
}

async fn app_uninstall(
    State(state): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !authorised_mutation(&headers, &state.token) {
        return Err((StatusCode::UNAUTHORIZED, Json(json!({"error": "未授权"}))));
    }
    let app = find_app(&state, &id).map_err(|code| (code, Json(json!({"error": "应用不存在"}))))?;
    let _guard = state.install_lock.lock().await;
    if let Some(status) = state.supervisor.status(&app.id).await {
        if status.running {
            state.supervisor.stop(&app.id).await.map_err(|error| {
                (
                    StatusCode::CONFLICT,
                    Json(json!({"error": format!("卸载前停止应用失败: {error}")})),
                )
            })?;
        }
    }
    match release::uninstall(app) {
        Ok(removed) => {
            record_run(
                &state,
                &format!("app.{}.uninstall", app.id),
                json!({"removed": removed}),
            )
            .await;
            Ok(Json(json!({"ok": true, "removed": removed})))
        }
        Err(error) => {
            eprintln!("lilyco-manager: {} 卸载失败: {error}", app.id);
            Err((StatusCode::BAD_GATEWAY, Json(json!({"error": error}))))
        }
    }
}

async fn app_dev(
    State(state): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let app = find_app(&state, &id)?;
    let tasks: Vec<Value> = dev::task_list(app)
        .into_iter()
        .map(|(name, argv)| json!({"name": name, "argv": argv}))
        .collect();
    let mut jobs = state.dev.list(Some(&app.id)).await;
    for job in &mut jobs {
        job.lines.clear();
    }
    Ok(Json(json!({"id": app.id, "tasks": tasks, "jobs": jobs})))
}

async fn dev_task(
    State(state): State<App>,
    headers: HeaderMap,
    Path((id, task)): Path<(String, String)>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised_mutation(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let app = find_app(&state, &id)?;
    let job = state
        .dev
        .start(app, &task)
        .await
        .map_err(|error| match error {
            dev::DevError::UnknownTask => StatusCode::NOT_FOUND,
            dev::DevError::AlreadyRunning => StatusCode::CONFLICT,
            dev::DevError::NoSource => StatusCode::BAD_REQUEST,
            dev::DevError::Spawn(message) => {
                eprintln!("lilyco-manager: {message}");
                StatusCode::BAD_GATEWAY
            }
        })?;
    record_run(
        &state,
        &format!("app.{}.dev.{}", app.id, task),
        json!({"job": job}),
    )
    .await;
    Ok(Json(json!({"ok": true, "job": job})))
}

async fn dev_job_logs(
    State(state): State<App>,
    headers: HeaderMap,
    Path(job): Path<String>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let limit = query.limit.unwrap_or(200).clamp(1, 500);
    let mut snapshot = state
        .dev
        .logs(&job, limit)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let skip = snapshot.lines.len().saturating_sub(limit);
    snapshot.lines = snapshot.lines.split_off(skip);
    Ok(Json(json!({"job": snapshot})))
}

async fn mcp_tools(
    State(state): State<App>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut set = tokio::task::JoinSet::new();
    for app in state.apps.iter() {
        if !app.capabilities.mcp {
            continue;
        }
        let Some(program) = manifest::resolve_binary(app) else {
            continue;
        };
        let id = app.id.clone();
        let args = app.mcp_args();
        set.spawn(async move {
            let result = mcp::discover(&id, &program, &args).await;
            (id, result)
        });
    }

    let mut tools = Vec::new();
    let mut providers = Vec::new();
    while let Some(joined) = set.join_next().await {
        let Ok((id, result)) = joined else {
            continue;
        };
        match result {
            Ok(mut discovered) => {
                let count = discovered.len();
                tools.append(&mut discovered);
                providers.push(json!({"id": id, "ok": true, "tools": count}));
            }
            Err(error) => providers.push(json!({"id": id, "ok": false, "error": error})),
        }
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools.truncate(1000);
    providers.sort_by(|a, b| {
        a.get("id")
            .and_then(Value::as_str)
            .cmp(&b.get("id").and_then(Value::as_str))
    });
    Ok(Json(json!({"tools": tools, "providers": providers})))
}

fn mcp_servers(state: &App) -> BTreeMap<String, Value> {
    let mut servers = BTreeMap::new();
    for app in state.apps.iter() {
        if !app.capabilities.mcp {
            continue;
        }
        let Some(program) = manifest::resolve_binary(app) else {
            continue;
        };
        let alias = app.id.replace(['-', '.'], "_");
        if servers.contains_key(&alias) {
            continue;
        }
        servers.insert(
            alias,
            json!({
                "command": program.to_string_lossy(),
                "args": app.mcp_args(),
            }),
        );
    }
    servers
}

fn config_path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
    Some(root.join("lilyco").join("tools.json"))
}

fn toolhub_path() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("LILYCO_TOOLHUB")?);
    (path.is_absolute() && path.is_file()).then_some(path)
}

fn mcp_config_value(state: &App) -> Value {
    let servers = mcp_servers(state);
    let path = config_path();
    let toolhub = toolhub_path();
    let client_config = match (&toolhub, &path) {
        (Some(hub), Some(config)) => json!({
            "mcpServers": {
                "lilyco": {
                    "command": if cfg!(windows) { "python" } else { "python3" },
                    "args": [hub.to_string_lossy(), "--config", config.to_string_lossy()],
                }
            }
        }),
        _ => Value::Null,
    };
    let has_client = !client_config.is_null();
    json!({
        "servers": servers,
        "config_path": path.map(|path| path.to_string_lossy().to_string()),
        "toolhub": toolhub.map(|path| path.to_string_lossy().to_string()),
        "client_config": client_config,
        "hint": if has_client {
            "把 client_config 写入 Claude Desktop / Cursor / Codex 的 MCP 配置即可"
        } else {
            "设置 LILYCO_TOOLHUB 为 lilyco-toolhub/hub.py 的绝对路径后，可导出 MCP 客户端配置"
        },
    })
}

async fn mcp_config(
    State(state): State<App>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(mcp_config_value(&state)))
}

async fn write_mcp_config(
    State(state): State<App>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !authorised_mutation(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let path = config_path().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let servers = mcp_servers(&state);
    let body = serde_json::to_vec_pretty(&json!({"servers": servers}))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(&path, body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    record_run(
        &state,
        "mcp.config.write",
        json!({"path": path.to_string_lossy()}),
    )
    .await;
    let mut config = mcp_config_value(&state);
    if let Some(object) = config.as_object_mut() {
        object.insert("written".into(), Value::Bool(true));
    }
    Ok(Json(config))
}

async fn record_run(state: &App, tool: &str, output: Value) {
    let record = runs::ActiveModel {
        tool: Set(tool.to_string()),
        output: Set(output.to_string()),
        created_at: Set(now_secs().to_string()),
        ..Default::default()
    };
    let _ = record.insert(&state.db).await;
    prune_history(state).await;
}

/// 保留最近 HISTORY_LIMIT 条记录。失败不影响主流程。
async fn prune_history(state: &App) {
    let sql = format!(
        "DELETE FROM runs WHERE id NOT IN (SELECT id FROM runs ORDER BY id DESC LIMIT {HISTORY_LIMIT})"
    );
    let _ = state.db.execute_unprepared(&sql).await;
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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
    let db = connect_db(&db_path).await?;
    let token = std::env::var("LILYCO_MANAGER_TOKEN").unwrap_or(
        session_token().map_err(|err| format!("random token generation failed: {err}"))?,
    );
    if token.len() < 24 {
        return Err("LILYCO_MANAGER_TOKEN must be at least 24 characters".into());
    }
    let registry = manifest::load();
    for warning in &registry.warnings {
        eprintln!("lilyco-manager: {warning}");
    }
    let app = build_state(db, token.clone(), registry.apps, registry.warnings);
    let router = build_router(app);
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

async fn connect_db(path: &str) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let url = format!("sqlite://{path}?mode=rwc");
    let db = Database::connect(url).await?;
    db.execute_unprepared("CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY AUTOINCREMENT, tool TEXT NOT NULL, output TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")
        .await?;
    Ok(db)
}

fn build_state(
    db: DatabaseConnection,
    token: String,
    apps: Vec<Manifest>,
    warnings: Vec<String>,
) -> App {
    App {
        db,
        token,
        apps: Arc::new(apps),
        warnings: Arc::new(warnings),
        supervisor: Supervisor::default(),
        dev: dev::DevRunner::default(),
        install_lock: Arc::new(tokio::sync::Mutex::new(())),
    }
}

fn build_router(app: App) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/tools", get(list))
        .route("/api/ecosystem", get(ecosystem))
        .route("/api/adapters", get(adapters))
        .route("/api/tools/:name/run", post(run))
        .route("/api/history", get(history))
        .route("/api/apps", get(app_list))
        .route("/api/apps/:id/start", post(app_start))
        .route("/api/apps/:id/stop", post(app_stop))
        .route("/api/apps/:id/logs", get(app_logs))
        .route("/api/apps/:id/schema", get(app_schema))
        .route("/api/apps/:id/release", get(app_release))
        .route("/api/apps/:id/install", post(app_install))
        .route("/api/apps/:id/uninstall", post(app_uninstall))
        .route("/api/apps/:id/dev", get(app_dev))
        .route("/api/apps/:id/dev/:task", post(dev_task))
        .route("/api/dev/jobs/:job/logs", get(dev_job_logs))
        .route("/api/mcp/tools", get(mcp_tools))
        .route("/api/mcp/config", get(mcp_config).post(write_mcp_config))
        .with_state(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn test_state() -> App {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY AUTOINCREMENT, tool TEXT NOT NULL, output TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")
            .await
            .unwrap();
        build_state(db, "t".repeat(32), vec![sample_manifest()], vec![])
    }

    fn sample_manifest() -> Manifest {
        Manifest {
            schema_version: 1,
            id: "demo".into(),
            name: "Demo".into(),
            description: "测试应用".into(),
            repository: None,
            binary: "lilyco-manager-test-missing-binary".into(),
            capabilities: Default::default(),
            run: None,
            mcp: None,
            schema: None,
            release: None,
            dev: None,
            source_dir: None,
        }
    }

    fn request(method: &str, uri: &str, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header("x-lilyco-token", token);
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn call(app: &App, request: Request<Body>) -> (StatusCode, Value) {
        let response = build_router(app.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    #[tokio::test]
    async fn api_requires_session_token() {
        let app = test_state().await;
        let (status, _) = call(&app, request("GET", "/api/tools", None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, body) = call(&app, request("GET", "/api/tools", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["tools"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn run_records_history() {
        let app = test_state().await;
        let (status, body) = call(
            &app,
            request("POST", "/api/tools/manager.health/run", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["output"]["status"], "ok");

        let (status, body) = call(&app, request("GET", "/api/history", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK);
        let runs = body["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["tool"], "manager.health");
    }

    #[tokio::test]
    async fn mutations_reject_cross_origin() {
        let app = test_state().await;
        let mut cross = request("POST", "/api/apps/missing/start", Some(&app.token));
        cross
            .headers_mut()
            .insert("host", "127.0.0.1:9000".parse().unwrap());
        cross
            .headers_mut()
            .insert("origin", "http://evil.example".parse().unwrap());
        let (status, _) = call(&app, cross).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let mut same = request("POST", "/api/apps/missing/start", Some(&app.token));
        same.headers_mut()
            .insert("host", "127.0.0.1:9000".parse().unwrap());
        same.headers_mut()
            .insert("origin", "http://127.0.0.1:9000".parse().unwrap());
        let (status, _) = call(&app, same).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn apps_endpoint_lists_manifests() {
        let app = test_state().await;
        let (status, body) = call(&app, request("GET", "/api/apps", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK);
        let apps = body["apps"].as_array().unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0]["id"], "demo");
        assert_eq!(apps[0]["installed"], false);
        assert!(body["warnings"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn history_is_bounded() {
        let app = test_state().await;
        for index in 0..(HISTORY_LIMIT + 5) {
            record_run(&app, "manager.health", json!({"index": index})).await;
        }
        let records = runs::Entity::find().all(&app.db).await.unwrap();
        assert_eq!(records.len(), HISTORY_LIMIT as usize);
        let (status, body) = call(&app, request("GET", "/api/history", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["runs"].as_array().unwrap().len(), 50);
    }

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

    #[test]
    fn origin_must_match_host_for_mutations() {
        let mut headers = HeaderMap::new();
        headers.insert("x-lilyco-token", "t".parse().unwrap());
        headers.insert("host", "127.0.0.1:9000".parse().unwrap());
        assert!(authorised_mutation(&headers, "t"));
        headers.insert("origin", "http://127.0.0.1:9000".parse().unwrap());
        assert!(authorised_mutation(&headers, "t"));
        headers.insert("origin", "http://evil.example".parse().unwrap());
        assert!(!authorised_mutation(&headers, "t"));
    }

    #[test]
    fn mcp_aliases_are_tool_name_safe() {
        let alias = "lilyco.manager".replace(['-', '.'], "_");
        assert_eq!(alias, "lilyco_manager");
        assert!(alias.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }
}
