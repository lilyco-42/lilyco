//! 多命令启动：`Registry` → clap 子命令 → 按子命令取 handler → 执行并把进度事件打到 stdout。
//!
//! 可见/别名/隐藏语义与 TUI / Web / MCP 对齐（docs/CODEGRAPH.md §5）；
//! 执行宿主仍是 `core::executor`，本模块只负责「怎么把事件说给人听」。

use clap::{Arg, ArgAction, Command};

use lilyco_core::prelude::*;

use crate::command::{build_command, leak_str};
use crate::renderer::CliRenderer;

/// 多命令一行启动：把整个 [`Registry`] 渲染成 clap 子命令树。
///
/// - 可见命令 → 普通 subcommand（别名一并在 help 中可用）
/// - 隐藏命令 → `hide(true)` 子命令（可调用，help 不显示）
/// - 根级 `--schema`：打印注册表 JSON 清单（全部命令的 schema，供 Agent 消费）
/// - 每个子命令自带 `--schema` / `--json` / `--json-stream` 等内置标志
///
/// ```ignore
/// fn main() {
///     let mut registry = Registry::new();
///     registry.register(RegisteredCommand::from_app::<Compress>()).unwrap();
///     registry.register(RegisteredCommand::from_app::<Resize>()).unwrap();
///     lilyco_cli::run_registry("imgtool", registry);
/// }
/// ```
pub fn run_registry(app_name: &str, registry: Registry) {
    let cmd = build_registry_command(app_name, &registry);
    let matches = cmd.get_matches();

    if matches.get_flag("schema") {
        println!(
            "{}",
            serde_json::to_string_pretty(&registry.to_json()).unwrap()
        );
        return;
    }

    let Some((sub_name, sub_m)) = matches.subcommand() else {
        return; // arg_required_else_help 已在无参数时打印帮助
    };
    let Some(entry) = resolve_registry_command(&registry, sub_name) else {
        eprintln!("Error: unknown command: {sub_name}");
        std::process::exit(2);
    };

    execute_registered(entry, sub_m);
}

/// 构建注册表的根 `clap::Command`（纯函数，可单测）
///
/// 子命令名取自各命令 schema 的 `name` 字段；注册表别名通过
/// `resolve_registry_command` 在运行期解析。
pub fn build_registry_command(app_name: &str, registry: &Registry) -> Command {
    let mut root = Command::new(leak_str(app_name))
        .disable_version_flag(true)
        .arg_required_else_help(true);

    for entry in registry.iter() {
        let mut sub = build_command(&entry.schema);
        for alias in &entry.aliases {
            sub = sub.alias(leak_str(alias));
        }
        if entry.hidden {
            sub = sub.hide(true);
        }
        root = root.subcommand(sub);
    }

    root.arg(
        Arg::new("schema")
            .long("schema")
            .help("打印注册表 JSON 清单（全部命令）并退出")
            .action(ArgAction::SetTrue)
            .exclusive(true),
    )
}

/// 按规范名或别名解析注册表条目；schema.name 与注册名不一致的
/// 声明式命令（`register_from_json`）也能命中
pub(crate) fn resolve_registry_command<'r>(
    registry: &'r Registry,
    name: &str,
) -> Option<&'r RegisteredCommand> {
    registry
        .get(name)
        .or_else(|| registry.iter().find(|c| c.schema.name == name))
}

/// 执行单个已解析命令：内置标志 → 参数提取 → executor → 输出渲染
pub(crate) fn execute_registered(entry: &RegisteredCommand, sub_m: &clap::ArgMatches) {
    let schema = &entry.schema;
    if CliRenderer::handle_builtin_flags(schema, sub_m) {
        return;
    }
    let output_format = CliRenderer::output_format(sub_m);
    let args = CliRenderer::extract_args(schema, sub_m);

    let Some(handler) = entry.handler.clone() else {
        eprintln!(
            "Error: command `{}` has no handler（声明式加载的命令不能直接执行）",
            entry.name
        );
        std::process::exit(1);
    };
    let args_value = serde_json::to_value(&args).unwrap_or(serde_json::json!({}));
    let task = spawn(handler, args_value);
    drain_events(task, output_format);
}

/// 消费任务进度事件并按输出格式渲染（`run` 与 `run_registry` 共用）。
/// 事件流协议保证以 Done / Error 结尾，线程 join 后进程退出。
pub(crate) fn drain_events(task: Task, output_format: OutputFormat) {
    match output_format {
        OutputFormat::JsonStream => {
            for event in task.rx {
                println!("{}", serde_json::to_string(&event).unwrap());
                if matches!(event, Progress::Done { .. } | Progress::Error { .. }) {
                    break;
                }
            }
        }
        OutputFormat::Json => {
            for event in task.rx {
                if let Progress::Done { result, .. } = event {
                    println!("{}", serde_json::to_string_pretty(&result).unwrap());
                    break;
                }
                if let Progress::Error { message, .. } = event {
                    eprintln!("Error: {message}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            for event in task.rx {
                match &event {
                    Progress::Tick {
                        message, percent, ..
                    } => {
                        if let Some(msg) = message {
                            let pct = percent
                                .map(|p| format!("{:3.0}%", p * 100.0))
                                .unwrap_or_default();
                            eprintln!("\r  {pct}  {msg}");
                        }
                    }
                    Progress::Log { level, message } => {
                        eprintln!("  [{level:?}] {message}");
                    }
                    Progress::Telemetry { key, value } => {
                        eprintln!("  [telemetry] {key}={value}");
                    }
                    Progress::Done {
                        result,
                        duration_ms,
                    } => {
                        if let Ok(r) = serde_json::to_string_pretty(result) {
                            if r != "null" && r != "\"ok\"" && r.len() < 500 {
                                println!("{r}");
                            }
                        }
                        eprintln!("\n  Done in {duration_ms}ms");
                        break;
                    }
                    Progress::Error { message, .. } => {
                        eprintln!("\n  Error: {message}");
                        std::process::exit(1);
                    }
                    _ => {}
                }
            }
            eprintln!();
        }
    }

    if let Err(e) = task.handle.join() {
        eprintln!("Error: {e:?}");
        std::process::exit(1);
    }
}
