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
    use crossterm::event::Event;
    use crossterm::execute;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use lilyco_tui::{AppState, TuiApp};
    use std::time::Duration;

    let schema = A::schema();
    let mut app = TuiApp::new(&schema);

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
                let args = collect_args(&app);
                task_state = TaskState::Running(executor::spawn(build_handler::<A>(), args));
            }
        }

        // 2. Running：非阻塞排空任务进度事件，保持 UI 响应
        if app.state() == &AppState::Running {
            if let TaskState::Running(task) = &task_state {
                drain_task(&mut app, task);
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

        // 4. 已离开 Running（Done / Error）→ 请求取消并回收线程
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
}
