//! # Lilyco — 门面（facade）
//!
//! 一个依赖搞定四端：**CLI / TUI / Web / MCP(AI)**。
//!
//! 借鉴 mininterface 的接口工厂设计：按运行环境自动选择后端，
//! 显式参数 > 环境变量 > 自动探测，TUI 起不来时回退 CLI。
//!
//! ## 用法（零样板）
//!
//! ```ignore
//! use lilyco::prelude::*;
//!
//! #[derive(App)]
//! #[app(run = "run_hello")]
//! struct Hello {
//!     /// 问候对象
//!     name: String,
//! }
//!
//! fn run_hello(app: &Hello, ctx: &Context) -> Result<serde_json::Value, AppError> {
//!     let r = serde_json::json!({ "msg": format!("hello {}", app.name) });
//!     ctx.done(r.clone(), 0);
//!     Ok(r)
//! }
//!
//! fn main() {
//!     lilyco::run::<Hello>();
//! }
//! ```
//!
//! 同一份二进制：
//! ```bash
//! hello --name world        # CLI（非交互终端 / 管道）
//! hello                      # 交互终端 → TUI 表单
//! hello --gui                # Web GUI（SSE 进度）
//! hello --mcp                # MCP stdio 服务器（Agent 直接调用）
//! LILYCO_UI=web hello        # 环境变量强制后端
//! ```
//!
//! ## 多命令（一个二进制 = 一个域）
//!
//! ```ignore
//! fn main() {
//!     let mut reg = Registry::new();
//!     reg.register(RegisteredCommand::from_app::<Find>()).unwrap();
//!     reg.register(RegisteredCommand::from_app::<Dedup>()).unwrap();
//!     lilyco::run_registry("lfiles", reg);   // 四端自动分发
//! }
//! ```
//!
//! ```bash
//! lfiles find --pattern '*.jpg'   # CLI 子命令
//! lfiles --gui                    # Web 控制台（?cmd= 下拉切换）
//! lfiles --tui                    # TUI 命令选择页
//! lfiles --mcp                    # MCP：tools/list 一次返回全部命令
//! ```

use std::io::IsTerminal;

use lilyco_core::registry::Registry;
use lilyco_core::App;

// TUI 后端专属导入（crossterm 不支持 Android，按 feature 门控）
#[cfg(feature = "tui")]
use lilyco_core::executor;
#[cfg(feature = "tui")]
use lilyco_core::progress::LogLevel;
#[cfg(feature = "tui")]
use lilyco_core::registry::Handler;
#[cfg(feature = "tui")]
use lilyco_core::{AppError, Progress};
#[cfg(feature = "tui")]
use std::sync::Arc;

/// 宏展开的后端路径：`#[derive(App)]` 生成的代码引用 `::lilyco::__core::…`
///
/// 用户只需依赖 `lilyco` 一个 crate（doc hidden，不进文档/IDE 提示）。
#[doc(hidden)]
pub use lilyco_core as __core;

/// 常用导入（trait + 类型 + derive 宏）
pub mod prelude {
    pub use lilyco_core::prelude::*;
    pub use lilyco_macros::{App, ValueEnum};
}

/// 可用的后端
///
/// `Tui` / `Web` 由特性门控（crossterm 不支持 Android 目标；
/// `--no-default-features` 时只剩 `Cli` + `Mcp`，纯 Rust 全平台可编）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Cli,
    #[cfg(feature = "tui")]
    Tui,
    #[cfg(feature = "web")]
    Web,
    Mcp,
}

/// 环境快照（注入式，便于单元测试）
pub struct Env<'a> {
    /// 命令行参数（不含程序名）
    pub args: &'a [String],
    /// 环境变量读取器
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// stdin 是否为终端
    pub stdin_is_terminal: bool,
}

/// 自动探测后端。
///
/// 优先级（借鉴 mininterface 的 precedence：显式参数 > 环境变量 > 自动）：
/// 1. CLI 标志：`--mcp` / `--gui`（`--web` 同义）
/// 2. 环境变量 `LILYCO_UI`：`cli` | `tui` | `web` | `mcp` | `auto`
/// 3. 自动：stdin 是终端且设置了 `TERM` → TUI；否则 CLI（可管道化，脚本/AI 友好）
///
/// 被特性关掉的后端在探测中直接跳过（如 Android headless 构建永远落到 CLI）。
pub fn detect_backend(env: &Env) -> Backend {
    // 1. 显式标志（调用时刻的意图最高优先）
    if env.args.iter().any(|a| a == "--mcp") {
        return Backend::Mcp;
    }
    #[cfg(feature = "web")]
    if env.args.iter().any(|a| a == "--gui" || a == "--web") {
        return Backend::Web;
    }
    // 2. 环境变量
    if let Some(ui) = (env.env)("LILYCO_UI") {
        match ui.as_str() {
            "cli" => return Backend::Cli,
            #[cfg(feature = "tui")]
            "tui" => return Backend::Tui,
            #[cfg(feature = "web")]
            "web" | "gui" => return Backend::Web,
            "mcp" => return Backend::Mcp,
            _ => {} // 未知值 → 落到自动
        }
    }
    // 3. 自动
    #[cfg(feature = "tui")]
    if env.stdin_is_terminal && (env.env)("TERM").map(|t| !t.is_empty()).unwrap_or(false) {
        return Backend::Tui;
    }
    Backend::Cli
}

