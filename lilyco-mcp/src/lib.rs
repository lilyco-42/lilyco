//! # Lilyco MCP
//!
//! 把 Lilyco 命令注册表暴露为标准 **Model Context Protocol** (MCP) 服务器。
//! 实现 2024-11-05 协议子集：`initialize` / `ping` / `tools/list` / `tools/call`。
//!
//! 设计说明：
//! - 低耦合：只依赖 `lilyco-core`（registry + executor），不关心任何渲染端；
//!   新增一个 AI 后端 = 新增一个 crate，core 零改动。
//! - 可测试：核心逻辑是纯函数 [`McpServer::handle_line`]（一行请求 → 一行响应），
//!   [`McpServer::serve`] 可挂任意 `Read + Write` 对。
//! - 与官方 MCP SDK 的关系：这里手工实现最小 stdio 子集（零额外依赖），
//!   满足 Agent 直接调用；需要完整 SDK 能力（采样、roots、进度通知）时
//!   可另建 `lilyco-mcp-full` 基于 modelcontextprotocol/rust-sdk 包装，core 无需变化。
//!
//! ## 使用
//!
//! ```ignore
//! let registry = ...; // lilyco_core::Registry
//! lilyco_mcp::McpServer::new(registry).serve_stdio()?;
//! ```

use std::io::{BufRead, Write};

use lilyco_core::executor;
use lilyco_core::registry::Registry;

/// MCP 协议版本（2024-11-05）
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC 错误码
pub const ERROR_PARSE: i64 = -32700;
pub const ERROR_METHOD_NOT_FOUND: i64 = -32601;
pub const ERROR_INVALID_PARAMS: i64 = -32602;
pub const ERROR_INTERNAL: i64 = -32603;

/// 最小 MCP 服务器（stdio 传输）
pub struct McpServer {
    registry: Registry,
}

impl McpServer {
    /// 从命令注册表创建服务器
    pub fn new(registry: Registry) -> Self {
        Self { registry }
    }

    /// 处理一行 JSON-RPC 请求，返回响应 JSON 字符串。
    ///
    /// 通知（无 `id` 的请求，如 `notifications/initialized`）返回 `None`。
    pub fn handle_line(&self, line: &str) -> Option<String> {
        let req: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return Some(error_response(None, ERROR_PARSE, "parse error")),
        };

        let id = req.get("id").cloned();
        let method = req
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or_default();

        let result = match method {
            "initialize" => initialize_response(),
            "ping" => Ok(serde_json::json!({})),
            "tools/list" => self.tools_list(),
            "tools/call" => self.tools_call(&req),
            "notifications/initialized" => return None, // 通知无需响应
            _ => {
                return Some(error_response(
                    id.as_ref(),
                    ERROR_METHOD_NOT_FOUND,
                    &format!("method not found: {method}"),
                ))
            }
        };

        match result {
            Ok(res) => Some(success_response(id.as_ref(), res)),
            Err((code, msg)) => Some(error_response(id.as_ref(), code, &msg)),
        }
    }

    /// 在任意 `Read + Write` 对上提供服务（内存测试 / 自定义传输均可用）
    pub fn serve<R: BufRead, W: Write>(&self, mut reader: R, mut writer: W) -> std::io::Result<()> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break; // EOF
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(resp) = self.handle_line(trimmed) {
                writeln!(writer, "{resp}")?;
                writer.flush()?;
            }
        }
        Ok(())
    }

    /// 在 stdin/stdout 上提供服务（MCP 标准传输，供 Agent 直接 spawn）
    pub fn serve_stdio(&self) -> std::io::Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        self.serve(stdin.lock(), stdout.lock())
    }

    // ── 方法实现 ────────────────────────────────────────

    fn tools_list(&self) -> Result<serde_json::Value, (i64, String)> {
        let tools: Vec<serde_json::Value> = self
            .registry
            .visible()
            .map(|cmd| {
                serde_json::json!({
                    "name": cmd.name,
                    "description": cmd.schema.about,
                    "inputSchema": cmd.schema.to_json_schema(),
                })
            })
            .collect();
        Ok(serde_json::json!({ "tools": tools }))
    }

    fn tools_call(&self, req: &serde_json::Value) -> Result<serde_json::Value, (i64, String)> {
        let params = req.get("params").cloned().unwrap_or(serde_json::json!({}));
        let name = params
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or((ERROR_INVALID_PARAMS, "missing tool name"))?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        let cmd = self
            .registry
            .get(name)
            .ok_or((ERROR_INVALID_PARAMS, format!("unknown tool: {name}")))?;
        let handler = cmd
            .handler
            .clone()
            .ok_or((ERROR_INTERNAL, format!("tool `{name}` has no handler")))?;

        // 同步执行（最小实现）。进度通知（notifications/progress）留待完整版。
        let outcome = executor::execute(handler, args);
        match outcome.result {
            Ok(value) => Ok(serde_json::json!({
                "content": [{ "type": "text", "text": value.to_string() }],
                "isError": false,
            })),
            Err(e) => Ok(serde_json::json!({
                "content": [{ "type": "text", "text": e.to_string() }],
                "isError": true,
            })),
        }
    }
}

