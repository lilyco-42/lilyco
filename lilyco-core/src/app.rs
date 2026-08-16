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

    /// 执行业务逻辑，通过 ctx 上报进度
    fn run(&self, ctx: &Context) -> Result<serde_json::Value, AppError>;
}

/// 渲染器：把 CommandSchema 转换成各端的表示
pub trait Renderer {
    /// 各端各自的输出类型
    type Output;

    /// 从 CommandSchema 渲染出该端的表示
    fn render(&self, schema: &CommandSchema) -> Self::Output;
}
