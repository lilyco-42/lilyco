//! 协议常量与 JSON-RPC 线格式：这一层只认「报文长什么样」，不认注册表。

/// MCP 协议版本（2024-11-05）
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC 错误码
pub const ERROR_PARSE: i64 = -32700;
pub const ERROR_METHOD_NOT_FOUND: i64 = -32601;
pub const ERROR_INVALID_PARAMS: i64 = -32602;
pub const ERROR_INTERNAL: i64 = -32603;

/// 构造一条 `notifications/progress`（MCP 2024-11-05）。
/// `total` / `message` 为 `None` 时省略字段。
pub(crate) fn progress_notification(
    token: &serde_json::Value,
    progress: f64,
    total: Option<u64>,
    message: Option<String>,
) -> String {
    let mut params = serde_json::Map::new();
    params.insert("progressToken".into(), token.clone());
    params.insert("progress".into(), serde_json::json!(progress));
    if let Some(t) = total {
        params.insert("total".into(), serde_json::json!(t));
    }
    if let Some(m) = message {
        params.insert("message".into(), serde_json::json!(m));
    }
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/progress",
        "params": params,
    })
    .to_string()
}

pub(crate) fn initialize_response() -> Result<serde_json::Value, (i64, String)> {
    Ok(serde_json::json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "lilyco",
            "version": env!("CARGO_PKG_VERSION"),
        }
    }))
}

pub(crate) fn success_response(
    id: Option<&serde_json::Value>,
    result: serde_json::Value,
) -> String {
    let mut resp = serde_json::Map::new();
    resp.insert("jsonrpc".into(), serde_json::json!("2.0"));
    resp.insert("id".into(), id.cloned().unwrap_or(serde_json::Value::Null));
    resp.insert("result".into(), result);
    serde_json::to_string(&serde_json::Value::Object(resp)).unwrap_or_default()
}

pub(crate) fn error_response(id: Option<&serde_json::Value>, code: i64, message: &str) -> String {
    let mut resp = serde_json::Map::new();
    resp.insert("jsonrpc".into(), serde_json::json!("2.0"));
    resp.insert("id".into(), id.cloned().unwrap_or(serde_json::Value::Null));
    resp.insert(
        "error".into(),
        serde_json::json!({ "code": code, "message": message }),
    );
    serde_json::to_string(&serde_json::Value::Object(resp)).unwrap_or_default()
}
