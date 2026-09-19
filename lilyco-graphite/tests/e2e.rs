// 与 lib.rs 同款：graphite 类型链撑爆默认 trait 求解深度
#![recursion_limit = "512"]

//! Graphite 桥端到端（P0 spike）
//!
//! 链路：lilyco App → 安全门（T0 直通 / T1 拒绝）→ GraphiteHost（headless 消息总线）
//! → 内核文档状态 → 序列化产物，全程无 GUI、无 GPU。
//!
//! 内核进程内只有一个宿主（`ENVIRONMENT` OnceLock），且 "活动文档" 是全局状态：
//! 并行测试会互相切换活动文档，因此所有触碰宿主的区域统一用 [`DRIVER`] 串行化
//! （锁序：DRIVER → 宿主锁；execute 的 handler 线程只拿宿主锁，不会死锁）。

use lilyco::prelude::*;
use lilyco_graphite::{
    gpu_available, parse_hex_color, shared_host, GraphiteAddRect, GraphiteDocNew,
    GraphiteExportPng, GraphiteExportSvg, GraphiteSave, GraphiteSetFill,
};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// 测试级串行锁：保证 "建文档 / 画图 / 执行 App" 的复合操作不被并行测试插入。
/// 毒化恢复：某个断言失败不应连坐后续测试。
fn driver() -> MutexGuard<'static, ()> {
    static DRIVER: OnceLock<Mutex<()>> = OnceLock::new();
    DRIVER
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 共享宿主上锁（毒化恢复同 driver()）
fn lock_host() -> MutexGuard<'static, lilyco_graphite::GraphiteHost> {
    shared_host().lock().unwrap_or_else(|e| e.into_inner())
}

fn handler_of(reg: &Registry, name: &str) -> Handler {
    reg.get(name).unwrap().handler.clone().unwrap()
}

// ── headless 全链（核心 spike 验证点） ────────────────────

#[test]
fn headless_new_doc_add_rect_save_full_chain() {
    let _session = driver();
    let mut host = lock_host();

    // 1) 新建文档
    host.new_document("spike");
    let before = host.node_count().unwrap();

    // 2) 设主色 + 画矩形（指针消息序列）
    host.set_primary_color(parse_hex_color("#E14D2A").unwrap());
    host.draw_rectangle(10., 20., 200., 100.);

    // 3) 断言文档状态：画矩形后根网络节点数必须增加
    let after = host.node_count().unwrap();
    assert!(
        after > before,
        "加矩形后节点数应增加: before={before} after={after}"
    );

    // 4) 改填充不透明度——会向图层插入 blending fill 节点，节点数可能再增
    host.set_fill(0.5);
    let filled = host.node_count().unwrap();
    assert!(
        filled >= after,
        "set_fill 不应减少节点: after={after} filled={filled}"
    );

    // 5) 序列化：拿到 .graphite 字节且为合法 JSON，内含 rectangle 图层痕迹
    let (name, bytes) = host.save_content().unwrap();
    assert!(
        name.ends_with(".graphite") || name.ends_with(".gdd"),
        "文件名异常: {name}"
    );
    assert!(!bytes.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let text = value.to_string().to_lowercase();
    assert!(
        text.contains("rectangle"),
        "序列化产物应包含 rectangle 节点"
    );
    assert_eq!(
        value
            .pointer("/network_interface/network/nodes")
            .and_then(|n| n.as_array())
            .unwrap()
            .len(),
        filled,
        "节点计数应与序列化产物一致"
    );
}

#[test]
fn hex_color_parsing() {
    assert!(parse_hex_color("#E14D2A").is_ok());
    assert!(parse_hex_color("E14D2ACC").is_ok());
    assert!(parse_hex_color("#E1D2").is_err());
    assert!(parse_hex_color("#GGGGGG").is_err());
}

// ── 安全门 + 遥测端到端 ───────────────────────────────────

#[test]
fn doc_new_passes_gate_and_emits_telemetry() {
    let _session = driver();
    let mut reg = Registry::new(); // 默认 DenyElevated：T0 直通
    reg.register(RegisteredCommand::from_app::<GraphiteDocNew>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "graphite-doc-new"),
        serde_json::json!({ "name": "telemetry" }),
    );
    let result = outcome.result.expect("T0 新建文档必须放行");
    assert_eq!(result["nodes"], serde_json::json!(0));

    // P1 遥测：操作名 + 节点数
    assert!(outcome.events.iter().any(|e| matches!(
        e,
        Progress::Telemetry { key, value }
            if key == "graphite.op" && value.as_str() == Some("doc-new")
    )));
    assert!(outcome.events.iter().any(|e| matches!(
        e,
        Progress::Telemetry { key, .. } if key == "graphite.nodes"
    )));
}

