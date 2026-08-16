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
use std::sync::Arc;

use lilyco_core::executor;
use lilyco_core::progress::LogLevel;
use lilyco_core::registry::{Handler, RegisteredCommand, Registry};
use lilyco_core::{App, AppError, Progress};

/// 常用导入（trait + 类型 + derive 宏）
pub mod prelude {
    pub use lilyco_core::prelude::*;
    pub use lilyco_macros::{App, ValueEnum};
}

/// 可用的后端
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Cli,
    Tui,
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
pub fn detect_backend(env: &Env) -> Backend {
    // 1. 显式标志（调用时刻的意图最高优先）
    if env.args.iter().any(|a| a == "--mcp") {
        return Backend::Mcp;
    }
    if env.args.iter().any(|a| a == "--gui" || a == "--web") {
        return Backend::Web;
    }
    // 2. 环境变量
    if let Some(ui) = (env.env)("LILYCO_UI") {
        match ui.as_str() {
            "cli" => return Backend::Cli,
            "tui" => return Backend::Tui,
            "web" | "gui" => return Backend::Web,
            "mcp" => return Backend::Mcp,
            _ => {} // 未知值 → 落到自动
        }
    }
    // 3. 自动
    if env.stdin_is_terminal && (env.env)("TERM").map(|t| !t.is_empty()).unwrap_or(false) {
        Backend::Tui
    } else {
        Backend::Cli
    }
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
        Backend::Tui => {
            // mininterface 借鉴点：TUI 起不来（非终端/CI）时回退 CLI，绝不裸崩
            if run_tui::<A>().is_err() {
                run_cli::<A>();
            }
        }
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
        .register(RegisteredCommand::from_app::<A>())
        .expect("register app command");
    registry
}

fn run_cli<A: App + Send + 'static>() {
    lilyco_cli::run::<A>(|app, ctx| app.run(ctx));
}

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

fn run_tui<A: App + Send + 'static>() -> std::io::Result<()> {
    use crossterm::event::Event;
    use crossterm::execute;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use lilyco_tui::{AppState, TuiApp};

    let schema = A::schema();
    let mut app = TuiApp::new(&schema);

    enable_raw_mode()?;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;

    loop {
        terminal.draw(|f| {
            let area = f.area();
            app.render(area, f.buffer_mut());
        })?;

        if app.should_quit {
            break;
        }

        match crossterm::event::read()? {
            Event::Key(key) => {
                let cont = app.handle_event(key);
                if !cont {
                    break;
                }
                // 用户连续 Enter 确认后进入 Running → 执行
                if app.state() == &AppState::Running {
                    let args = collect_args(&app);
                    execute_form::<A>(&mut app, args);
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

/// 把表单当前值收集为参数 JSON
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

/// 通过共享 executor 执行并把进度事件灌进 TUI 状态
fn execute_form<A: App + Send + 'static>(app: &mut lilyco_tui::TuiApp, args: serde_json::Value) {
    let handler: Handler = Arc::new(move |ctx, args| {
        let obj = args
            .as_object()
            .ok_or_else(|| AppError::InvalidArg("args must be a JSON object".into()))?;
        let map: std::collections::HashMap<String, serde_json::Value> =
            obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let a = A::from_args(&map)?;
        a.run(ctx)
    });

    let task = executor::spawn(handler, args);
    for ev in task.rx {
        match ev {
            Progress::Started { message, .. } => app.start_progress(None, message),
            Progress::Tick {
                current,
                total,
                message,
                ..
            } => app.tick_progress(current, total, message),
            Progress::Log { level, message } => app.log_progress(level_name(&level), message),
            Progress::Done {
                result,
                duration_ms,
            } => {
                app.finish_progress(result, duration_ms);
                break;
            }
            Progress::Error { code, message, .. } => {
                app.error_progress(code, message);
                break;
            }
        }
    }
    let _ = task.handle.join();
}

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
