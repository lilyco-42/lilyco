//! # Lilyco Ultra UI
//!
//! Excel 风格的声明式 JSON → React UI 生成器。
//! 验证 lilyco 框架的极简 UI 生成能力：无需 Rust 代码，只需写 JSON 即可生成专业 React 界面。

mod blocks;
mod builder;
mod designer;
pub mod exec;
pub mod generator;
pub mod graph;
mod graph_page;
pub mod spec;

use axum::{
    extract::State,
    http::header::{CACHE_CONTROL, CONTENT_TYPE},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde_json::Value;
use std::sync::Arc;

pub use spec::{ElementSpec, UiSpec, WindowSize, WindowSpec};

/// Ultra UI 服务器
pub struct UltraUiServer {
    port: u16,
}

impl UltraUiServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// 启动服务器，自动打开浏览器
    pub async fn serve(&self) {
        let app = Router::new()
            .route("/", get(builder_page))
            .route("/graph", get(graph_page_handler))
            .route("/designer", get(designer_page))
            .route("/blocks", get(blocks_page))
            .route("/api/designer/save", post(designer_save_handler))
            .route("/api/designer/load", get(designer_load_handler))
            .route("/api/designer/export", post(designer_export_handler))
            .route("/assets/litegraph.js", get(litegraph_js))
            .route("/assets/grapesjs.min.js", get(grapesjs_js))
            .route("/api/render", post(render_handler))
            .route("/api/validate", post(validate_handler))
            .route("/api/graph/validate", post(graph_validate_handler))
            .route("/api/graph/plan", post(graph_plan_handler))
            .route("/api/graph/layout", post(graph_layout_handler))
            .route("/api/graph/run", post(graph_run_handler))
            .route("/api/graph/example", get(graph_example_handler))
            .route("/api/graph/commands", get(graph_commands_handler))
            .route("/api/graph/save", post(graph_save_handler))
            .route("/api/graph/load", get(graph_load_handler))
            .route("/api/example", get(example_handler))
            .route("/api/example/calculator", get(calculator_example_handler))
            .with_state(Arc::new(()));

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await
            .unwrap();
        let url = format!("http://localhost:{}", self.port);
        eprintln!("Lilyco Ultra UI ready: {url}");
        let _ = webbrowser::open(&url);
        axum::serve(listener, app).await.unwrap();
    }
}

async fn builder_page() -> Html<String> {
    Html(builder::builder_html())
}

async fn graph_page_handler() -> Html<String> {
    Html(graph_page::graph_html())
}

async fn designer_page() -> Html<String> {
    Html(designer::designer_html())
}

async fn blocks_page() -> Html<String> {
    Html(blocks::blocks_html())
}

/// 设计稿落盘位置：`~/.lilyco/designer.json`
fn designer_store_path() -> std::path::PathBuf {
    let base = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    base.join(".lilyco").join("designer.json")
}

async fn designer_save_handler(Json(payload): Json<designer::DesignDoc>) -> impl IntoResponse {
    let path = designer_store_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body = match serde_json::to_vec_pretty(&payload) {
        Ok(b) => b,
        Err(e) => return err_response(400, &format!("序列化失败: {e}")),
    };
    match std::fs::write(&path, body) {
        Ok(_) => Json(serde_json::json!({ "ok": true, "path": path.display().to_string() }))
            .into_response(),
        Err(e) => err_response(500, &format!("写入失败: {e}")),
    }
}

async fn designer_load_handler() -> impl IntoResponse {
    match std::fs::read_to_string(designer_store_path()) {
        Ok(text) => match serde_json::from_str::<designer::DesignDoc>(&text) {
            Ok(doc) => Json(serde_json::json!({ "ok": true, "doc": doc })).into_response(),
            Err(e) => err_response(400, &format!("解析失败: {e}")),
        },
        Err(_) => err_response(404, "尚无保存的设计稿"),
    }
}

async fn designer_export_handler(Json(payload): Json<designer::DesignDoc>) -> impl IntoResponse {
    match payload.to_spec() {
        Ok(spec) => Json(serde_json::json!({
            "ok": true,
            "spec": spec,
            "html": generator::generate_react_html(&spec),
        }))
        .into_response(),
        Err(e) => err_response(400, &e),
    }
}

/// playground 内置的命令表（真实场景由宿主传入 Registry）
fn known_args(command: &str) -> Option<Vec<String>> {
    match command {
        "count" => Some(vec!["steps".into()]),
        "echo" => Some(vec!["text".into()]),
        "compress" => Some(vec!["input".into(), "quality".into()]),
        _ => None,
    }
}

async fn graph_validate_handler(Json(payload): Json<graph::GraphSpec>) -> Json<Value> {
    let errors = graph::validate_with(&payload, known_args);
    Json(serde_json::json!({ "valid": errors.is_empty(), "errors": errors }))
}

async fn graph_plan_handler(
    Json(payload): Json<graph::GraphSpec>,
) -> impl axum::response::IntoResponse {
    let errors = graph::validate_with(&payload, known_args);
    if !errors.is_empty() {
        return err_response(400, &format!("图校验失败: {errors:?}"));
    }
    match graph::plan(&payload) {
        Ok(plan) => Json(serde_json::json!({ "ok": true, "plan": plan })).into_response(),
        Err(e) => err_response(400, &e.to_string()),
    }
}

