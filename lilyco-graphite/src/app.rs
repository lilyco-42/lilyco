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

/// 取导出用文档字节：显式路径读盘；缺省用共享宿主当前文档的 save_content 产物
fn document_bytes(doc: &Option<String>) -> Result<Vec<u8>, AppError> {
    match doc {
        Some(path) => {
            std::fs::read(path).map_err(|e| AppError::Runtime(format!("读文档失败 {path}: {e}")))
        }
        None => with_host(|host| host.save_content().map(|(_, bytes)| bytes)),
    }
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

/// 渲染导出 SVG（T0，P1 出图决定性验证）
///
/// 链路：消息总线文档（或 .graphite 文件）→ DynamicExecutor 图求值 → SVG 字节落盘。
/// 纯 CPU 矢量路径（SvgRender），headless 无 GPU 也能跑。
#[derive(App)]
#[app(
    name = "graphite-export-svg",
    about = "把 Graphite 文档渲染为 SVG 文件（headless 矢量渲染，无 GPU 依赖）",
    run = "run_export_svg"
)]
pub struct GraphiteExportSvg {
    /// .graphite 文档路径（缺省 = 当前文档）
    doc: Option<String>,
    /// 输出 SVG 文件路径
    out: String,
    /// 导出缩放（缺省 1.0）
    scale: Option<f64>,
}

pub fn run_export_svg(
    app: &GraphiteExportSvg,
    ctx: &Context,
) -> Result<serde_json::Value, AppError> {
    let scale = app.scale.unwrap_or(1.);
    let out = app.out.clone();
    let bytes = document_bytes(&app.doc)?;

    let start = std::time::Instant::now();
    let svg = crate::export::render_svg(&bytes, scale).map_err(AppError::Runtime)?;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    std::fs::write(&out, &svg).map_err(|e| AppError::Runtime(format!("写 SVG 失败 {out}: {e}")))?;

    ctx.telemetry("graphite.op", serde_json::json!("export-svg"));
    ctx.telemetry("graphite.export_format", serde_json::json!("svg"));
    ctx.telemetry("graphite.export_bytes", serde_json::json!(svg.len()));
    ctx.telemetry("graphite.export_ms", serde_json::json!(elapsed_ms));
    let r =
        serde_json::json!({ "out": out, "format": "svg", "bytes": svg.len(), "ms": elapsed_ms });
    ctx.done(r.clone(), 0);
    Ok(r)
}

/// 渲染导出 PNG（T0，尽力而为——GPU/Vello 路径）
///
/// 无 GPU 环境返回带结论的错误（SVG 是主力路径）；有 GPU（独显或 Mesa 软件
/// GPU 栈 lavapipe/llvmpipe）时完整走 "Vello 光栅化 → 纹理回读 → PNG 编码"。
#[derive(App)]
#[app(
    name = "graphite-export-png",
    about = "把 Graphite 文档渲染为 PNG 文件（GPU 光栅化，无 GPU 环境会明确报错）",
    run = "run_export_png"
)]
pub struct GraphiteExportPng {
    /// .graphite 文档路径（缺省 = 当前文档）
    doc: Option<String>,
    /// 输出 PNG 文件路径
    out: String,
    /// 输出宽度（像素；与 height 同时给才生效，缺省随文档）
    width: Option<u32>,
    /// 输出高度（像素）
    height: Option<u32>,
    /// 导出缩放（缺省 1.0）
    scale: Option<f64>,
    /// 保留透明通道（缺省 false，合成不透明白底）
    transparent: Option<bool>,
}

pub fn run_export_png(
    app: &GraphiteExportPng,
    ctx: &Context,
) -> Result<serde_json::Value, AppError> {
    let scale = app.scale.unwrap_or(1.);
    let transparent = app.transparent.unwrap_or(false);
    let out = app.out.clone();
    let bytes = document_bytes(&app.doc)?;

    let start = std::time::Instant::now();
    let png = crate::export::render_png(&bytes, scale, app.width, app.height, transparent)
        .map_err(AppError::Runtime)?;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    std::fs::write(&out, &png).map_err(|e| AppError::Runtime(format!("写 PNG 失败 {out}: {e}")))?;

    ctx.telemetry("graphite.op", serde_json::json!("export-png"));
    ctx.telemetry("graphite.export_format", serde_json::json!("png"));
    ctx.telemetry("graphite.export_bytes", serde_json::json!(png.len()));
    ctx.telemetry("graphite.export_ms", serde_json::json!(elapsed_ms));
    let r =
        serde_json::json!({ "out": out, "format": "png", "bytes": png.len(), "ms": elapsed_ms });
    ctx.done(r.clone(), 0);
    Ok(r)
}