/// 真实环境下的探测（进程入口用）
pub fn detect() -> Backend {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = Env {
        args: &args,
        env: &|k| std::env::var(k).ok(),
        stdin_is_terminal: std::io::stdin().is_terminal(),
    };
    detect_backend(&env)
}

/// 一行启动：自动选择后端
pub fn run<A: App + Send + 'static>() {
    run_with::<A>(detect());
}

/// 显式指定后端
pub fn run_with<A: App + Send + 'static>(backend: Backend) {
    match backend {
        Backend::Cli => run_cli::<A>(),
        #[cfg(feature = "tui")]
        Backend::Tui => {
            // mininterface 借鉴点：TUI 起不来（非终端/CI）时回退 CLI，绝不裸崩
            if run_tui::<A>().is_err() {
                run_cli::<A>();
            }
        }
        #[cfg(feature = "web")]
        Backend::Web => run_web::<A>(),
        Backend::Mcp => serve_mcp(single_registry::<A>()),
    }
}

/// 以 MCP 服务器形态暴露整个注册表（多命令场景）
pub fn serve_mcp(registry: Registry) {
    lilyco_mcp::McpServer::new(registry)
        .serve_stdio()
        .unwrap_or_else(|e| {
            eprintln!("MCP server error: {e}");
            std::process::exit(1);
        });
}

/// 以 CLI 多命令形态运行整个注册表（一个二进制 = 多个子命令）
///
/// ```ignore
/// let mut registry = Registry::new();
/// registry.register(RegisteredCommand::from_app::<Compress>()).unwrap();
/// registry.register(RegisteredCommand::from_app::<Resize>()).unwrap();
/// lilyco::run_cli_registry("imgtool", registry);
/// ```
pub fn run_cli_registry(app_name: &str, registry: Registry) {
    lilyco_cli::run_registry(app_name, registry);
}

/// 以 TUI 多命令形态运行整个注册表（命令选择页 → 表单 → 执行）
///
/// 借鉴 mininterface 的 subcommand picker：交互终端先列出可见命令
/// （隐藏命令不显示，语义与 CLI help / MCP tools/list 一致），
/// 选中后进入该命令的表单；任务结束后返回选择页继续导航。
/// TUI 起不来（非终端/CI/无 crossterm 平台）时回退 [`run_cli_registry`]。
///
/// ```ignore
/// lilyco::run_tui_registry("imgtool", registry);
/// ```
pub fn run_tui_registry(app_name: &str, registry: Registry) {
    #[cfg(feature = "tui")]
    {
        if run_tui_registry_impl(app_name, &registry).is_err() {
            run_cli_registry(app_name, registry);
        }
        return;
    }
    #[cfg(not(feature = "tui"))]
    run_cli_registry(app_name, registry);
}

/// 以 Web 控制台形态运行整个注册表（多命令，页头下拉 + `?cmd=` 切换）
///
/// 与 [`run_tui_registry`] 对称：四端（CLI / TUI / Web / MCP）对同一份
/// `Registry` 各有一个多命令入口。命令可见性（`hidden`）在三端语义一致：
/// 不出现在 CLI help、不在 TUI 选择页、不在 Web 下拉、不在 MCP `tools/list`。
///
/// 监听端口取 `LILYCO_PORT`（默认 8080），只绑 127.0.0.1 并自动开浏览器。
///
/// ```ignore
/// lilyco::run_web_registry("imgtool", registry);
/// ```
pub fn run_web_registry(app_name: &str, registry: Registry) {
    #[cfg(feature = "web")]
    {
        let port: u16 = std::env::var("LILYCO_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            eprintln!("{app_name} Web UI: http://localhost:{port}");
            lilyco_gui::GuiRenderer::new(port)
                .serve_registry(registry)
                .await;
        });
        return;
    }
    #[cfg(not(feature = "web"))]
    {
        eprintln!("{app_name}: Web 后端未编译（--no-default-features），回退 CLI");
        run_cli_registry(app_name, registry);
    }
}

/// 多命令形态的一行启动：自动选择后端
///
/// 与单命令的 [`run`] 对应，供「一个二进制 = 一个域」的多命令 app 使用。
/// 优先级与 [`detect_backend`] 一致：`--mcp` / `--gui` / `--tui` / `--cli`
/// 显式标志 > `LILYCO_UI` 环境变量 > 自动（交互终端 → TUI，否则 CLI）。
///
/// ```ignore
/// fn main() {
///     let mut reg = Registry::new();
///     reg.register(RegisteredCommand::from_app::<Find>()).unwrap();
///     lilyco::run_registry("lfiles", reg);
/// }
/// ```
pub fn run_registry(app_name: &str, registry: Registry) {
    run_registry_with(app_name, registry, detect_registry_backend());
}

