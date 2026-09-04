//! 多命令示例：一个二进制，两个子命令（CLI 多命令 / `run_registry`）。
//!
//! ```bash
//! cargo run -p lilyco-example --example multi -- --schema        # 注册表清单（Agent 可消费）
//! cargo run -p lilyco-example --example multi -- ping --name 世界
//! cargo run -p lilyco-example --example multi -- add --a 1 --b 2
//! cargo run -p lilyco-example --example multi -- add --json --a 1 --b 2
//! cargo run -p lilyco-example --example multi -- p --name hi     # 别名
//! ```

use lilyco::prelude::*;

/// 打招呼
#[derive(App)]
#[app(name = "ping", about = "向某人问好", run = "run_ping")]
struct Ping {
    /// 问谁
    #[arg(default = "world")]
    name: String,
}

fn run_ping(app: &Ping, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let msg = format!("hello, {}", app.name);
    ctx.done(serde_json::json!({ "msg": msg }), 0);
    Ok(serde_json::json!({ "msg": msg }))
}

/// 加法（演示长任务进度上报）
#[derive(App)]
#[app(name = "add", about = "两数相加", run = "run_add")]
struct Add {
    /// 第一个数
    a: f64,
    /// 第二个数
    b: f64,
    /// 分步上报进度
    #[arg(default = true)]
    verbose: bool,
}

fn run_add(app: &Add, ctx: &Context) -> Result<serde_json::Value, AppError> {
    if app.verbose {
        ctx.emit(Progress::Started {
            total: Some(2),
            message: Some("开始计算".into()),
        });
        ctx.tick(1, Some(2), "读取操作数");
        ctx.tick(2, Some(2), "求和");
    }
    let sum = app.a + app.b;
    ctx.done(serde_json::json!({ "sum": sum }), 0);
    Ok(serde_json::json!({ "sum": sum }))
}

fn main() {
    let mut registry = Registry::new();
    registry
        .register(RegisteredCommand::from_app::<Ping>().alias("p"))
        .unwrap();
    registry
        .register(RegisteredCommand::from_app::<Add>())
        .unwrap();
    lilyco::run_cli_registry("multi", registry);
}
