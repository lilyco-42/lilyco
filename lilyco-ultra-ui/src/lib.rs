//! # Lilyco Ultra UI
//!
//! Excel 风格的声明式 JSON → React UI 生成器。
//! 验证 lilyco 框架的极简 UI 生成能力：无需 Rust 代码，只需写 JSON 即可生成专业 React 界面。

mod builder;
pub mod generator;
pub mod spec;

use axum::{
    extract::State,
    response::Html,
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
            .route("/api/render", post(render_handler))
            .route("/api/validate", post(validate_handler))
            .route("/api/example", get(example_handler))
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
