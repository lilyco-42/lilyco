// 与 lib.rs 同款：graphite 类型链撑爆默认 trait 求解深度
#![recursion_limit = "512"]

//! lgraphite — Graphite 桥演示：一个二进制 = CLI + TUI + Web + MCP 四端 + 安全门 + 遥测
//!
//! ```bash
//! lgraphite graphite-doc-new --name demo            # 新建文档（T0 直通）
//! lgraphite graphite-add-rect --x 0 --y 0 --w 200 --h 100 --fill "#E14D2A"
//! lgraphite graphite-set-fill --fill 0.5            # T1：自动化面默认拒绝
//! lgraphite graphite-save --path demo.graphite      # 序列化落盘
//! lgraphite graphite-export-svg --out demo.svg      # P1 出图：SVG 矢量渲染（无 GPU）
//! lgraphite graphite-export-png --out demo.png      # P1 出图：PNG（需 GPU/软件 GPU 栈）
//! lgraphite --mcp                                   # MCP 服务器：Agent 可调
//! ```
//!
//! 出图链路：消息总线建图 → save_content 序列化 → load_network →
//! wrap_network_in_scope → Preprocessor → Compiler → DynamicExecutor →
//! RenderConfig{ExportFormat::Svg|Raster} 图求值 → 文件落盘（官方 graphene-cli 同款）。

use lilyco::prelude::*;
use lilyco_graphite::{
    GraphiteAddRect, GraphiteDocNew, GraphiteExportPng, GraphiteExportSvg, GraphiteSave,
    GraphiteSetFill,
};

fn main() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<GraphiteDocNew>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<GraphiteAddRect>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<GraphiteSetFill>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<GraphiteSave>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<GraphiteExportSvg>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<GraphiteExportPng>())
        .unwrap();

    match lilyco::detect() {
        lilyco::Backend::Mcp => lilyco::serve_mcp(reg),
        _ => lilyco::run_cli_registry("lgraphite", reg),
    }
}
