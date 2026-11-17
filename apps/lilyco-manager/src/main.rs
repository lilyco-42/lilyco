use async_stream::stream;
use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, Response,
    },
    routing::{get, patch, post},
    Json, Router,
};
use futures_core::Stream;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, Database, DatabaseConnection, EntityTrait,
    QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PAGE: &str = include_str!("../frontend/dist/index.html");
const ECOSYSTEM: &str = include_str!("../ecosystem.json");
const MAX_SCHEMA_BYTES: usize = 512 * 1024;
/// 运行历史只保留最近 500 条，避免本地 SQLite 无限增长。
const HISTORY_LIMIT: i64 = 500;

mod dev;
mod manifest;
mod mcp;
mod projects;
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
    /// 一次性日志流票据：job id -> 过期时间。EventSource 不能带自定义 header，
    /// 所以先用带 token 的 POST 换短期票据，票据只能用于对应 job 且单次有效。
    tickets: Arc<tokio::sync::Mutex<BTreeMap<String, StreamTicket>>>,
}

#[derive(Debug, Clone)]
struct StreamTicket {
    job: String,
    expires: Instant,
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

/// 机器可读的接口清单：给管理面板、脚本和契约测试共用。
/// 只暴露路由形状与鉴权要求，不包含 token、本机路径或进程信息。
async fn openapi(State(state): State<App>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorised(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(api_spec()))
}

fn operation(summary: &str, tag: &str) -> Value {
    json!({
        "summary": summary,
        "tags": [tag],
        "responses": {
            "200": {"description": "成功"},
            "401": {"description": "缺少或错误的 x-lilyco-token"},
        }
    })
}

/// 项目/方案写接口除了 401 还有 400（校验失败）与 404（目标不存在），
/// 这两类码是面板与脚本的分支依据，写进文档而不是让调用方猜。
fn project_operation(summary: &str) -> Value {
    let mut operation = operation(summary, "projects");
    operation["responses"]["400"] = json!({"description": "名称/标题/状态/长度校验失败"});
    operation["responses"]["404"] = json!({"description": "项目或方案不存在"});
    operation
}

fn api_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Lilyco Manager API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "本机只读工具、应用生命周期、MCP 聚合与开发任务。全部接口要求 x-lilyco-token，状态变更接口额外校验 Origin/Host 同源；开发任务日志流先用 POST 换一次性票据，再由 EventSource 带 ticket 订阅。",
        },
        "servers": [{"url": "http://127.0.0.1"}],
        "components": {
            "securitySchemes": {
                "managerToken": {"type": "apiKey", "in": "header", "name": "x-lilyco-token"}
            }
        },
        "security": [{"managerToken": []}],
        "paths": {
            "/api/tools": {"get": operation("只读工具注册表", "tools")},
            "/api/ecosystem": {"get": operation("已审核生态项目目录", "meta")},
            "/api/adapters": {"get": operation("固定可执行适配器可用性", "meta")},
            "/api/tools/{name}/run": {"post": operation("执行已注册的只读工具", "tools")},
            "/api/history": {"get": operation("最近的执行记录", "tools")},
            "/api/apps": {"get": operation("应用清单与受管进程状态", "apps")},
            "/api/apps/{id}/start": {"post": operation("启动清单定义的应用", "apps")},
            "/api/apps/{id}/stop": {"post": operation("停止管理器启动的应用", "apps")},
            "/api/apps/{id}/logs": {"get": operation("读取有界进程日志", "apps")},
            "/api/apps/{id}/schema": {"get": operation("执行 --schema 并返回命令清单", "apps")},
            "/api/apps/{id}/release": {"get": operation("查询 GitHub Release 与本地托管版本", "release")},
            "/api/apps/{id}/install": {"post": operation("校验 SHA-256 后安装托管二进制", "release")},
            "/api/apps/{id}/uninstall": {"post": operation("删除托管安装目录", "release")},
            "/api/apps/{id}/dev": {"get": operation("清单中的固定开发任务与最近记录", "dev")},
            "/api/apps/{id}/dev/{task}": {"post": operation("按固定 argv 启动开发任务", "dev")},
            "/api/dev/jobs/{job}/logs": {"get": operation("读取开发任务输出与退出码", "dev")},
            "/api/dev/jobs/{job}/stream-ticket": {"post": operation("为日志流签发一次性短期票据", "dev")},
            "/api/dev/jobs/{job}/stream": {"get": operation("SSE 实时推送开发任务输出", "dev")},
            "/api/projects": {
                "get": project_operation("项目列表与嵌套方案"),
                "post": project_operation("创建项目"),
            },
            "/api/projects/{id}": {
                "get": project_operation("读取单个项目与方案"),
                "patch": project_operation("更新项目名称、描述或状态"),
                "delete": project_operation("删除项目并级联删除方案"),
            },
            "/api/projects/{id}/plans": {"post": project_operation("为项目新增方案")},
            "/api/plans/{id}": {
                "patch": project_operation("更新方案标题、说明或状态"),
                "delete": project_operation("删除方案"),
            },
            "/api/mcp/tools": {"get": operation("聚合各应用 --mcp 的工具列表", "mcp")},
            "/api/mcp/config": {
                "get": operation("生成 lilyco-toolhub 配置", "mcp"),
                "post": operation("写入本机 tools.json", "mcp"),
            },
            "/api/openapi.json": {"get": operation("本接口清单", "meta")},
        }
    })
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

