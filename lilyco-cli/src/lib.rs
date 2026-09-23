//! # Lilyco CLI 后端
//!
//! 把 `CommandSchema` 渲染成 clap 子命令树、把解析结果还原成参数 map、把执行进度
//! 打到 stdout。**不做业务**：命令侧只实现 `App`，这里负责「怎么变成命令行」。
//!
//! 模块划分：
//! - `renderer`  `CliRenderer`：渲染入口 + 内置标志 + 参数提取
//! - `command`   schema → `clap::Command` 的构造规则本体
//! - `single`    单命令一行启动
//! - `registry`  多命令（`Registry` → 子命令 + 进度输出）
//!
//! 对外只有四个符号：[`CliRenderer`]、[`run`]、[`run_registry`]、[`build_registry_command`]。

mod command;
mod registry;
mod renderer;
mod single;

pub use crate::registry::{build_registry_command, run_registry};
pub use crate::renderer::CliRenderer;
pub use crate::single::run;

#[cfg(test)]
mod tests;
