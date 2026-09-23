//! # Lilyco Web GUI — axum 服务器 + 内嵌单页控制台
//!
//! 设计基调（取值与组件目录一律看 `DESIGN.md`，那里是唯一一张表；
//! `docs/WEBUI_EVALUATION.md` 是 2026-09-19 的预研记录，其 Pico.css 结论已被自研令牌取代）：
//! - **零外部 UI 依赖**：自研 ~20 KB 设计系统 CSS（明暗两套令牌 + 组件级 container query +
//!   七态），Layui（480KB，95% 能力闲置）退役；vanilla JS 架构保留，按组件拆成 `init*`
//! - **文件输入组件**：Path 参数支持拖拽 / 点击选择，也可让服务端弹本机选择器
//!   （见 `files`）
//! - **安全**：html_escape 全插值点、CSP/nosniff 响应头、动态内容一律 textContent 渲染；
//!   受保护 POST 全部要求 token + 回环 Origin（见 `security`）
//! - **取消**：`POST /cancel/{sid}` 置取消标志（命令侧经 ctx.is_cancelled() 响应）。
//!   句柄只有走 `Registry` 那条执行路才登记得到，所以 `serve_app` 内部就是单命令注册表；
//!   自己传 `RunnerFn` 的 `serve` 没有句柄，页面相应地不画「取消」按钮
//!
//! 端点（与 scripts/acceptance/web_probe.py 验收契约兼容）：
//! - `GET  /`                首页（?cmd= 多命令切换）
//! - `POST /run`             执行 → `{session_id}`
//! - `GET  /progress/{sid}`  SSE 进度流（started/tick/log/telemetry/done/error）
//! - `POST /upload`          文件暂存 → `{path,name,size}`
//! - `POST /pick`            本机原生选择器 → `{path}`
//! - `POST /cancel/{sid}`    请求取消
//!
//! 模块划分（一处一职）：
//! - `security` 回环 Host + Origin + 一次性令牌
//! - `state`    跨端点共享的那一份状态
//! - `render`   schema → HTML 表单（转义纪律集中在此）
//! - `run`      分发、SSE 进度流、取消
//! - `files`    拖拽上传与本机选择器两条取文件的路
//! - `util`     无领域含义的纯函数
//!
//! 对外只有三个符号：[`GuiRenderer`]、[`RunnerFn`]、[`TOKEN_HEADER`]。

mod files;
mod render;
mod run;
mod security;
mod state;
mod util;

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tokio::sync::Mutex;

use lilyco_core::registry::{Handler, RegisteredCommand, Registry};
use lilyco_core::schema::CommandSchema;
use lilyco_core::{App, AppError};

pub use crate::run::RunnerFn;
pub use crate::security::TOKEN_HEADER;

use crate::files::{pick_handler, upload_handler};
use crate::render::index;
use crate::run::{cancel_handler, progress_handler, run_handler};
use crate::security::security_mw;
use crate::state::AppState;
use crate::util::generate_id;

// ── GuiRenderer ───────────────────────────────────────────

pub struct GuiRenderer {
    port: u16,
}

impl GuiRenderer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// 单命令形态：自己给一个 `RunnerFn`。
    ///
    /// 注意这条路的取消：GUI 拿不到 handler 的取消句柄（它藏在 runner 里），
    /// 所以页面的「取消」按钮不会出现。要取消请用 [`Self::serve_app`] / [`Self::serve_registry`]。
    pub async fn serve(&self, schema: CommandSchema, runner: RunnerFn) {
        let state = Arc::new(AppState {
            schema: Arc::new(schema),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner,
            token: generate_id(),
        });
        self.serve_state(state).await;
    }

    /// 多命令形态：把整个 `Registry` 暴露为 Web 控制台
    ///
    /// `GET /?cmd=xxx` 按命令渲染表单（页头下拉切换）；隐藏命令不在下拉中，
    /// 与 CLI help / MCP tools/list 语义一致。执行走 Registry 内的 handler。
    pub async fn serve_registry(&self, registry: Registry) {
        let default_schema = registry
            .visible()
            .next()
            .expect("serve_registry: registry has no visible commands")
            .schema
            .clone();
        let state = Arc::new(AppState {
            schema: Arc::new(default_schema),
            registry: Some(Arc::new(registry)),
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            // registry 模式的执行路径在 run_handler 内按 ?cmd 分发，不走这里
            runner: Arc::new(|_, _| Box::pin(async {})),
            token: generate_id(),
        });
        self.serve_state(state).await;
    }

    async fn serve_state(&self, state: Arc<AppState>) {
        let app = Router::new()
            .route("/", get(index))
            .route("/run", post(run_handler))
            .route("/progress/{id}", get(progress_handler))
            // /upload 单独放宽：axum 默认只放 2 MB，拖一个稍大的文件进去会在传输层
            // 被掐断，页面只剩一句「Failed to fetch」——那句可读的 413 轮不到说出口
            .route(
                "/upload",
                post(upload_handler).layer(DefaultBodyLimit::max(files::MAX_JSON_BODY)),
            )
            .route("/cancel/{id}", post(cancel_handler))
            .route("/pick", post(pick_handler))
            .route_layer(middleware::from_fn_with_state(state.clone(), security_mw))
            .with_state(state);

        // 只监听本机回环地址：本地 GUI 工具不需要暴露到局域网
        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", self.port))
            .await
            .unwrap();
        let url = format!("http://localhost:{}", self.port);
        eprintln!("Lilyco GUI ready: {url}");

        // Auto-open browser
        if let Err(e) = webbrowser::open(&url) {
            eprintln!("  (could not open browser: {e})");
        }

        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                tokio::signal::ctrl_c().await.ok();
                eprintln!("\nShutting down...");
            })
            .await
            .unwrap();
    }

    /// Serve with a concrete `App` type. Auto-wires `from_args` + `run`
    /// and streams progress events to the browser via SSE.
    /// Eliminates the need to manually construct a `RunnerFn` closure.
    ///
    /// 内部走 `serve_registry`（单命令的注册表）而不是 `serve` + `RunnerFn`：
    /// 只有那条路会把 executor 的取消句柄登记进 `AppState::cancels`，
    /// 而 `#[app]` 的单命令二进制是这套 GUI 最主要的用户形态 ——
    /// 走 RunnerFn 的话页面那颗「取消」每次只回一句 404，是个假承诺。
    pub async fn serve_app<A>(&self, schema: CommandSchema)
    where
        A: App + Send + 'static,
    {
        // 执行语义交给 core::executor（与 CLI / TUI / MCP 共享同一宿主）
        let handler: Handler = Arc::new(move |ctx, args| {
            let obj = args
                .as_object()
                .ok_or_else(|| AppError::InvalidArg("args must be a JSON object".into()))?;
            let map: HashMap<String, serde_json::Value> =
                obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let app = A::from_args(&map)?;
            app.run(ctx)
        });
        let mut registry = Registry::new();
        registry
            .register(RegisteredCommand::new(schema.name.clone(), schema).with_handler(handler))
            .expect("serve_app: 空注册表不会拒绝它的第一条命令");
        self.serve_registry(registry).await;
    }
}
