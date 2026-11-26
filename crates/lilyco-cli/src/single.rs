//! 单命令一行启动：schema → clap → parse → 执行 → 输出。
//!
//! 这是应用侧最常碰的唯一入口（facade 的 `lilyco::run::<A>()` 转的就是它）。

use lilyco_core::prelude::*;

use crate::registry::drain_events;
use crate::renderer::CliRenderer;

/// 一行启动 CLI：自动处理 schema → clap → parse → progress → 输出。
///
/// ```ignore
/// fn main() {
///     lilyco_cli::run::<MyApp>(my_run_fn);
/// }
/// ```
pub fn run<A: App + Send + 'static>(
    runner: fn(&A, &Context) -> Result<serde_json::Value, AppError>,
) {
    use std::sync::Arc;

    let schema = A::schema();
    let renderer = CliRenderer::new();
    let cmd = renderer.render(&schema);
    let matches = cmd.get_matches();

    if CliRenderer::handle_builtin_flags(&schema, &matches) {
        return;
    }

    let output_format = CliRenderer::output_format(&matches);
    let args = CliRenderer::extract_args(&schema, &matches);

    // 执行语义交给 core::executor（与 TUI / GUI / MCP 共享同一宿主）
    let args_value = serde_json::to_value(&args).unwrap_or(serde_json::json!({}));
    let handler: Handler = Arc::new(move |ctx, args| {
        let obj = args
            .as_object()
            .ok_or_else(|| AppError::InvalidArg("args must be a JSON object".into()))?;
        let map: std::collections::HashMap<String, serde_json::Value> =
            obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let app = A::from_args(&map)?;
        runner(&app, ctx)
    });
    let task = spawn(handler, args_value);
    drain_events(task, output_format);
}