const STREAM_TICKET_TTL: Duration = Duration::from_secs(30);
const STREAM_POLL: Duration = Duration::from_millis(150);

#[derive(Deserialize)]
struct StreamQuery {
    ticket: Option<String>,
}

fn stream_ticket() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// 用带 token 的 POST 换一张短期、单次、绑定 job 的票据。
/// 浏览器 `EventSource` 不能设置 `x-lilyco-token`，票据让长期 token 不进入 URL 与访问日志。
async fn dev_stream_ticket(
    State(state): State<App>,
    headers: HeaderMap,
    Path(job): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !authorised_mutation(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if state.dev.logs(&job, 1).await.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let ticket = stream_ticket().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tickets = state.tickets.lock().await;
    let now = Instant::now();
    tickets.retain(|_, entry| entry.expires > now);
    tickets.insert(
        ticket.clone(),
        StreamTicket {
            job,
            expires: now + STREAM_TICKET_TTL,
        },
    );
    Ok(Json(
        json!({"ticket": ticket, "expires_in": STREAM_TICKET_TTL.as_secs()}),
    ))
}

/// SSE 增量推送：只发送上次之后的新行，任务结束后发 `done` 并关闭流。
async fn dev_stream(
    State(state): State<App>,
    Path(job): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let Some(ticket) = query.ticket else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let valid = {
        let mut tickets = state.tickets.lock().await;
        matches!(tickets.remove(&ticket), Some(entry) if entry.job == job && entry.expires > Instant::now())
    };
    if !valid {
        return Err(StatusCode::UNAUTHORIZED);
    }
    // 票据校验之后再判断 job 是否存在，避免匿名探测。
    if state.dev.logs(&job, 1).await.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let dev = state.dev.clone();
    let stream = stream! {
        // 已消费到的累计行号（含已被环形缓冲裁掉的行）。用累计行号而不是
        // 缓冲区长度，缓冲写满并开始 pop_front 后仍能继续推送新行。
        let mut consumed = 0u64;
        loop {
            let Some(snapshot) = dev.logs(&job, 500).await else { break };
            let available = snapshot.lines.len() as u64;
            let first_index = snapshot.total_lines.saturating_sub(available);
            let skip = consumed.saturating_sub(first_index) as usize;
            for line in snapshot.lines.iter().skip(skip) {
                let data = serde_json::to_string(line).unwrap_or_else(|_| "\"\"".to_string());
                yield Ok(Event::default().event("line").data(data));
            }
            consumed = snapshot.total_lines;
            if !snapshot.running {
                let done = json!({
                    "exit_code": snapshot.exit_code,
                    "finished_at": snapshot.finished_at,
                });
                yield Ok(Event::default().event("done").data(done.to_string()));
                break;
            }
            tokio::time::sleep(STREAM_POLL).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
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

/// SQLite URL 里的路径统一用 `/`。Windows 的 `verify\runs.sqlite` 或
/// `C:\\Users\\me\\runs.sqlite` 直接塞进 `sqlite://` 会解析失败，
/// 盘符路径需要写成 `sqlite:///C:/...`（三条斜杠）。
fn sqlite_url(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let absolute_drive = normalized.len() >= 2 && normalized.as_bytes()[1] == b':';
    if absolute_drive {
        format!("sqlite:///{normalized}?mode=rwc")
    } else {
        format!("sqlite://{normalized}?mode=rwc")
    }
}

async fn connect_db(path: &str) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let db = Database::connect(sqlite_url(path)).await?;
    db.execute_unprepared("CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY AUTOINCREMENT, tool TEXT NOT NULL, output TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")
        .await?;
    projects::ensure_tables(&db).await?;
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
        tickets: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
    }
}

/// 项目路由的鉴权中间件。必须在 `Json` 提取之前执行：否则无 token 的
/// POST/PATCH 会先被提取器判成 400/415/422，破坏「全路由无 token 一律 401」
/// 的 OpenAPI 契约。写操作同时校验 Origin（沿用 authorised_mutation）。
async fn projects_guard(
    State(state): State<App>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let allowed = if request.method() == Method::GET || request.method() == Method::HEAD {
        authorised(request.headers(), &state.token)
    } else {
        authorised_mutation(request.headers(), &state.token)
    };
    if !allowed {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

async fn projects_list(State(state): State<App>) -> Result<Json<Value>, StatusCode> {
    Ok(Json(projects::list(&state.db).await?))
}

async fn project_get(
    State(state): State<App>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, StatusCode> {
    Ok(Json(projects::get(&state.db, id).await?))
}

async fn project_create(
    State(state): State<App>,
    Json(input): Json<projects::ProjectInput>,
) -> Result<Json<Value>, StatusCode> {
    Ok(Json(projects::create(&state.db, input).await?))
}

async fn project_update(
    State(state): State<App>,
    Path(id): Path<i32>,
    Json(patch): Json<projects::ProjectPatch>,
) -> Result<Json<Value>, StatusCode> {
    Ok(Json(projects::update(&state.db, id, patch).await?))
}

async fn project_delete(
    State(state): State<App>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, StatusCode> {
    Ok(Json(projects::delete(&state.db, id).await?))
}

async fn plan_create(
    State(state): State<App>,
    Path(id): Path<i32>,
    Json(input): Json<projects::PlanInput>,
) -> Result<Json<Value>, StatusCode> {
    Ok(Json(projects::create_plan(&state.db, id, input).await?))
}

async fn plan_update(
    State(state): State<App>,
    Path(id): Path<i32>,
    Json(patch): Json<projects::PlanPatch>,
) -> Result<Json<Value>, StatusCode> {
    Ok(Json(projects::update_plan(&state.db, id, patch).await?))
}

async fn plan_delete(
    State(state): State<App>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, StatusCode> {
    Ok(Json(projects::delete_plan(&state.db, id).await?))
}

fn build_router(app: App) -> Router {
    let projects_router = Router::new()
        .route("/api/projects", get(projects_list).post(project_create))
        .route(
            "/api/projects/:id",
            get(project_get)
                .patch(project_update)
                .delete(project_delete),
        )
        .route("/api/projects/:id/plans", post(plan_create))
        .route("/api/plans/:id", patch(plan_update).delete(plan_delete))
        .route_layer(middleware::from_fn_with_state(app.clone(), projects_guard));

    Router::new()
        .route("/", get(index))
        .route("/api/tools", get(list))
        .route("/api/ecosystem", get(ecosystem))
        .route("/api/openapi.json", get(openapi))
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
        .route("/api/dev/jobs/:job/stream-ticket", post(dev_stream_ticket))
        .route("/api/dev/jobs/:job/stream", get(dev_stream))
        .route("/api/mcp/tools", get(mcp_tools))
        .route("/api/mcp/config", get(mcp_config).post(write_mcp_config))
        .merge(projects_router)
        .with_state(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn test_state_with(apps: Vec<Manifest>) -> App {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY AUTOINCREMENT, tool TEXT NOT NULL, output TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")
            .await
            .unwrap();
        projects::ensure_tables(&db).await.unwrap();
        build_state(db, "t".repeat(32), apps, vec![])
    }

    async fn test_state() -> App {
        test_state_with(vec![sample_manifest()]).await
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

    /// Windows 上 `verify\runs.sqlite` 曾被拼成无法解析的 URL：
    /// 这里钉住相对路径、反斜杠、盘符、Unix 绝对路径四种形态。
    #[test]
    fn sqlite_url_normalises_windows_paths() {
        assert_eq!(sqlite_url("runs.sqlite"), "sqlite://runs.sqlite?mode=rwc");
        assert_eq!(
            sqlite_url("verify\\runs.sqlite"),
            "sqlite://verify/runs.sqlite?mode=rwc"
        );
        assert_eq!(
            sqlite_url("C:\\Users\\me\\runs.sqlite"),
            "sqlite:///C:/Users/me/runs.sqlite?mode=rwc"
        );
        assert_eq!(
            sqlite_url("/tmp/runs.sqlite"),
            "sqlite:///tmp/runs.sqlite?mode=rwc"
        );
    }

    /// 反斜杠路径不只要 URL 合法，连接与建表也要真的可用。
    #[tokio::test]
    async fn connect_db_accepts_backslash_paths() {
        let dir = std::env::temp_dir().join(format!("lilyco-manager-db-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let windows_style = dir.join("runs.sqlite").to_string_lossy().replace('/', "\\");

        let db = connect_db(&windows_style).await.unwrap();
        runs::ActiveModel {
            tool: Set("db.test".into()),
            output: Set("{}".into()),
            created_at: Set(now_secs().to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert_eq!(runs::Entity::find().all(&db).await.unwrap().len(), 1);
        db.close().await.unwrap();

        assert!(dir.join("runs.sqlite").is_file(), "库文件应落到归一化路径");
        let _ = std::fs::remove_dir_all(&dir);
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

    async fn call_json(
        app: &App,
        method: &str,
        uri: &str,
        body: Value,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("x-lilyco-token", token);
        }
        let request = builder.body(Body::from(body.to_string())).unwrap();
        call(app, request).await
    }

    /// 项目/方案全生命周期：创建 → 列表嵌套形状 → 单读 → 更新 → 删除方案 →
    /// 级联删除项目。写操作还要覆盖「无 token 401 先于 body 解析」。
    #[tokio::test]
    async fn projects_and_plans_roundtrip() {
        let app = test_state().await;
        let token = app.token.clone();

        let (status, body) = call(&app, request("GET", "/api/projects", Some(&token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["projects"], json!([]));
        assert_eq!(body["count"], 0);

        let (status, body) = call_json(
            &app,
            "POST",
            "/api/projects",
            json!({"name": " 发布 v0.3 ", "description": " 阶段目标 ", "status": "active"}),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let project_id = body["project"]["id"].as_i64().unwrap();
        assert_eq!(body["project"]["name"], "发布 v0.3");
        assert_eq!(body["project"]["description"], "阶段目标");
        assert_eq!(body["project"]["status"], "active");
        assert_eq!(body["project"]["plans"], json!([]));

        // 方案默认状态 draft，返回的 project_id 与路径一致。
        let (status, body) = call_json(
            &app,
            "POST",
            &format!("/api/projects/{project_id}/plans"),
            json!({"title": "接入 SSE"}),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let plan_id = body["plan"]["id"].as_i64().unwrap();
        assert_eq!(body["plan"]["status"], "draft");
        assert_eq!(body["plan"]["project_id"], project_id);

        // 列表把方案嵌在项目里，面板一次请求即可渲染。
        let (status, body) = call(&app, request("GET", "/api/projects", Some(&token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["count"], 1);
        assert_eq!(body["projects"][0]["plans"][0]["title"], "接入 SSE");

        let (status, body) = call(
            &app,
            request("GET", &format!("/api/projects/{project_id}"), Some(&token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["project"]["plans"][0]["id"], plan_id);

        // 只更新传入字段，其余保持原值。
        let (status, body) = call_json(
            &app,
            "PATCH",
            &format!("/api/projects/{project_id}"),
            json!({"status": "paused"}),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["project"]["status"], "paused");
        assert_eq!(body["project"]["name"], "发布 v0.3");

        let (status, body) = call_json(
            &app,
            "PATCH",
            &format!("/api/plans/{plan_id}"),
            json!({"title": "接入 SSE 日志流", "status": "doing"}),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["plan"]["title"], "接入 SSE 日志流");
        assert_eq!(body["plan"]["status"], "doing");

        let (status, body) = call(
            &app,
            request("DELETE", &format!("/api/plans/{plan_id}"), Some(&token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        let (status, _) = call(
            &app,
            request("DELETE", &format!("/api/plans/{plan_id}"), Some(&token)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = call_json(
            &app,
            "POST",
            &format!("/api/projects/{project_id}/plans"),
            json!({"title": "第二条"}),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = call(
            &app,
            request(
                "DELETE",
                &format!("/api/projects/{project_id}"),
                Some(&token),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["deleted_plans"], 1);
        let (status, _) = call(
            &app,
            request("GET", &format!("/api/projects/{project_id}"), Some(&token)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (_, body) = call(&app, request("GET", "/api/projects", Some(&token))).await;
        assert_eq!(body["projects"], json!([]));
    }

    /// 项目/方案的状态码契约：无 token 401（先于 JSON 解析）、未知 id 404、
    /// 名称/标题/状态/长度校验 400、跨源写操作 401。
    #[tokio::test]
    async fn projects_validation_and_unknown_ids_are_explicit() {
        let app = test_state().await;
        let token = app.token.clone();

        // 无 token 的写请求即使 body 合法也必须 401，不能先报 400/415/422。
        for (method, uri, body) in [
            ("POST", "/api/projects", json!({"name": "x"})),
            ("PATCH", "/api/projects/1", json!({"status": "done"})),
            ("DELETE", "/api/projects/1", json!(null)),
        ] {
            let (status, _) = call_json(&app, method, uri, body, None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        }

        // 跨源写操作同样拒绝。
        let cross_origin = Request::builder()
            .method("POST")
            .uri("/api/projects")
            .header("content-type", "application/json")
            .header("x-lilyco-token", &token)
            .header("origin", "http://evil.example")
            .header("host", "127.0.0.1:4321")
            .body(Body::from(json!({"name": "cross"}).to_string()))
            .unwrap();
        let (status, _) = call(&app, cross_origin).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // 未知 id 一律 404，而不是 200/500。
        for (method, uri) in [
            ("GET", "/api/projects/999"),
            ("PATCH", "/api/projects/999"),
            ("DELETE", "/api/projects/999"),
            ("POST", "/api/projects/999/plans"),
            ("PATCH", "/api/plans/999"),
            ("DELETE", "/api/plans/999"),
        ] {
            let (status, _) = call_json(
                &app,
                method,
                uri,
                json!({"name": "x", "title": "x"}),
                Some(&token),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        }

        for body in [
            json!({"name": "   "}),
            json!({"name": "x".repeat(121)}),
            json!({"name": "ok", "status": "blocked"}),
            json!({"name": "ok", "description": "x".repeat(2001)}),
        ] {
            let (status, _) = call_json(&app, "POST", "/api/projects", body, Some(&token)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }

        let (_, body) = call_json(
            &app,
            "POST",
            "/api/projects",
            json!({"name": "校验"}),
            Some(&token),
        )
        .await;
        let id = body["project"]["id"].as_i64().unwrap();
        for body in [
            json!({"title": ""}),
            json!({"title": "x".repeat(161)}),
            json!({"title": "ok", "status": "blocked"}),
            json!({"title": "ok", "detail": "x".repeat(4001)}),
        ] {
            let (status, _) = call_json(
                &app,
                "POST",
                &format!("/api/projects/{id}/plans"),
                body,
                Some(&token),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
    }

    #[cfg(unix)]
    async fn call_text(app: &App, request: Request<Body>) -> (StatusCode, String, Option<String>) {
        let response = build_router(app.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            String::from_utf8_lossy(&bytes).to_string(),
            content_type,
        )
    }

    /// 日志超过 500 行后环形缓冲开始裁剪：SSE 必须继续推送新行，且每行只发一次。
    #[cfg(unix)]
    #[tokio::test]
    async fn dev_stream_keeps_emitting_after_log_buffer_wraps() {
        let dir = std::env::temp_dir().join(format!("lilyco-manager-wrap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let manifest_path = dir.join(manifest::MANIFEST_FILE);
        std::fs::write(
            &manifest_path,
            r#"{
                "schema_version": 1,
                "id": "devwrap",
                "name": "Dev Wrap",
                "binary": "sh",
                "dev": {"tasks": {"burst": ["sh", "-c", "i=0; while [ $i -lt 510 ]; do echo line-$i; i=$((i+1)); done; sleep 0.3; echo line-after-cap"]}}
            }"#,
        )
        .unwrap();
        let manifest = manifest::load_file(&manifest_path).unwrap();
        let app = test_state_with(vec![manifest]).await;
        let token = app.token.clone();

        let (status, body) = call(
            &app,
            request("POST", "/api/apps/devwrap/dev/burst", Some(&token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let job = body["job"].as_str().unwrap().to_string();
        let (status, body) = call(
            &app,
            request(
                "POST",
                &format!("/api/dev/jobs/{job}/stream-ticket"),
                Some(&token),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let ticket = body["ticket"].as_str().unwrap();

        let (status, text, _) = call_text(
            &app,
            request(
                "GET",
                &format!("/api/dev/jobs/{job}/stream?ticket={ticket}"),
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // 缓冲写满后的新行仍然到达（旧实现会在这里静默停住）。
        assert!(text.contains("\"line-after-cap\""), "{text}");
        assert!(text.contains("\"line-509\""), "{text}");
        // 不重复发送：同一行只出现一次。
        assert_eq!(
            text.matches("\"line-400\"").count(),
            1,
            "line-400 重复发送: {text}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SSE 实时进度：长期 token 不进 URL；用带 token 的 POST 换单次短期票据，
    /// 票据绑定 job、只能成功用一次；流里先到增量行，最后到 done。
    #[cfg(unix)]
    #[tokio::test]
    async fn dev_stream_requires_ticket_and_pushes_incrementally() {
        let dir = std::env::temp_dir().join(format!("lilyco-manager-sse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let manifest_path = dir.join(manifest::MANIFEST_FILE);
        std::fs::write(
            &manifest_path,
            r#"{
                "schema_version": 1,
                "id": "devstream",
                "name": "Dev Stream",
                "binary": "sh",
                "dev": {"tasks": {"stream": ["sh", "-c", "echo first-line; sleep 0.4; echo second-line"]}}
            }"#,
        )
        .unwrap();
        let manifest = manifest::load_file(&manifest_path).unwrap();
        let app = test_state_with(vec![manifest]).await;
        let token = app.token.clone();

        // 无票据、错误票据与未认证换票的契约。
        let (status, _, _) =
            call_text(&app, request("GET", "/api/dev/jobs/none/stream", None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _, _) = call_text(
            &app,
            request("GET", "/api/dev/jobs/none/stream?ticket=bogus", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call(
            &app,
            request("POST", "/api/dev/jobs/missing/stream-ticket", Some(&token)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(
            &app,
            request("POST", "/api/dev/jobs/missing/stream-ticket", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // 启动任务后换票。
        let (status, body) = call(
            &app,
            request("POST", "/api/apps/devstream/dev/stream", Some(&token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let job = body["job"].as_str().unwrap().to_string();
        let ticket_uri = format!("/api/dev/jobs/{job}/stream-ticket");
        let (status, body) = call(&app, request("POST", &ticket_uri, Some(&token))).await;
        assert_eq!(status, StatusCode::OK);
        let ticket = body["ticket"].as_str().unwrap().to_string();
        assert_eq!(ticket.len(), 32, "票据应为 32 位十六进制");
        assert_eq!(body["expires_in"], 30);

        // 流本身不带 header，只带票据。
        let uri = format!("/api/dev/jobs/{job}/stream?ticket={ticket}");
        let (status, text, content_type) = call_text(&app, request("GET", &uri, None)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            content_type
                .unwrap_or_default()
                .starts_with("text/event-stream"),
            "SSE content-type 不正确"
        );
        assert!(text.contains("event: line"), "{text}");
        assert!(text.contains("\"first-line\""), "{text}");
        assert!(text.contains("\"second-line\""), "{text}");
        assert!(text.contains("event: done"), "{text}");
        assert!(text.contains("\"exit_code\":0"), "{text}");
        let first = text.find("first-line").unwrap();
        let second = text.find("second-line").unwrap();
        let done = text.find("event: done").unwrap();
        assert!(
            first < second && second < done,
            "增量行必须先于 done: {text}"
        );

        // 票据单次有效。
        let (status, _, _) = call_text(&app, request("GET", &uri, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // 票据绑定 job：换个 job 路径即无效。
        let (_, body) = call(&app, request("POST", &ticket_uri, Some(&token))).await;
        let other = body["ticket"].as_str().unwrap();
        let (status, _, _) = call_text(
            &app,
            request(
                "GET",
                &format!("/api/dev/jobs/other/stream?ticket={other}"),
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let _ = std::fs::remove_dir_all(&dir);
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

    /// FastAPI 风格的接口契约：文档里的每个 path + method 都必须真实存在且要求 token。
    /// 未注册的路由会直接 404，所以「无 token 得到 401」同时证明了文档与路由一致。
    #[tokio::test]
    async fn openapi_document_covers_every_route_and_requires_token() {
        let app = test_state().await;
        let (status, _) = call(&app, request("GET", "/api/openapi.json", None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, doc) = call(&app, request("GET", "/api/openapi.json", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(doc["openapi"], "3.1.0");
        assert_eq!(doc["security"][0]["managerToken"], json!([]));
        assert_eq!(
            doc["components"]["securitySchemes"]["managerToken"]["name"],
            "x-lilyco-token"
        );

        let paths = doc["paths"].as_object().unwrap();
        assert!(paths.len() >= 25, "接口文档只覆盖了 {} 条路径", paths.len());
        let mut operations = 0;
        for (path, item) in paths {
            assert!(path.starts_with("/api/"), "非 API 路径: {path}");
            for method in item.as_object().unwrap().keys() {
                let method = method.to_ascii_uppercase();
                let uri = path
                    .replace("{name}", "manager.health")
                    .replace("{id}", "demo")
                    .replace("{task}", "check")
                    .replace("{job}", "none");
                let (status, _) = call(&app, request(&method, &uri, None)).await;
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri} 未受保护");
                operations += 1;
            }
        }
        assert!(operations >= 30, "只验证了 {operations} 个操作");
    }

    /// 错误路径的状态码契约：404 不存在、400 能力缺失、409 未托管进程、503 二进制缺失。
    /// 这些码是面板与脚本的分支依据，不能被顺手改成 200 或 500。
    #[tokio::test]
    async fn error_paths_use_explicit_status_codes() {
        let mut run_only = sample_manifest();
        run_only.id = "runonly".into();
        run_only.name = "Run Only".into();
        run_only.capabilities.run = true;
        let app = test_state_with(vec![sample_manifest(), run_only]).await;
        let token = app.token.clone();

        for (method, uri, expected) in [
            ("GET", "/api/apps/missing/logs", StatusCode::NOT_FOUND),
            ("GET", "/api/apps/missing/release", StatusCode::NOT_FOUND),
            ("GET", "/api/apps/missing/schema", StatusCode::NOT_FOUND),
            ("GET", "/api/apps/missing/dev", StatusCode::NOT_FOUND),
            ("POST", "/api/apps/missing/start", StatusCode::NOT_FOUND),
            ("POST", "/api/apps/missing/stop", StatusCode::NOT_FOUND),
            ("GET", "/api/dev/jobs/missing/logs", StatusCode::NOT_FOUND),
            ("GET", "/api/projects/999", StatusCode::NOT_FOUND),
            ("POST", "/api/tools/missing.dev/run", StatusCode::NOT_FOUND),
            // 清单存在但没声明能力：明确 400，而不是含糊的 404。
            ("POST", "/api/apps/demo/start", StatusCode::BAD_REQUEST),
            ("GET", "/api/apps/demo/schema", StatusCode::BAD_REQUEST),
            // 没有 dev 规格时任务名也无从匹配，按未知任务处理。
            ("POST", "/api/apps/demo/dev/check", StatusCode::NOT_FOUND),
            // 有 run 能力但二进制不在这台机器上：503 而不是 500 或假成功。
            (
                "POST",
                "/api/apps/runonly/start",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ] {
            let (status, _) = call(&app, request(method, uri, Some(&token))).await;
            assert_eq!(status, expected, "{method} {uri} 状态码不符合契约");
        }

        // 未由管理器启动的应用：停止返回 409，而不是假装成功。
        let (status, _) = call(&app, request("POST", "/api/apps/demo/stop", Some(&token))).await;
        assert_eq!(status, StatusCode::CONFLICT);

        // 没有进程时日志仍是 200 + 空数组，方便面板直接渲染。
        for uri in [
            "/api/apps/demo/logs",
            "/api/apps/demo/logs?limit=0",
            "/api/apps/runonly/logs",
        ] {
            let (status, body) = call(&app, request("GET", uri, Some(&token))).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert_eq!(body["lines"], json!([]), "{uri} 不应有日志");
        }
    }

    #[tokio::test]
    async fn uninstall_unknown_app_is_not_found() {
        let app = test_state().await;
        let (status, _) = call(
            &app,
            request("POST", "/api/apps/missing/uninstall", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[cfg(unix)]
    fn manifest_with(id: &str, binary: &str, capabilities: manifest::Capabilities) -> Manifest {
        Manifest {
            schema_version: 1,
            id: id.into(),
            name: id.into(),
            description: String::new(),
            repository: None,
            binary: binary.into(),
            capabilities,
            run: None,
            mcp: None,
            schema: None,
            release: None,
            dev: None,
            source_dir: None,
        }
    }

    /// 只读元数据接口的响应形状：字段名与只读约束不能漂。
    #[tokio::test]
    async fn ecosystem_and_adapters_have_stable_shape() {
        let app = test_state().await;
        let (status, body) = call(&app, request("GET", "/api/ecosystem", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK);
        let projects = body["projects"].as_array().unwrap();
        assert!(!projects.is_empty(), "生态目录不应为空");
        for project in projects {
            for key in ["name", "url", "category", "integration", "visibility"] {
                assert!(
                    project.get(key).and_then(Value::as_str).is_some(),
                    "生态项目缺字段 {key}: {project}"
                );
            }
        }

        let (status, body) = call(&app, request("GET", "/api/adapters", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK);
        let adapters = body["adapters"].as_array().unwrap();
        assert_eq!(adapters.len(), 2);
        for adapter in adapters {
            assert_eq!(adapter["read_only"], json!(true));
            assert!(adapter["tool"].is_string() && adapter["project"].is_string());
        }
    }

    /// 未配置发布源时：release 仍是 200 + configured:false，安装明确 400。
    #[tokio::test]
    async fn release_reports_unconfigured_and_install_rejects() {
        let app = test_state().await;
        let (status, body) = call(
            &app,
            request("GET", "/api/apps/demo/release", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["release"]["configured"], false);
        assert_eq!(body["release"]["platform"], release::platform_key());

        let (status, body) = call(
            &app,
            request("POST", "/api/apps/demo/install", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "该应用未配置 release 发布源");
    }

    /// `--schema` 只执行清单里的固定 argv，输出的 JSON 原样返回并记录命令数。
    #[cfg(unix)]
    #[tokio::test]
    async fn schema_route_runs_fixed_schema_argv() {
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("lilyco-manager-schema-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("lilyco-schema-test");
        std::fs::write(
            &script,
            "#!/bin/sh\n[ \"$1\" = \"--schema\" ] || exit 9\nprintf '[{\"name\":\"demo\",\"args\":[]}]'\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("LILYCO_MANAGER_BIN_SCHEMAAPP", &script);

        let manifest = manifest_with(
            "schemaapp",
            "lilyco-schema-test",
            manifest::Capabilities {
                run: false,
                mcp: false,
                schema: true,
            },
        );
        let plain = manifest_with("plain", "lilyco-schema-test", Default::default());
        let app = test_state_with(vec![manifest, plain]).await;

        let (status, body) = call(
            &app,
            request("GET", "/api/apps/schemaapp/schema", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["commands"], 1);
        assert_eq!(body["schema"][0]["name"], "demo");

        // 没有 schema 能力的应用直接 400，不会去执行任何东西。
        let (status, _) = call(
            &app,
            request("GET", "/api/apps/plain/schema", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        std::env::remove_var("LILYCO_MANAGER_BIN_SCHEMAAPP");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MCP 聚合：真实 stdio 握手（initialize + tools/list）→ 命名空间化工具；
    /// 配置导出与写入本机 tools.json 走同一次断言。
    #[cfg(unix)]
    #[tokio::test]
    async fn mcp_discovery_and_config_roundtrip() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("lilyco-manager-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("lilyco-mcp-test");
        std::fs::write(
            &script,
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}}' ;;
    *'"tools/list"'*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"回显文本","inputSchema":{"type":"object"}}]}}' ;;
  esac
done
"#,
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("LILYCO_MANAGER_BIN_MCPAPP", &script);
        let config_home = dir.join("config");
        std::env::set_var("XDG_CONFIG_HOME", &config_home);

        let manifest = manifest_with(
            "mcpapp",
            "lilyco-mcp-test",
            manifest::Capabilities {
                run: false,
                mcp: true,
                schema: false,
            },
        );
        let app = test_state_with(vec![manifest]).await;

        let (status, body) = call(&app, request("GET", "/api/mcp/tools", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1, "{body}");
        assert_eq!(tools[0]["name"], "mcpapp__echo");
        assert_eq!(tools[0]["app"], "mcpapp");
        assert_eq!(tools[0]["original_name"], "echo");
        assert_eq!(body["providers"][0]["ok"], true);

        let (status, body) = call(&app, request("GET", "/api/mcp/config", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["servers"]["mcpapp"]["command"],
            script.to_string_lossy().as_ref()
        );
        assert_eq!(body["servers"]["mcpapp"]["args"][0], "--mcp");

        let (status, body) = call(&app, request("POST", "/api/mcp/config", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["written"], true);
        let written = config_home.join("lilyco").join("tools.json");
        let parsed: Value =
            serde_json::from_str(&std::fs::read_to_string(&written).unwrap()).unwrap();
        assert_eq!(parsed["servers"]["mcpapp"]["args"][0], "--mcp");

        std::env::remove_var("LILYCO_MANAGER_BIN_MCPAPP");
        std::env::remove_var("XDG_CONFIG_HOME");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    fn lifecycle_manifest() -> Manifest {
        Manifest {
            schema_version: 1,
            id: "lifecycle".into(),
            name: "Lifecycle".into(),
            description: "生命周期测试应用".into(),
            repository: None,
            binary: "lilyco-lifecycle-test".into(),
            capabilities: manifest::Capabilities {
                run: true,
                mcp: false,
                schema: false,
            },
            run: Some(manifest::RunSpec {
                args: Vec::new(),
                env: BTreeMap::new(),
                url: None,
            }),
            mcp: None,
            schema: None,
            release: None,
            dev: None,
            source_dir: None,
        }
    }

    /// 开发任务只接受清单里的任务名，argv 固定，输出与退出码经 HTTP 读回。
    #[cfg(unix)]
    #[tokio::test]
    async fn dev_task_runs_fixed_argv_and_reports_exit_code() {
        let dir = std::env::temp_dir().join(format!("lilyco-manager-dev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = Manifest {
            schema_version: 1,
            id: "devapp".into(),
            name: "Dev App".into(),
            description: String::new(),
            repository: None,
            binary: "lilyco-missing-dev-binary".into(),
            capabilities: Default::default(),
            run: None,
            mcp: None,
            schema: None,
            release: None,
            dev: Some(manifest::DevSpec {
                tasks: BTreeMap::from([(
                    "check".to_string(),
                    vec!["sh".into(), "-c".into(), "echo dev-ok; exit 3".into()],
                )]),
                env: BTreeMap::new(),
            }),
            source_dir: Some(dir.clone()),
        };
        let app = test_state_with(vec![manifest]).await;

        let (status, body) = call(
            &app,
            request("POST", "/api/apps/devapp/dev/check", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let job = body["job"].as_str().unwrap().to_string();

        let (status, body) = call(
            &app,
            request("GET", "/api/apps/devapp/dev", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["tasks"][0]["name"], "check");

        let mut finished = false;
        for _ in 0..100 {
            let (status, body) = call(
                &app,
                request(
                    "GET",
                    &format!("/api/dev/jobs/{job}/logs"),
                    Some(&app.token),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            if body["job"]["running"] == false {
                assert_eq!(body["job"]["exit_code"], 3);
                assert!(body["job"]["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|line| line.as_str().unwrap_or_default().contains("dev-ok")));
                finished = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(finished, "开发任务未在 5 秒内结束");

        let (status, _) = call(
            &app,
            request("POST", "/api/apps/devapp/dev/nope", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 全生命周期：启动 → 状态 → 日志 → 重复启动拒绝 → 停止 → 重复停止拒绝。
    #[cfg(unix)]
    #[tokio::test]
    async fn app_lifecycle_start_logs_stop() {
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("lilyco-manager-lifecycle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("lilyco-lifecycle-test");
        std::fs::write(
            &script,
            "#!/bin/sh\ntrap 'exit 0' TERM\nprintf 'lifecycle ready\\n'\nwhile :; do sleep 1; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("LILYCO_MANAGER_BIN_LIFECYCLE", &script);

        let app = test_state_with(vec![lifecycle_manifest()]).await;
        let (status, body) = call(
            &app,
            request("POST", "/api/apps/lifecycle/start", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let pid = body["pid"].as_u64().unwrap();
        assert!(pid > 0);

        let mut logs = Vec::new();
        for _ in 0..100 {
            logs = app.supervisor.logs("lifecycle", 20).await;
            if logs.iter().any(|line| line.contains("lifecycle ready")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            logs.iter().any(|line| line.contains("lifecycle ready")),
            "子进程输出未进入日志: {logs:?}"
        );

        let (status, body) = call(&app, request("GET", "/api/apps", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["apps"][0]["status"]["running"], true, "{body}");
        assert_eq!(body["apps"][0]["status"]["pid"], pid);

        let (status, _) = call(
            &app,
            request("POST", "/api/apps/lifecycle/start", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);

        let (status, body) = call(
            &app,
            request("GET", "/api/apps/lifecycle/logs?limit=5", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body["lines"].as_array().unwrap().is_empty());

        let (status, body) = call(
            &app,
            request("POST", "/api/apps/lifecycle/stop", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body["exit_code"].is_number());

        let (status, body) = call(&app, request("GET", "/api/apps", Some(&app.token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["apps"][0]["status"]["running"], false, "{body}");

        // 停止是幂等的：已退出时返回同一条退出码，不重复杀进程。
        let (status, body) = call(
            &app,
            request("POST", "/api/apps/lifecycle/stop", Some(&app.token)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["exit_code"].is_number());

        std::env::remove_var("LILYCO_MANAGER_BIN_LIFECYCLE");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