#[test]
fn add_rect_passes_gate_and_grows_document() {
    let _session = driver();
    // 确保有活动文档
    lock_host().new_document("add-rect");

    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<GraphiteAddRect>())
        .unwrap();

    let handler = handler_of(&reg, "graphite-add-rect");
    let first = execute(
        handler.clone(),
        serde_json::json!({ "x": 0., "y": 0., "w": 100., "h": 50. }),
    );
    let baseline = first.result.expect("T0 画矩形必须放行")["nodes"]
        .as_u64()
        .unwrap();

    let second = execute(
        handler,
        serde_json::json!({ "x": 10., "y": 10., "w": 80., "h": 40., "fill": "#00FF00" }),
    );
    let grown = second.result.expect("第二次画矩形必须放行")["nodes"]
        .as_u64()
        .unwrap();
    assert!(
        grown > baseline,
        "再画一个矩形后节点数应增加: {baseline} -> {grown}"
    );
}

#[test]
fn set_fill_blocked_by_default_policy() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<GraphiteSetFill>())
        .unwrap();

    // T1：自动化面默认拒绝，且拒绝发生在触碰内核之前（门在 handler 外层）
    let outcome = execute(
        handler_of(&reg, "graphite-set-fill"),
        serde_json::json!({ "fill": 0.5 }),
    );
    let err = outcome.result.unwrap_err();
    assert!(matches!(err, AppError::Safety(_)), "{err}");
    assert!(err.to_string().contains("graphite-set-fill"), "{err}");
}

/// 交互面策略：T1 命令放行（模拟人类在环确认后的本地调用）
#[test]
fn set_fill_allowed_by_interactive_policy() {
    let _session = driver();
    // 先确保有文档和选中图层（handler 线程会再拿宿主锁，这里不能持有）
    {
        let mut host = lock_host();
        host.new_document("interactive");
        host.draw_rectangle(0., 0., 50., 50.);
    }

    let mut reg = Registry::new().with_policy(Arc::new(Interactive));
    reg.register(RegisteredCommand::from_app::<GraphiteSetFill>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "graphite-set-fill"),
        serde_json::json!({ "fill": 0.75 }),
    );
    let result = outcome.result.expect("Interactive 策略应放行 T1");
    assert_eq!(result["fill"], serde_json::json!(0.75));
    assert!(outcome.events.iter().any(|e| matches!(
        e,
        Progress::Telemetry { key, value }
            if key == "graphite.fill" && value.as_f64() == Some(0.75)
    )));
}

/// save 落盘端到端：T0 直通，产物可读回且为 Graphite 文档
#[test]
fn save_writes_document_bytes_to_disk() {
    let _session = driver();
    // 保证有内容可存
    {
        let mut host = lock_host();
        host.new_document("save-demo");
        host.draw_rectangle(5., 5., 30., 30.);
    }

    let path = std::env::temp_dir().join("lilyco-graphite-spike.graphite");
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<GraphiteSave>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "graphite-save"),
        serde_json::json!({ "path": path.to_string_lossy() }),
    );
    let result = outcome.result.expect("T0 保存必须放行");
    assert!(result["bytes"].as_u64().unwrap() > 0);

    let bytes = std::fs::read(&path).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        value.get("network_interface").is_some(),
        "产物应为 Graphite 文档序列化格式"
    );
    let _ = std::fs::remove_file(&path);
}

// ── P1 出图验证：消息总线建图 → DynamicExecutor 渲染 → 图片文件落盘 ──