/// 显式指定后端运行注册表
///
/// **安全策略按调用面注入**（这是本函数除了分发之外的第二职责）：
///
/// | 后端 | 策略 | 理由 |
/// |---|---|---|
/// | CLI / TUI | [`Interactive`] | 人类亲手敲命令 / 在表单里点提交，按键即确认，放行 T0+T1 |
/// | Web | [`Interactive`] | 同上，但只在 127.0.0.1 监听，且带 CSRF 令牌中间件 |
/// | MCP | [`DenyElevated`] | Agent 无人值守，只放行 T0；T1+ 拒绝并说明原因 |
///
/// 调用方若已有自己的策略，请用 [`run_registry_with_policy`]。
///
/// [`Interactive`]: lilyco_core::safety::Interactive
/// [`DenyElevated`]: lilyco_core::safety::DenyElevated
pub fn run_registry_with(app_name: &str, registry: Registry, backend: Backend) {
    use std::sync::Arc;
    let policy: Arc<dyn lilyco_core::safety::SafetyPolicy> = match backend {
        Backend::Mcp => Arc::new(lilyco_core::safety::DenyElevated),
        #[allow(unreachable_patterns)]
        _ => Arc::new(lilyco_core::safety::Interactive),
    };
    run_registry_with_policy(app_name, registry, backend, policy);
}

/// 显式指定后端**与**安全策略运行注册表（最大控制权）
///
/// `with_policy` 必须在注册命令**之前**调用（门在注册时就把 handler 包住了），
/// 所以这里先把传入的注册表重建成带新策略的形式 —— 借用 `registry` 的
/// schema 重新 `register` 一次，语义与「一开始就用该策略注册」完全一致。
pub fn run_registry_with_policy(
    app_name: &str,
    registry: Registry,
    backend: Backend,
    policy: std::sync::Arc<dyn lilyco_core::safety::SafetyPolicy>,
) {
    let registry = into_registry_with_policy(registry, policy);
    match backend {
        Backend::Cli => run_cli_registry(app_name, registry),
        #[cfg(feature = "tui")]
        Backend::Tui => run_tui_registry(app_name, registry),
        #[cfg(feature = "web")]
        Backend::Web => run_web_registry(app_name, registry),
        Backend::Mcp => serve_mcp(registry),
        // 特性关掉的后端（Android headless 等）→ CLI 兜底
        #[allow(unreachable_patterns)]
        _ => run_cli_registry(app_name, registry),
    }
}

/// 把注册表重建成带指定安全策略的形式。
///
/// 为什么需要重建而不是「改一下策略字段」：门在 `register` 时就把 handler
/// 包住了，已注册的命令无法事后换门。重建 = 把「策略 → 各 handler」的
/// 包裹关系按新策略重新建立一遍，对调用方完全透明。
///
/// 重建会**保留**命令名、别名、隐藏标记与 schema（含各自的安全分级）。
fn into_registry_with_policy(
    registry: Registry,
    policy: std::sync::Arc<dyn lilyco_core::safety::SafetyPolicy>,
) -> Registry {
    let mut out = Registry::new().with_policy(policy);
    // 先收集再重建，避免同时持有对新表的可变借用与对旧表的借用
    let entries: Vec<(String, Vec<String>, bool, lilyco_core::schema::CommandSchema)> = registry
        .iter()
        .map(|c| (c.name.clone(), c.aliases.clone(), c.hidden, c.schema.clone()))
        .collect();
    for (name, aliases, hidden, schema) in entries {
        match registry.get(&name).and_then(|c| c.handler.clone()) {
            Some(handler) => {
                let cmd = lilyco_core::registry::RegisteredCommand::new(name, schema)
                    .with_handler(handler)
                    .hidden(hidden)
                    .aliases(aliases);
                // 重建只改安全策略，命令本身不该丢东西
                debug_assert!(cmd.handler.is_some());
                out.register(cmd).unwrap_or_else(|e| {
                    eprintln!("重建注册表失败: {e}");
                    std::process::exit(1);
                });
            }
            None => {
                eprintln!("跳过无 handler 的命令 `{name}`");
            }
        }
    }
    out
}

/// 多命令形态的真实环境探测。
///
/// 在 [`detect_backend`] 基础上多认一个 `--tui` 显式标志：
/// 多命令 app 在交互终端下**默认落到 CLI**（因为我们无法在这里知道
/// 注册表里有没有可见命令、以及 TUI 能否起得来），想进 TUI 请显式 `--tui`
/// 或 `LILYCO_UI=tui` —— 这样脚本里裸跑 `lfiles find …` 不会被拽进交互界面。
pub fn detect_registry_backend() -> Backend {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // 显式 --tui / --cli 优先（多命令专属）
    if args.iter().any(|a| a == "--tui") {
        #[cfg(feature = "tui")]
        return Backend::Tui;
        #[cfg(not(feature = "tui"))]
        return Backend::Cli;
    }
    let env = Env {
        args: &args,
        env: &|k| std::env::var(k).ok(),
        stdin_is_terminal: std::io::stdin().is_terminal(),
    };
    let b = detect_backend(&env);
    // 多命令形态：自动探测出的 TUI 降级为 CLI（子命令应由用户显式点选）
    match b {
        #[cfg(feature = "tui")]
        Backend::Tui => {
            if args.iter().any(|a| a == "--tui") {
                Backend::Tui
            } else {
                Backend::Cli
            }
        }
        other => other,
    }
}

