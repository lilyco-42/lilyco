//! 独立 GTK 窗口：同一份 UiSpec JSON，不经过任何 WebView。
//!
//!   cd lilyco-native-ui && cargo run                     # 内置图片压缩工具示例
//!   cargo run -- ./my-spec.json                         # AI 生成的 JSON 直接起原生窗口

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;

use lilyco_ultra_ui::spec::{UiSpec, WindowSize};

fn main() -> impl std::process::Termination {
    // LILYCO_BENCH=1 只给 A/B 基准开毫秒时间戳、per-commit trace 和 FPS 采样；
    // 正常运行不额外打日志，也不装性能监视器。
    let bench = std::env::var_os("LILYCO_BENCH").is_some();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(if bench {
        "nsh::perf=info,nsh::fps=info"
    } else {
        "warn"
    }))
    .format_timestamp_millis()
    .init();
    if bench {
        eprintln!("LILYCO_BENCH_PID {}", std::process::id());
    }

    let json = std::env::args()
        .nth(1)
        .map(|p| std::fs::read_to_string(&p).expect("读取 UiSpec JSON 失败"))
        .unwrap_or_else(lilyco_ultra_ui::spec::default_example_json);
    let spec = UiSpec::from_json(&json).expect("UiSpec JSON 解析失败");
    let errors = spec.validate();
    assert!(errors.is_empty(), "UiSpec 校验失败: {}", errors.join("; "));

    let title = spec.window.title.clone();
    let size = window_size(&spec.window.size);
    let _ = lilyco_native_ui::set_spec(spec);
    platform::set_ui_sets(vec![nsh_base::registry(), lilyco_native_ui::registry()]);

    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<lilyco_native_ui::App>()),
        platform::AppOptions {
            title,
            app_id: "com.lilyco.native-ui".to_owned(),
            size,
            trace: bench,
            fps: bench,
            ..Default::default()
        },
    )
}

fn window_size(size: &WindowSize) -> (i32, i32) {
    match size {
        WindowSize::小 => (420, 620),
        WindowSize::中等 => (640, 820),
        WindowSize::大 => (960, 900),
        WindowSize::全屏 => (1280, 960),
    }
}