/// **决定性 golden**：doc-new → add-rect(红) → export-svg → 断言真实 SVG 图片文件
///
/// 链路 = 消息总线文档 → save_content 序列化 → load_network → wrap_network_in_scope
/// → Preprocessor → Compiler → DynamicExecutor → RenderConfig{Svg} 图求值 → SVG 落盘
/// （官方 graphene-cli 同款渲染初始化序列，复刻见 src/export.rs）
#[test]
fn export_svg_renders_red_rect_golden() {
    let _session = driver();

    // 1) 消息总线建图：新建文档 + 红色矩形
    // 注意：编辑器默认路由是 "副色喂填充、主色喂描边"，要出填充红必须设填充工作色
    {
        let mut host = lock_host();
        host.new_document("export-svg");
        host.set_fill_color(parse_hex_color("#E14D2A").unwrap());
        host.draw_rectangle(10., 20., 200., 100.);
    }

    // 2) 经 App 层导出（不持宿主锁——handler 线程要拿宿主锁 save_content）
    let path = std::env::temp_dir().join("lilyco-export-golden.svg");
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<GraphiteExportSvg>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "graphite-export-svg"),
        serde_json::json!({ "out": path.to_string_lossy(), "scale": 1.0 }),
    );
    let result = outcome
        .result
        .expect("SVG 导出必须成功（这是 P1 决定性验证）");
    assert!(result["bytes"].as_u64().unwrap() > 0, "SVG 应非空");
    assert!(
        outcome.events.iter().any(|e| matches!(
            e,
            Progress::Telemetry { key, value }
                if key == "graphite.export_format" && value.as_str() == Some("svg")
        )),
        "应发出格式遥测"
    );

    // 3) 断言产物：真实存在的合法 SVG，含形状元素与填充色
    // 注意：Graphite 的 SVG 渲染器把所有矢量形状统一发射为 <path d="…">（不发射
    // <rect>，rect 只出现在 clipPath defs 里），所以这里断言 <path 而非 <rect>
    let svg = std::fs::read_to_string(&path).expect("SVG 文件必须存在");
    assert!(
        svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""),
        "SVG 应以标准 <svg> 根元素开头（合法 XML），实际开头: {}",
        &svg.chars().take(80).collect::<String>()
    );
    assert!(svg.trim_end().ends_with("</svg>"), "SVG 应有闭合根元素");
    assert!(svg.contains("<path d="), "应包含 <path> 矢量形状元素");
    assert!(
        svg.to_lowercase().contains("e14d2a"),
        "应包含矩形填充色 #E14D2A（SVG 内以 fill=\"#…\" 十六进制发射）。\
         实际 fill 属性: {:?}；SVG 前 1200 字符: {}",
        svg.match_indices("fill=")
            .map(|(i, _)| &svg[i..svg.len().min(i + 40)])
            .collect::<Vec<_>>(),
        &svg[..svg.len().min(1200)]
    );
    let _ = std::fs::remove_file(&path);
}

/// 从 .graphite 文件渲染（而非当前文档）：save 落盘 → export-svg 读盘渲染
#[test]
fn export_svg_from_saved_file_roundtrip() {
    let _session = driver();

    // 建图并落盘成 .graphite 文件
    let doc_path = std::env::temp_dir().join("lilyco-export-roundtrip.graphite");
    {
        let mut host = lock_host();
        host.new_document("roundtrip");
        host.set_fill_color(parse_hex_color("#2244EE").unwrap());
        host.draw_rectangle(0., 0., 80., 40.);
        let (_, bytes) = host.save_content().unwrap();
        std::fs::write(&doc_path, &bytes).unwrap();
    }

    let out_path = std::env::temp_dir().join("lilyco-export-roundtrip.svg");
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<GraphiteExportSvg>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "graphite-export-svg"),
        serde_json::json!({
            "doc": doc_path.to_string_lossy(),
            "out": out_path.to_string_lossy(),
        }),
    );
    let result = outcome.result.expect("从文件渲染 SVG 必须成功");
    assert!(result["bytes"].as_u64().unwrap() > 0);

    let svg = std::fs::read_to_string(&out_path).unwrap();
    assert!(
        svg.contains("<path d="),
        "文件渲染产物应含 <path> 矢量形状元素"
    );
    assert!(
        svg.to_lowercase().contains("2244ee"),
        "文件渲染产物应含填充色 #2244EE"
    );
    let _ = std::fs::remove_file(&doc_path);
    let _ = std::fs::remove_file(&out_path);
}

/// PNG 导出（GPU/Vello 路径，尽力而为）：有可用 GPU 时完整跑通并断言 PNG 魔数；
/// 无 GPU（裸 CI 容器）时优雅跳过——不阻塞 CI，结论随 PR 汇报。
#[test]
fn export_png_when_gpu_available() {
    // GPU 软探测：WgpuExecutor 初始化失败 → 跳过（装了 Mesa lavapipe/llvmpipe 的
    // CI 或有独显的本机会真实执行）
    if !gpu_available() {
        eprintln!(
            "跳过 PNG 导出测试：当前环境无可用 GPU 执行器（WgpuExecutor 初始化失败）。\
             SVG 为主力出图路径不受影响；需要验证 PNG 时安装 Mesa 软件 GPU 栈 \
             （mesa-vulkan-drivers/libegl1）后重跑"
        );
        return;
    }

    let _session = driver();
    {
        let mut host = lock_host();
        host.new_document("export-png");
        host.set_fill_color(parse_hex_color("#E14D2A").unwrap());
        host.draw_rectangle(10., 20., 200., 100.);
    }

    let path = std::env::temp_dir().join("lilyco-export-golden.png");
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<GraphiteExportPng>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "graphite-export-png"),
        serde_json::json!({
            "out": path.to_string_lossy(),
            "width": 256,
            "height": 256,
        }),
    );
    let result = outcome.result.expect("有 GPU 时 PNG 导出必须成功");
    assert!(result["bytes"].as_u64().unwrap() > 0, "PNG 应非空");

    let png = std::fs::read(&path).expect("PNG 文件必须存在");
    assert!(
        png.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
        "产物应为 PNG 魔数开头"
    );
    let _ = std::fs::remove_file(&path);
}
