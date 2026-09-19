//! Graphite 桥的四个派生应用——同一 struct 派生 CLI/TUI/Web/MCP，天生受安全门控
//!
//! - [`GraphiteDocNew`]：T0 新建文档（AI 画布的起点）
//! - [`GraphiteAddRect`]：T0 画矩形（指针消息序列模拟，AI 放第一个形状）
//! - [`GraphiteSetFill`]：T1 改选中图层填充——自动化面默认拒绝（P0 安全门示范），
//!   交互面注入 `Interactive` 策略后放行
//! - [`GraphiteSave`]：T0 序列化文档到文件（.graphite/.gdd 字节落盘）
//!
//! 四个工具共享进程级唯一内核实例（[`shared_host`]），因此 MCP 场景下
//! "doc-new → add-rect → set-fill → save" 天然操作同一份文档状态。

use lilyco::prelude::*;

use crate::host::{parse_hex_color, shared_host};
use crate::GraphiteHost;

/// 取共享宿主并上锁（锁中毒转 AppError）
fn with_host<T>(f: impl FnOnce(&mut GraphiteHost) -> Result<T, String>) -> Result<T, AppError> {
    let mut host = shared_host()
        .lock()
        .map_err(|e| AppError::Runtime(format!("Graphite 宿主锁中毒: {e}")))?;
    f(&mut host).map_err(AppError::Runtime)
}

/// 新建 Graphite 文档（T0 只读级，AI 画布起点）
#[derive(App)]
#[app(
    name = "graphite-doc-new",
    about = "新建一个 Graphite 文档（headless 驱动内核消息总线）",
    run = "run_doc_new"
)]
pub struct GraphiteDocNew {
    /// 文档名（缺省 "未命名文档"）
    name: Option<String>,
}

pub fn run_doc_new(app: &GraphiteDocNew, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let name = app.name.clone().unwrap_or_else(|| "未命名文档".to_string());
    let nodes = with_host(|host| {
        host.new_document(&name);
        host.node_count()
    })?;
    // P1 遥测：操作名 + 文档节点数，Agent 实时看到画布状态
    ctx.telemetry("graphite.op", serde_json::json!("doc-new"));
    ctx.telemetry("graphite.nodes", serde_json::json!(nodes));
    let r = serde_json::json!({ "document": name, "nodes": nodes });
    ctx.done(r.clone(), 0);
    Ok(r)
}

/// 画一个矩形图层（T0）：模拟矩形工具的指针拖拽消息序列
#[derive(App)]
#[app(
    name = "graphite-add-rect",
    about = "在当前文档画一个矩形图层（x/y 为左上角，w/h 为宽高）",
    run = "run_add_rect"
)]
pub struct GraphiteAddRect {
    /// 矩形左上角 x（编辑器坐标）
    x: f64,
    /// 矩形左上角 y
    y: f64,
    /// 矩形宽度（> 0）
    w: f64,
    /// 矩形高度（> 0）
    h: f64,
    /// 填充色 hex（#RRGGBB 或 #RRGGBBAA，缺省沿用上一次主色）
    fill: Option<String>,
}

pub fn run_add_rect(app: &GraphiteAddRect, ctx: &Context) -> Result<serde_json::Value, AppError> {
    if app.w <= 0. || app.h <= 0. {
        return Err(AppError::Runtime(format!(
            "宽高需为正数: w={}, h={}",
            app.w, app.h
        )));
    }
    let fill = app.fill.clone();
    let nodes = with_host(|host| {
        if let Some(hex) = &fill {
            let color = parse_hex_color(hex)?;
            host.set_primary_color(color);
        }
        host.draw_rectangle(app.x, app.y, app.w, app.h);
        host.node_count()
    })?;
    ctx.telemetry("graphite.op", serde_json::json!("add-rect"));
    ctx.telemetry("graphite.nodes", serde_json::json!(nodes));
    let r = serde_json::json!({ "rect": [app.x, app.y, app.w, app.h], "nodes": nodes });
    ctx.done(r.clone(), 0);
    Ok(r)
}

/// 修改选中图层填充不透明度（T1 需人工确认——自动化面默认拒绝）
#[derive(App)]
#[app(
    name = "graphite-set-fill",
    about = "修改选中图层的填充不透明度（0.0-1.0，需人工确认）",
    run = "run_set_fill",
    safety = "t1"
)]
pub struct GraphiteSetFill {
    /// 填充不透明度（0.0-1.0，越界自动钳制）
    fill: f64,
}

pub fn run_set_fill(app: &GraphiteSetFill, ctx: &Context) -> Result<serde_json::Value, AppError> {
    with_host(|host| {
        host.set_fill(app.fill);
        Ok(())
    })?;
    ctx.telemetry("graphite.op", serde_json::json!("set-fill"));
    ctx.telemetry("graphite.fill", serde_json::json!(app.fill.clamp(0., 1.)));
    let r = serde_json::json!({ "fill": app.fill.clamp(0., 1.) });
    ctx.done(r.clone(), 0);
    Ok(r)
}

/// 序列化当前文档并落盘（T0）
#[derive(App)]
#[app(
    name = "graphite-save",
    about = "把当前 Graphite 文档序列化（.graphite/.gdd）写入指定路径",
    run = "run_save"
)]
pub struct GraphiteSave {
    /// 输出文件路径
    path: String,
}

pub fn run_save(app: &GraphiteSave, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let path = app.path.clone();
    let saved = with_host(|host| host.save_content())?;
    let (name, bytes) = saved;
    std::fs::write(&path, &bytes)
        .map_err(|e| AppError::Runtime(format!("写文件失败 {path}: {e}")))?;
    ctx.telemetry("graphite.op", serde_json::json!("save"));
    ctx.telemetry("graphite.bytes", serde_json::json!(bytes.len()));
    let r = serde_json::json!({ "path": path, "file_name": name, "bytes": bytes.len() });
    ctx.done(r.clone(), 0);
    Ok(r)
}
