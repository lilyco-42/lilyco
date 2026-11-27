use std::collections::HashMap;

use crate::context::Context;
use crate::error::AppError;
use crate::schema::CommandSchema;

/// 用户需要实现的唯一 trait
///
/// 后续 `#[derive(App)]` 过程宏会自动实现，手动实现也支持。
pub trait App: Sized {
    /// 返回这个命令的完整 schema（用于 CLI 生成、AI schema 导出）
    fn schema() -> CommandSchema;

    /// 从解析后的参数 map 构造自身（CLI 调用路径）
    fn from_args(args: &HashMap<String, serde_json::Value>) -> Result<Self, AppError>;

    /// 执行
    fn run(&self, ctx: &Context) -> Result<serde_json::Value, AppError>;

    /// 静态视图槽位（`#[view(id = "...", title = "...", kind = "...")]`）
    ///
    /// 默认无槽位。运行时用 [`Context::view`] 按同一 id 填充。
    fn views() -> Vec<crate::view::ViewSlot> {
        Vec::new()
    }

    /// Optional command and argument descriptions packaged as a TOML language catalog.
    fn ui_messages() -> Option<&'static str> {
        None
    }
}

/// 渲染器：把 CommandSchema 转换成各端的表示
pub trait Renderer {
    /// 各端各自的输出类型
    type Output;

    /// 从 CommandSchema 渲染出该端的表示
    fn render(&self, schema: &CommandSchema) -> Self::Output;
}