async fn graph_example_handler() -> Json<Value> {
    Json(serde_json::to_value(graph::example_graph()).unwrap())
}

/// 画布需要的命令 → 参数名映射（决定每个节点左侧的输入端口）
async fn graph_commands_handler() -> Json<Value> {
    Json(serde_json::json!({
        "count": ["steps"],
        "echo": ["text"],
        "compress": ["input", "quality"],
    }))
}

/// 随二进制内嵌的 LiteGraph（MIT），页面同源加载，无外部请求
async fn litegraph_js() -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, "application/javascript; charset=utf-8"),
            (CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        include_str!("../assets/litegraph.js"),
    )
}

/// 随二进制内嵌的 GrapesJS 0.22.5（BSD-3-Clause），设计器同源加载
async fn grapesjs_js() -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, "application/javascript; charset=utf-8"),
            (CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        include_str!("../assets/grapesjs.min.js"),
    )
}

async fn graph_layout_handler(Json(payload): Json<graph::GraphSpec>) -> impl IntoResponse {
    match graph::layout(&payload) {
        Ok(pos) => Json(serde_json::json!({
            "ok": true,
            "nodes": pos
                .iter()
                .map(|(id, x, y)| serde_json::json!({ "id": id, "x": x, "y": y }))
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(e) => err_response(400, &e.to_string()),
    }
}

async fn graph_run_handler(Json(payload): Json<graph::GraphSpec>) -> impl IntoResponse {
    match exec::run(&payload, demo_handler) {
        Ok(result) => Json(serde_json::json!({ "ok": true, "result": result })).into_response(),
        Err(e) => err_response(400, &e.to_string()),
    }
}

/// 图 JSON 的落盘位置：`~/.lilyco/graph.json`（唯一契约，前端只做渲染）
fn graph_store_path() -> std::path::PathBuf {
    let base = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    base.join(".lilyco").join("graph.json")
}

async fn graph_save_handler(Json(payload): Json<graph::GraphSpec>) -> impl IntoResponse {
    let path = graph_store_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body = match serde_json::to_vec_pretty(&payload) {
        Ok(b) => b,
        Err(e) => return err_response(400, &format!("序列化失败: {e}")),
    };
    match std::fs::write(&path, body) {
        Ok(_) => Json(serde_json::json!({ "ok": true, "path": path.display().to_string() }))
            .into_response(),
        Err(e) => err_response(500, &format!("写入失败: {e}")),
    }
}

async fn graph_load_handler() -> impl IntoResponse {
    let path = graph_store_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<graph::GraphSpec>(&text) {
            Ok(spec) => Json(serde_json::json!({ "ok": true, "graph": spec })).into_response(),
            Err(e) => err_response(400, &format!("解析失败: {e}")),
        },
        Err(_) => err_response(404, "尚无保存的图"),
    }
}

/// playground 的演示命令实现（真实场景换成 Registry handler）
fn demo_handler(command: &str, args: &Value) -> Result<Value, String> {
    match command {
        "count" => Ok(serde_json::json!({
            "value": args.get("steps").and_then(Value::as_u64).unwrap_or(0),
        })),
        "echo" => Ok(serde_json::json!({
            "text": args.get("text").cloned().unwrap_or(Value::Null),
        })),
        "compress" => Ok(serde_json::json!({
            "quality": args.get("quality").cloned().unwrap_or(serde_json::json!(75)),
        })),
        other => Err(format!("未知命令: {other}")),
    }
}

async fn render_handler(Json(payload): Json<Value>) -> impl axum::response::IntoResponse {
    match serde_json::from_value::<UiSpec>(payload) {
        Ok(spec) => {
            let errors = spec.validate();
            if !errors.is_empty() {
                return err_response(400, &errors.join("; "));
            }
            let html = generator::generate_react_html(&spec);
            ok_html(html)
        }
        Err(e) => err_response(400, &format!("JSON 解析失败: {}", e)),
    }
}

async fn validate_handler(Json(payload): Json<Value>) -> Json<Value> {
    match serde_json::from_value::<UiSpec>(payload) {
        Ok(spec) => {
            let errors = spec.validate();
            Json(serde_json::json!({ "valid": errors.is_empty(), "errors": errors }))
        }
        Err(e) => {
            Json(serde_json::json!({ "valid": false, "errors": [format!("JSON 解析失败: {}", e)] }))
        }
    }
}

async fn example_handler(State(_): State<Arc<()>>) -> Json<Value> {
    let json = spec::default_example_json();
    let val: Value = serde_json::from_str(&json).unwrap();
    Json(val)
}

async fn calculator_example_handler(State(_): State<Arc<()>>) -> Json<Value> {
    let json = spec::calculator_example_json();
    Json(serde_json::from_str(&json).unwrap())
}

fn err_response(code: u16, msg: &str) -> axum::response::Response {
    axum::response::Response::builder()
        .status(code)
        .header("Content-Type", "application/json; charset=utf-8")
        .body(axum::body::Body::from(
            serde_json::json!({ "error": msg }).to_string(),
        ))
        .unwrap()
}

fn ok_html(html: String) -> axum::response::Response {
    axum::response::Response::builder()
        .header("Content-Type", "text/html; charset=utf-8")
        .body(axum::body::Body::from(html))
        .unwrap()
}
