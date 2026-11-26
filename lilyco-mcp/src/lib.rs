//! # Lilyco MCP
//!
//! 把 Lilyco 命令注册表暴露为标准 **Model Context Protocol** (MCP) 服务器。
//!
//! 实现的方法：`initialize`（协议版本按客户端协商：2025-11-25 / 2025-06-18 /
//! 2025-03-26 / 2024-11-05，协商不上回落 2024-11-05）、`ping`、`tools/list`、
//! `tools/call`（携带 `_meta.progressToken` 时流式返回 `notifications/progress`）、
//! `resources/list` / `resources/read`（每条命令一个 `lilyco://command/<name>` 资源，
//! 内容是它的 schema）、`resources/templates/list` 与 `prompts/list`（空表，避免
//! 客户端因 -32601 报错），以及 `tools/call` 结果里的图片内容块
//! （命令返回值带 `images[].data_base64` 时译成 `{ "type": "image" }`）。
//!
//! 设计说明：
//! - 低耦合：只依赖 `lilyco-core`（registry + executor），不关心任何渲染端；
//!   新增一个 AI 后端 = 新增一个 crate，core 零改动。
//! - 可测试：核心逻辑是纯函数 [`McpServer::handle_line`]（一行请求 → 一行响应），
//!   [`McpServer::serve`] 可挂任意 `Read + Write` 对。
//! - 与官方 MCP SDK 的关系：这里手工实现最小 stdio 子集（零额外依赖），
//!   满足 Agent 直接调用；采样 / roots 等完整能力可另建 `lilyco-mcp-full`
//!   基于 modelcontextprotocol/rust-sdk 包装，core 无需变化。
//! - 兼容性：`capabilities` 里 `tools` 与 `resources` 两个键都必须存在 ——
//!   有些客户端（如 dsh）用 `serverCapabilities.resources !== undefined` 决定
//!   要不要真的发 `resources/list`。
//!
//! 模块划分：
//! - `protocol` 协议常量 + JSON-RPC 报文装配（含进度通知）
//! - `server`   方法分发与传输外壳（stdio / 任意 Read+Write）
//! - `bridge`   server → client 反向请求（sampling / roots）
//!
//! ## 使用
//!
//! ```ignore
//! let registry = ...; // lilyco_core::Registry
//! lilyco_mcp::McpServer::new(registry).serve_stdio()?;
//! ```

mod bridge;
mod protocol;
mod server;

pub use crate::protocol::{
    ERROR_INTERNAL, ERROR_INVALID_PARAMS, ERROR_METHOD_NOT_FOUND, ERROR_PARSE,
};
pub use crate::protocol::{PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS};
pub use crate::server::McpServer;

#[cfg(test)]
mod tests;
