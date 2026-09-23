//! lvision — 图像视觉工具包，作为 lilyco App 提供给 AI（DeepSeek Harness 的视觉工具）。
//!
//! 重写自 DSH Vision Toolkit 的本地操作，同一份定义天然四端 + AI 可调：
//! ```bash
//! lvision --mcp                                  # MCP stdio 服务器（Agent 直接调用 8 个视觉工具）
//! lvision --list                                 # 输出注册表 JSON（schema 清单）
//! ```
//!
//! 工具清单（含区域/输出约定）：
//! - `image-info`：读取格式 / 尺寸 / 文件字节数
//! - `crop`：像素框裁剪（超出边界自动收紧）+ 可选放大，输出 PNG
//! - `resize`：缩放（0 表示保持比例），输出 PNG
//! - `dominant-colors`：主色分析（缩图采样 + 贪心聚类合并）
//! - `pixel-diff`：原图 vs 重建图的网格级差异热力排名
//! - `extract-foreground`：背景透明化（边界连通域洪水填充，确定性近似）
//! - `trace`：位图矢量化（vtracer，与上游 DSH 工具同一引擎），输出 SVG
//! - `html-screenshot`：无头浏览器截图（30s 超时 kill）
//!

mod common;
mod crop;
mod diff;
mod dominant;
mod foreground;
mod info;
mod resize;
mod screenshot;
mod trace;

use lilyco::prelude::*;

use crate::crop::Crop;
use crate::diff::PixelDiff;
use crate::dominant::DominantColors;
use crate::foreground::ExtractForeground;
use crate::info::ImageInfo;
use crate::resize::Resize;
use crate::screenshot::HtmlScreenshot;
use crate::trace::Trace;

// ── registry + main ────────────────────────────────────────

/// 注册全部 8 个视觉工具，供 MCP / --list 使用
fn build_registry() -> Registry {
    let mut r = Registry::new();
    for cmd in [
        RegisteredCommand::from_app::<ImageInfo>(),
        RegisteredCommand::from_app::<Crop>(),
        RegisteredCommand::from_app::<Resize>(),
        RegisteredCommand::from_app::<DominantColors>(),
        RegisteredCommand::from_app::<PixelDiff>(),
        RegisteredCommand::from_app::<ExtractForeground>(),
        RegisteredCommand::from_app::<Trace>(),
        RegisteredCommand::from_app::<HtmlScreenshot>(),
    ] {
        r.register(cmd).expect("register vision tool");
    }
    r
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--mcp") {
        lilyco::serve_mcp(build_registry());
        return;
    }
    if args.iter().any(|a| a == "--list") {
        println!("{}", build_registry().to_json());
        return;
    }
    eprintln!("usage: lvision --mcp | --list");
    std::process::exit(2);
}

#[cfg(test)]
mod tests;
