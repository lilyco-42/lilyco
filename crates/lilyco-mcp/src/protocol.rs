//! 协议常量与 JSON-RPC 线格式：这一层只认「报文长什么样」，不认注册表。

/// MCP 协议版本（本实现声明的最低版本，也是协商不上时的回落值）
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// 本实现能协商的协议版本，新的在前。
///
/// 客户端在 `initialize` 里报 `params.protocolVersion`；给的是这里面的值就原样回，
/// 否则回 [`PROTOCOL_VERSION`]，由客户端自己决定是否继续（官方 SDK 会因此断开）。
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

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

/// 协商 `initialize` 的协议版本：客户端给的是我们支持的版本就原样回，
/// 否则回最低版本 [`PROTOCOL_VERSION`]。
///
/// 为什么必须「原样回」：官方 SDK（dsh 用的 @modelcontextprotocol/client 就是）在
/// legacy 握手后会校验 `legacyVersions.includes(result.protocolVersion)`，不在列表里
/// 直接抛 `Server's protocol version is not supported` 并断开连接。
pub(crate) fn negotiate_version(params: Option<&serde_json::Value>) -> &'static str {
    let offered = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(|v| v.as_str());
    match offered {
        Some(v) => SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .copied()
            .find(|s| *s == v)
            .unwrap_or(PROTOCOL_VERSION),
        None => PROTOCOL_VERSION,
    }
}

/// `initialize` 的结果。
///
/// `capabilities` 里的 `resources` 必须是**存在的键**（哪怕空对象）：不少客户端
/// （含 dsh 的 MCP 客户端）用 `serverCapabilities.resources !== undefined` 判断要不要
/// 真的发 `resources/list`，键缺失就直接返回空列表、一条请求都不发。
pub(crate) fn initialize_response(
    params: Option<&serde_json::Value>,
) -> Result<serde_json::Value, (i64, String)> {
    Ok(serde_json::json!({
        "protocolVersion": negotiate_version(params),
        "capabilities": {
            "tools": {},
            "resources": {},
        },
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