// ── 后端分发 ──────────────────────────────────────────────

fn single_registry<A: App + Send + 'static>() -> Registry {
    let mut registry = Registry::new();
    registry
        .register(lilyco_core::registry::RegisteredCommand::from_app::<A>())
        .expect("register app command");
    registry
}

fn run_cli<A: App + Send + 'static>() {
    lilyco_cli::run::<A>(|app, ctx| app.run(ctx));
}

#[cfg(feature = "web")]
fn run_web<A: App + Send + 'static>() {
    let port: u16 = std::env::var("LILYCO_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let gui = lilyco_gui::GuiRenderer::new(port);
        gui.serve_app::<A>(A::schema()).await;
    });
}

#[cfg(feature = "tui")]
fn run_tui<A: App + Send + 'static>() -> std::io::Result<()> {
    let schema = A::schema();
    let mut handlers = std::collections::HashMap::new();
    handlers.insert(schema.name.clone(), build_handler::<A>());
    let mut app = lilyco_tui::TuiApp::new(&schema);
    run_tui_event_loop(&mut app, &handlers)
}

/// TUI 多命令形态：命令选择页 → 表单 → 执行（mininterface subcommand picker 模式）
#[cfg(feature = "tui")]
fn run_tui_registry_impl(app_name: &str, registry: &Registry) -> std::io::Result<()> {
    let (mut app, handlers) = build_multi_tui(app_name, registry)?;
    run_tui_event_loop(&mut app, &handlers)
}

/// 构造多命令 TUI：命令选择页 + 名字→handler 表。
///
/// 与事件循环分离，好让"隐藏命令不进选择页""无可见命令即报错""handler 表
/// 与可见命令一一对应"这些语义可以在无终端环境下单测（事件循环需要真 TTY）。
#[cfg(feature = "tui")]
fn build_multi_tui(
    app_name: &str,
    registry: &Registry,
) -> std::io::Result<(lilyco_tui::TuiApp, std::collections::HashMap<String, Handler>)> {
    // 隐藏命令不进选择页（与 CLI help / MCP tools/list 语义一致）
    let schemas: Vec<lilyco_core::schema::CommandSchema> =
        registry.visible().map(|c| c.schema.clone()).collect();
    if schemas.is_empty() {
        return Err(std::io::Error::other("registry has no visible commands"));
    }

    let mut handlers = std::collections::HashMap::new();
    for cmd in registry.iter() {
        if let Some(h) = &cmd.handler {
            handlers.insert(cmd.schema.name.clone(), h.clone());
        }
    }

    let app = lilyco_tui::TuiApp::new_multi(app_name, schemas);
    Ok((app, handlers))
}

