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
    parse_hex_color, shared_host, GraphiteAddRect, GraphiteDocNew, GraphiteSave, GraphiteSetFill,
};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// 测试级串行锁：保证 "建文档 / 画图 / 执行 App" 的复合操作不被并行测试插入
fn driver() -> MutexGuard<'static, ()> {
    static DRIVER: OnceLock<Mutex<()>> = OnceLock::new();
    DRIVER.get_or_init(|| Mutex::new(())).lock().unwrap()
}

fn handler_of(reg: &Registry, name: &str) -> Handler {
    reg.get(name).unwrap().handler.clone().unwrap()
}

// ── headless 全链（核心 spike 验证点） ────────────────────

#[test]
fn headless_new_doc_add_rect_save_full_chain() {
    let _session = driver();
    let mut host = shared_host().lock().unwrap();

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

    // 4) 改填充不透明度（消息派发不 panic 即视为已受理）
    host.set_fill(0.5);

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
        after,
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
    shared_host().lock().unwrap().new_document("add-rect");

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
        let mut host = shared_host().lock().unwrap();
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
        let mut host = shared_host().lock().unwrap();
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