// ── JSON-RPC 装配 ────────────────────────────────────────

fn initialize_response() -> Result<serde_json::Value, (i64, String)> {
    Ok(serde_json::json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "lilyco",
            "version": env!("CARGO_PKG_VERSION"),
        }
    }))
}

fn success_response(id: Option<&serde_json::Value>, result: serde_json::Value) -> String {
    let mut resp = serde_json::Map::new();
    resp.insert("jsonrpc".into(), serde_json::json!("2.0"));
    resp.insert("id".into(), id.cloned().unwrap_or(serde_json::Value::Null));
    resp.insert("result".into(), result);
    serde_json::to_string(&serde_json::Value::Object(resp)).unwrap_or_default()
}

fn error_response(id: Option<&serde_json::Value>, code: i64, message: &str) -> String {
    let mut resp = serde_json::Map::new();
    resp.insert("jsonrpc".into(), serde_json::json!("2.0"));
    resp.insert("id".into(), id.cloned().unwrap_or(serde_json::Value::Null));
    resp.insert(
        "error".into(),
        serde_json::json!({ "code": code, "message": message }),
    );
    serde_json::to_string(&serde_json::Value::Object(resp)).unwrap_or_default()
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco_core::registry::{RegisteredCommand, Registry};
    use lilyco_core::schema::{ArgKind, ArgSchema, CommandSchema};
    use lilyco_core::{App, AppError, Context};
    use std::collections::HashMap;

    /// 一个可执行的最小 App：echo
    struct Echo;

    impl App for Echo {
        fn schema() -> CommandSchema {
            CommandSchema {
                name: "echo".into(),
                about: "echo the given text".into(),
                args: vec![ArgSchema {
                    name: "text".into(),
                    about: "text to echo".into(),
                    kind: ArgKind::Text,
                    required: true,
                    default: None,
                }],
                subcommands: vec![],
            }
        }

        fn from_args(args: &HashMap<String, serde_json::Value>) -> Result<Self, AppError> {
            if !args.contains_key("text") {
                return Err(AppError::InvalidArg("missing text".into()));
            }
            Ok(Echo)
        }

        fn run(&self, ctx: &Context) -> Result<serde_json::Value, AppError> {
            let r = serde_json::json!({ "echoed": true });
            ctx.done(r.clone(), 0);
            Ok(r)
        }
    }

    fn test_registry() -> Registry {
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::from_app::<Echo>()).unwrap();
        reg
    }

    #[test]
    fn initialize_returns_protocol_version() {
        let server = McpServer::new(test_registry());
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(v["result"]["capabilities"]["tools"], serde_json::json!({}));
    }

    #[test]
    fn tools_list_lists_visible_commands() {
        let server = McpServer::new(test_registry());
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let tools = v["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "echo");
        assert_eq!(tools[0]["inputSchema"]["type"], "object");
    }

    #[test]
    fn tools_call_executes_handler() {
        let server = McpServer::new(test_registry());
        let req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hi"}}}"#;
        let resp = server.handle_line(req).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], false);
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("echoed"));
    }

    #[test]
    fn tools_call_unknown_tool_is_error() {
        let server = McpServer::new(test_registry());
        let req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope"}}"#;
        let resp = server.handle_line(req).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], ERROR_INVALID_PARAMS);
    }

    #[test]
    fn notification_gets_no_response() {
        let server = McpServer::new(test_registry());
        assert!(server
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());
    }

    #[test]
    fn unknown_method_returns_error() {
        let server = McpServer::new(test_registry());
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":5,"method":"bogus"}"#)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], ERROR_METHOD_NOT_FOUND);
    }

    #[test]
    fn parse_error_returns_error() {
        let server = McpServer::new(test_registry());
        let resp = server.handle_line("not json at all").unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], ERROR_PARSE);
    }

    #[test]
    fn serve_roundtrips_over_memory_io() {
        let server = McpServer::new(test_registry());
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
        );
        let mut out = Vec::new();
        server.serve(std::io::Cursor::new(input), &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("\"result\":{}"), "ping response: {s}");
        assert!(s.contains("\"tools\""), "tools/list response: {s}");
        // 两行请求 → 两行响应
        assert_eq!(s.lines().count(), 2);
    }

    #[test]
    fn serve_skips_blank_lines() {
        let server = McpServer::new(test_registry());
        let input = "\n\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n";
        let mut out = Vec::new();
        server.serve(std::io::Cursor::new(input), &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap().lines().count(), 1);
    }
}