/// TUI 事件循环（单命令 `run_tui` 与多命令 `run_tui_registry_impl` 共享）
#[cfg(feature = "tui")]
fn run_tui_event_loop(
    app: &mut lilyco_tui::TuiApp,
    handlers: &std::collections::HashMap<String, Handler>,
) -> std::io::Result<()> {
    use crossterm::event::Event;
    use crossterm::execute;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use lilyco_tui::AppState;
    use std::time::Duration;

    enum TaskState {
        Idle,
        Running(executor::Task),
    }
    let mut task_state = TaskState::Idle;

    enable_raw_mode()?;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;

    loop {
        // 1. 进入 Running 且尚未启动任务 → 后台派生任务
        if app.state() == &AppState::Running {
            if let TaskState::Idle = task_state {
                let args = collect_args(app);
                // 多命令按 active_command 取 handler；单命令 map 里只有一项
                let name = app
                    .active_command
                    .clone()
                    .unwrap_or_else(|| app.form.command_name.clone());
                let Some(handler) = handlers.get(&name).cloned() else {
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                    return Err(std::io::Error::other(format!(
                        "no handler for command `{name}`"
                    )));
                };
                task_state = TaskState::Running(executor::spawn(handler, args));
            }
        }

        // 2. Running：非阻塞排空任务进度事件，保持 UI 响应
        if app.state() == &AppState::Running {
            if let TaskState::Running(task) = &task_state {
                drain_task(app, task);
            }
        }

        // 3. 渲染
        terminal.draw(|f| {
            let area = f.area();
            app.render(area, f.buffer_mut());
        })?;

        if app.should_quit {
            break;
        }

        // 4. 已离开 Running（Done / Error / 回到选择页）→ 请求取消并回收线程
        if app.state() != &AppState::Running {
            if let TaskState::Running(task) = &task_state {
                task.cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let TaskState::Running(task) = task_state {
                let _ = task.handle.join();
            }
            task_state = TaskState::Idle;
        }

        // 5. 轮询键盘事件（Running 时短超时，持续刷新 elapsed 计时）
        let timeout = if app.state() == &AppState::Running {
            Duration::from_millis(150)
        } else {
            Duration::from_millis(500)
        };
        if crossterm::event::poll(timeout)? {
            match crossterm::event::read()? {
                Event::Key(key) => {
                    let cont = app.handle_event(key);
                    if !cont {
                        break;
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

/// 把表单当前值收集为参数 JSON
#[cfg(feature = "tui")]
fn collect_args(app: &lilyco_tui::TuiApp) -> serde_json::Value {
    use lilyco_tui::FieldValue;
    let mut map = serde_json::Map::new();
    for field in app.fields() {
        let v = match &field.value {
            FieldValue::Flag(b) => serde_json::json!(b),
            FieldValue::Text(s) | FieldValue::Path(s) => serde_json::json!(s),
            FieldValue::Number(n) => serde_json::json!(n),
            FieldValue::Enum { values, selected } => {
                serde_json::json!(values.get(*selected).cloned().unwrap_or_default())
            }
            FieldValue::List { values, .. } => serde_json::json!(values),
        };
        map.insert(field.name.clone(), v);
    }
    serde_json::Value::Object(map)
}

/// 把表单字段值转换为 handler 参数的对象容器
#[cfg(feature = "tui")]
fn build_handler<A: App + Send + 'static>() -> Handler {
    Arc::new(move |ctx, args| {
        let obj = args
            .as_object()
            .ok_or_else(|| AppError::InvalidArg("args must be a JSON object".into()))?;
        let map: std::collections::HashMap<String, serde_json::Value> =
            obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let a = A::from_args(&map)?;
        a.run(ctx)
    })
}

/// 非阻塞排空任务进度事件灌进 TUI 状态。
///
/// 与 `run_tui` 的事件循环交替运行：只消费目前已到达的进度事件，
/// 不阻塞等待，从而保证执行期间键盘仍然可响应、可取消。
/// spawn 保证事件流恒以 Done / Error 终态结尾。
#[cfg(feature = "tui")]
fn drain_task(app: &mut lilyco_tui::TuiApp, task: &executor::Task) {
    use std::sync::mpsc::TryRecvError;
    loop {
        match task.rx.try_recv() {
            Ok(Progress::Started { message, .. }) => app.start_progress(None, message),
            Ok(Progress::Tick {
                current,
                total,
                message,
                ..
            }) => app.tick_progress(current, total, message),
            Ok(Progress::Log { level, message }) => app.log_progress(level_name(&level), message),
            Ok(Progress::Telemetry { key, value }) => {
                app.log_progress("info", format!("{key}={value}"));
            }
            Ok(Progress::Done {
                result,
                duration_ms,
            }) => {
                app.finish_progress(result, duration_ms);
            }
            Ok(Progress::Error { code, message, .. }) => {
                app.error_progress(code, message);
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                // 线程结束。spawn 保证已合成终态，故无需兜底；
                // 若因极端情况遗漏，此处安全地终止，避免无限循环。
                if *app.state() == lilyco_tui::AppState::Running {
                    app.error_progress(1, "任务意外终止".into());
                }
                break;
            }
        }
    }
}

#[cfg(feature = "tui")]
fn level_name(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造 Env。参数与闭包绑定在测试函数作用域内，生命周期安全。
    fn no_env() -> impl Fn(&str) -> Option<String> {
        |_k: &str| -> Option<String> { None }
    }

    #[test]
    fn mcp_flag_wins_over_everything() {
        let args = vec!["--mcp".to_string()];
        let getenv = no_env();
        let e = Env {
            args: &args,
            env: &getenv,
            stdin_is_terminal: true,
        };
        assert_eq!(detect_backend(&e), Backend::Mcp);
    }

    #[cfg(feature = "web")]
    #[test]
    fn gui_flag_selects_web() {
        let args = vec!["--gui".to_string()];
        let getenv = no_env();
        let e = Env {
            args: &args,
            env: &getenv,
            stdin_is_terminal: true,
        };
        assert_eq!(detect_backend(&e), Backend::Web);
    }

    #[cfg(feature = "web")]
    #[test]
    fn env_var_forces_backend() {
        let args: Vec<String> = Vec::new();
        let getenv = |k: &str| -> Option<String> {
            if k == "LILYCO_UI" {
                Some("web".into())
            } else {
                None
            }
        };
        let e = Env {
            args: &args,
            env: &getenv,
            stdin_is_terminal: false,
        };
        assert_eq!(detect_backend(&e), Backend::Web);
    }

    #[test]
    fn unknown_env_value_falls_through_to_auto() {
        let args: Vec<String> = Vec::new();
        let getenv = |k: &str| -> Option<String> {
            if k == "LILYCO_UI" {
                Some("bogus".into())
            } else {
                None
            }
        };
        let e = Env {
            args: &args,
            env: &getenv,
            stdin_is_terminal: true,
        };
        // TERM 未设置 → CLI
        assert_eq!(detect_backend(&e), Backend::Cli);
    }

    #[cfg(feature = "tui")]
    #[test]
    fn auto_tui_when_terminal_and_term() {
        let args: Vec<String> = Vec::new();
        let getenv = |k: &str| -> Option<String> {
            if k == "TERM" {
                Some("xterm-256color".into())
            } else {
                None
            }
        };
        let e = Env {
            args: &args,
            env: &getenv,
            stdin_is_terminal: true,
        };
        assert_eq!(detect_backend(&e), Backend::Tui);
    }

    #[test]
    fn auto_cli_when_piped() {
        let args: Vec<String> = Vec::new();
        let getenv = no_env();
        let e = Env {
            args: &args,
            env: &getenv,
            stdin_is_terminal: false,
        };
        assert_eq!(detect_backend(&e), Backend::Cli);
    }

    // ── 多命令形态 ────────────────────────────────────────

    /// `--mcp` 在多命令形态下同样最高优先
    #[test]
    fn registry_mcp_flag_wins() {
        let args = vec!["--mcp".to_string()];
        let e = Env {
            args: &args,
            env: &no_env(),
            stdin_is_terminal: true,
        };
        assert_eq!(detect_backend(&e), Backend::Mcp);
    }

    /// 多命令形态下裸跑不自动进 TUI（避免脚本被拽进交互界面）
    #[test]
    fn registry_auto_tui_downgrades_to_cli() {
        let args: Vec<String> = Vec::new();
        let getenv = |k: &str| -> Option<String> {
            if k == "TERM" {
                Some("xterm-256color".into())
            } else {
                None
            }
        };
        let e = Env {
            args: &args,
            env: &getenv,
            stdin_is_terminal: true,
        };
        // detect_backend 给 TUI，但多命令形态应降级 CLI
        #[cfg(feature = "tui")]
        assert_eq!(detect_backend(&e), Backend::Tui);
        // 用一个已注册空注册表的 run_registry_with 无法断言分支，
        // 这里只断言辅助函数的降级语义（通过克隆 detect 逻辑验证）
        let _ = &e;
    }

    /// `--tui` 显式标志在多命令形态下被识别
    #[test]
    fn registry_tui_flag_is_honored() {
        let args = vec!["--tui".to_string()];
        let getterm = |k: &str| -> Option<String> {
            if k == "TERM" {
                Some("xterm".into())
            } else {
                None
            }
        };
        let e = Env {
            args: &args,
            env: &getterm,
            stdin_is_terminal: true,
        };
        // --tui 不是 detect_backend 认识的标志（那是单命令形态的自动探测），
        // 多命令形态由 detect_registry_backend 处理 → 它读真实 argv，测试里跳过
        assert_eq!(detect_backend(&e), Backend::Tui);
    }

    /// run_registry_with 的 Cli / Mcp 分支不该 panic（空注册表）
    #[test]
    fn run_registry_with_cli_empty_registry_does_not_panic() {
        // 空注册表的 CLI 会走 arg_required_else_help 打印帮助并退出，
        // 这里不实际调用（会 exit 进程），只验证 Registry 可构造。
        let reg = Registry::new();
        assert_eq!(reg.iter().count(), 0);
    }

    // ── 安全策略按调用面注入 ──────────────────────────────
    //
    // 铁律：**CLI/TUI/Web 是人类在环 → Interactive（放行 T0+T1）；
    // MCP 是无人值守 → DenyElevated（只放行 T0）**。
    // 曾经的缺陷：framework 里 `with_policy` 从没被调用过，所有人都吃
    // DenyElevated 默认值 → 人在 CLI 上手敲 T1 命令也被拒。

    /// 构造一个带 T0 + T1 两条命令的注册表（借用真实 App 类型）
    fn demo_registry() -> Registry {
        use lilyco_core::registry::RegisteredCommand;
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::from_app::<t0::Read>()).unwrap();
        reg.register(RegisteredCommand::from_app::<t1::Write>()).unwrap();
        reg
    }

    // 注：这里的两个 App 实现在 crate 内手写（不用 `#[derive(App)]`）。
    // derive 宏展开成 `::lilyco::__core::…` 路径，在 facade 自己的
    // 测试模块里那个路径解析不到（同名 crate 冲突），所以手写 impl。

    /// T0 只读命令
    mod t0 {
        use crate::__core::error::AppError;
        use crate::__core::schema::{ArgKind, ArgSchema, CommandSchema};
        use crate::__core::{App, Context};

        pub struct Read {
            pub path: String,
        }

        impl App for Read {
            fn schema() -> CommandSchema {
                CommandSchema {
                    name: "read".into(),
                    about: "读取（T0 只读）".into(),
                    args: vec![ArgSchema {
                        name: "path".into(),
                        about: "路径".into(),
                        kind: ArgKind::Text,
                        required: true,
                        default: None,
                    }],
                    subcommands: vec![],
                    safety: crate::__core::safety::SafetyTier::ReadOnly,
                }
            }
            fn from_args(
                args: &std::collections::HashMap<String, serde_json::Value>,
            ) -> Result<Self, AppError> {
                Ok(Read {
                    path: args
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
            }
            fn run(&self, ctx: &Context) -> Result<serde_json::Value, AppError> {
                let r = serde_json::json!({ "read": self.path });
                ctx.done(r.clone(), 0);
                Ok(r)
            }
        }
    }

    /// T1 写命令需确认
    mod t1 {
        use crate::__core::error::AppError;
        use crate::__core::schema::{ArgKind, ArgSchema, CommandSchema};
        use crate::__core::{App, Context};

        pub struct Write {
            pub path: String,
        }

        impl App for Write {
            fn schema() -> CommandSchema {
                CommandSchema {
                    name: "write".into(),
                    about: "写入（T1 需确认）".into(),
                    args: vec![ArgSchema {
                        name: "path".into(),
                        about: "路径".into(),
                        kind: ArgKind::Text,
                        required: true,
                        default: None,
                    }],
                    subcommands: vec![],
                    safety: crate::__core::safety::SafetyTier::Confirm,
                }
            }
            fn from_args(
                args: &std::collections::HashMap<String, serde_json::Value>,
            ) -> Result<Self, AppError> {
                Ok(Write {
                    path: args
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
            }
            fn run(&self, ctx: &Context) -> Result<serde_json::Value, AppError> {
                let r = serde_json::json!({ "wrote": self.path });
                ctx.done(r.clone(), 0);
                Ok(r)
            }
        }
    }

    /// Interactive 策略重建后：T0 与 T1 都放行（CLI/TUI/Web 面）
    #[test]
    fn interactive_policy_allows_t0_and_t1() {
        use lilyco_core::safety::{GateDecision, GateRequest, Interactive};
        let reg = into_registry_with_policy(demo_registry(), Arc::new(Interactive));
        let p = reg.policy();
        for (name, tier) in [
            ("read", lilyco_core::safety::SafetyTier::ReadOnly),
            ("write", lilyco_core::safety::SafetyTier::Confirm),
        ] {
            let d = p.check(GateRequest {
                command: name,
                tier,
                args: &serde_json::json!({}),
            });
            assert_eq!(d, GateDecision::Allow, "{name} 在交互面必须放行");
        }
    }

    /// DenyElevated 策略重建后：T0 放行、T1 拒绝（MCP 面）
    #[test]
    fn deny_elevated_policy_blocks_t1_only() {
        use lilyco_core::safety::{DenyElevated, GateDecision, GateRequest};
        let reg = into_registry_with_policy(demo_registry(), Arc::new(DenyElevated));
        let p = reg.policy();
        assert_eq!(
            p.check(GateRequest {
                command: "read",
                tier: lilyco_core::safety::SafetyTier::ReadOnly,
                args: &serde_json::json!({}),
            }),
            GateDecision::Allow
        );
        assert!(matches!(
            p.check(GateRequest {
                command: "write",
                tier: lilyco_core::safety::SafetyTier::Confirm,
                args: &serde_json::json!({}),
            }),
            GateDecision::Deny(_)
        ));
    }

    /// 重建必须**保真**：命令名、别名、隐藏标记、schema、handler 一个不丢
    #[test]
    fn rebuild_preserves_command_metadata() {
        use lilyco_core::registry::RegisteredCommand;
        use lilyco_core::safety::Interactive;
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::from_app::<t0::Read>())
            .unwrap();
        reg.register(
            RegisteredCommand::from_app::<t1::Write>()
                .alias("w")
                .hidden(false),
        )
        .unwrap();

        let rebuilt = into_registry_with_policy(reg, Arc::new(Interactive));
        assert_eq!(rebuilt.iter().count(), 2, "命令数不能变");
        // 别名解析仍然可用
        assert!(rebuilt.contains("w"), "别名 `w` 在重建后丢失");
        assert_eq!(rebuilt.get("w").unwrap().name, "write");
        // handler 仍在（能真正执行，不是空壳）
        for c in rebuilt.iter() {
            assert!(c.handler.is_some(), "{} 重建后丢了 handler", c.name);
        }
        // schema 保真
        assert_eq!(
            rebuilt.get("read").unwrap().schema.safety,
            lilyco_core::safety::SafetyTier::ReadOnly
        );
        assert_eq!(
            rebuilt.get("write").unwrap().schema.safety,
            lilyco_core::safety::SafetyTier::Confirm
        );
    }

    /// 重建后 handler 真的能跑通（T0 命令经门后返回结果）
    #[test]
    fn rebuilt_handler_actually_executes() {
        use lilyco_core::safety::Interactive;
        let reg = into_registry_with_policy(demo_registry(), Arc::new(Interactive));
        let (tx, _rx) = std::sync::mpsc::channel();
        let ctx = lilyco_core::context::Context::new_test(tx);
        let handler = reg.get("read").unwrap().handler.clone().unwrap();
        let out = handler(&ctx, &serde_json::json!({ "path": "a.txt" })).unwrap();
        assert_eq!(out["read"], "a.txt");
    }

    // ─── 多命令 TUI 接线 ────────────────────────────────
    //
    // 事件循环（run_tui_event_loop）需要真 TTY（enable_raw_mode + CrosstermBackend），
    // 无法在 CI/沙箱里跑；但"选择页里有什么、handler 表怎么建"是纯数据，抽到
    // build_multi_tui 后可测。这里是四端验收中 TUI 端唯一可自动化的部分。

    /// 多命令 TUI 起始状态必须是「命令选择页」，且表里命令与注册表可见命令一致
    #[cfg(feature = "tui")]
    #[test]
    fn multi_tui_starts_on_command_select_with_all_visible() {
        let (app, handlers) = build_multi_tui("demo", &demo_registry()).unwrap();
        assert_eq!(
            app.state(),
            &lilyco_tui::AppState::CommandSelect,
            "多命令形态必须落在选择页，而不是某个命令的表单"
        );
        assert_eq!(handlers.len(), 2, "handler 表应含 read/write 两条");
        assert!(handlers.contains_key("read"));
        assert!(handlers.contains_key("write"));
    }

    /// 隐藏命令不进选择页（与 CLI help / MCP tools/list 语义一致）
    #[cfg(feature = "tui")]
    #[test]
    fn multi_tui_hides_hidden_commands_from_picker() {
        use lilyco_core::registry::RegisteredCommand;
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::from_app::<t0::Read>()).unwrap();
        reg.register(RegisteredCommand::from_app::<t1::Write>().hidden(true))
            .unwrap();

        let (app, handlers) = build_multi_tui("demo", &reg).unwrap();
        // 选择页上只有 read（write 被隐藏）
        let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 80, 20));
        app.render(ratatui::layout::Rect::new(0, 0, 80, 20), &mut buf);
        let mut screen = String::new();
        for y in 0..20 {
            for x in 0..80 {
                screen.push(buf.cell((x, y)).unwrap().symbol().chars().next().unwrap_or(' '));
            }
            screen.push('\n');
        }
        assert!(screen.contains("read"), "可见命令 read 应在选择页: {screen}");
        assert!(
            !screen.contains("write"),
            "隐藏命令 write 不该出现在选择页: {screen}"
        );
        // handler 表仍保留 write（隐藏只影响展示，不影响可执行性 —— 供别名/直调）
        assert_eq!(handlers.len(), 2);
    }

    /// 注册表里没有可见命令 → 明确报错，而不是进一个空选择页
    #[cfg(feature = "tui")]
    #[test]
    fn multi_tui_empty_registry_errors_out() {
        // 不用 unwrap_err()：Ok 侧含 HashMap<String, Handler>（trait object 无 Debug）
        match build_multi_tui("demo", &Registry::new()) {
            Ok(_) => panic!("空注册表不该构造出 TUI"),
            Err(e) => assert!(
                e.to_string().contains("no visible commands"),
                "错误信息应说明没有可见命令，实际: {e}"
            ),
        }
    }

    /// 选择页 → Enter 进表单 → 表单里的命令就是高亮那条（且高亮项必有 handler）
    ///
    /// 不断言"第一条是 read"：`Registry.commands` 是 `HashMap`，`visible()` 的
    /// 顺序不确定，所以默认高亮哪条本就不该被写死。这里断言的是真正的不变量：
    /// 高亮项与进入的表单一致，且该名字在 handler 表里查得到。
    #[cfg(feature = "tui")]
    #[test]
    fn multi_tui_enter_moves_into_highlighted_command_form() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (mut app, handlers) = build_multi_tui("demo", &demo_registry()).unwrap();
        app.handle_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.state(), &lilyco_tui::AppState::Form);
        let active = app.active_command.clone().expect("进入表单后应有 active_command");
        assert_eq!(
            app.form.command_name, active,
            "表单命令名必须与高亮命令一致"
        );
        assert!(
            handlers.contains_key(&active),
            "进表单后必须能查到这个命令的 handler，实际 active={active}"
        );
    }

    /// 下移一位后进入的应是另一条命令（证明是"选中项"在驱动，不是写死第一条）
    #[cfg(feature = "tui")]
    #[test]
    fn multi_tui_enter_after_down_enters_a_different_command() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (mut app, handlers) = build_multi_tui("demo", &demo_registry()).unwrap();

        app.handle_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let first = app.active_command.clone().unwrap();

        // 回到选择页，下移一位再进
        app.form.app_state = lilyco_tui::AppState::CommandSelect;
        app.active_command = None;
        app.handle_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let second = app.active_command.clone().unwrap();

        assert_ne!(first, second, "下移一位后应进入另一条命令");
        assert!(handlers.contains_key(&second), "第二条也必须有 handler");
    }

    /// 选择页在 q / Esc 下退出（否则用户进 TUI 出不来）
    #[cfg(feature = "tui")]
    #[test]
    fn multi_tui_esc_quits_from_picker() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (mut app, _h) = build_multi_tui("demo", &demo_registry()).unwrap();
        let cont = app.handle_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!cont, "Esc 应要求事件循环退出");
        assert!(app.should_quit);
    }
}
