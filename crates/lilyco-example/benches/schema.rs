//! schema 生成性能基准（roadmap: Performance benchmarks for schema generation）
//!
//! 零依赖实现（不引 criterion）：`harness = false` + std 计时。
//! 运行：`cargo bench -p lilyco-example`
//!
//! 基准点 = 框架热路径：derive 生成的 `schema()`、四端共用的
//! `to_json_schema` / `to_openai_tool` 导出、`validate_args` 校验、
//! 以及 `Registry::from_app + register` 装配。

use std::time::Instant;

use lilyco::prelude::*;

#[derive(App)]
#[app(name = "bench-sample", about = "基准样例", run = "run_bench")]
// 字段只被 derive 生成的 schema/from_args 读写，基准不读它们 —— 静默 dead_code
#[allow(dead_code)]
struct BenchSample {
    /// 输入文件
    input: String,
    /// 质量 1-100
    #[arg(default = 75, range = 1..=100)]
    quality: u8,
    /// 干跑
    dry_run: bool,
    /// 可选输出
    output: Option<String>,
}

fn run_bench(_app: &BenchSample, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let r = serde_json::json!({ "ok": true });
    ctx.done(r.clone(), 0);
    Ok(r)
}

fn bench<F: FnMut()>(name: &str, iters: u32, mut f: F) {
    for _ in 0..200 {
        f(); // warmup
    }
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    let elapsed = start.elapsed();
    println!(
        "  {name:>26}: {iters} iters, {elapsed:>10.1?} total, {:>8.3} µs/op",
        elapsed.as_nanos() as f64 / iters as f64 / 1000.0
    );
}

fn main() {
    println!("== lilyco schema generation benchmark ==");

    bench("derive schema()", 10_000, || {
        let _ = BenchSample::schema();
    });

    let schema = BenchSample::schema();
    bench("to_json_schema()", 10_000, || {
        let _ = schema.to_json_schema();
    });
    bench("to_openai_tool()", 10_000, || {
        let _ = schema.to_openai_tool();
    });
    bench("to_anthropic_tool()", 10_000, || {
        let _ = schema.to_anthropic_tool();
    });

    let args = serde_json::json!({ "input": "a.png", "quality": 50, "dry_run": true });
    bench("validate_args()", 10_000, || {
        let _ = schema.validate_args(&args);
    });

    bench("from_app + register", 10_000, || {
        let mut reg = Registry::new();
        let _ = reg.register(RegisteredCommand::from_app::<BenchSample>());
    });

    println!("(以上为 release profile 实测参考值，随机器浮动；dev profile 慢一个量级)");
}
