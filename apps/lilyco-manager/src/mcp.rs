use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_TOOLS_PER_APP: usize = 200;
const MAX_SCAN_LINES: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub struct McpTool {
    /// 命名空间化名称：`<app>__<tool>`，避免多个应用工具重名。
    pub name: String,
    pub app: String,
    pub original_name: String,
    pub description: String,
    pub input_schema: Value,
}

/// 启动应用自带的 MCP stdio 服务，完成 initialize + tools/list 后立即结束子进程。
/// 只使用清单里的固定 argv；HTTP 调用方无法传入命令或参数。
pub async fn discover(
    app_id: &str,
    program: &Path,
    args: &[String],
) -> Result<Vec<McpTool>, String> {
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| format!("启动 MCP 失败: {err}"))?;

    let mut stdin = child.stdin.take().ok_or("无法获取 MCP stdin")?;
    let stdout = child.stdout.take().ok_or("无法获取 MCP stdout")?;
    let stderr = child.stderr.take().ok_or("无法获取 MCP stderr")?;
    let stderr_buf = Arc::new(Mutex::new(String::new()));
    let stderr_task = tokio::spawn(drain_stderr(stderr, stderr_buf.clone()));

    let mut lines = BufReader::new(stdout).lines();
    let exchange = async {
        write_json(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "lilyco-manager", "version": env!("CARGO_PKG_VERSION")}
                }
            }),
        )
        .await?;
        read_response(&mut lines, 1).await?;

        write_json(
            &mut stdin,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await?;
        write_json(
            &mut stdin,
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
        )
        .await?;
        let response = read_response(&mut lines, 2).await?;
        parse_tools(app_id, &response)
    };

    let result = tokio::time::timeout(Duration::from_secs(10), exchange).await;
    let _ = child.start_kill();
    let _ = child.wait().await;
    stderr_task.abort();

    match result {
        Err(_) => Err(format!("MCP 发现超时: {app_id}")),
        Ok(Err(err)) => {
            let stderr = stderr_buf.lock().await.clone();
            if stderr.trim().is_empty() {
                Err(err)
            } else {
                Err(format!("{err} (stderr: {})", truncate(&stderr, 400)))
            }
        }
        Ok(Ok(tools)) => Ok(tools),
    }
}

async fn write_json<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> Result<(), String> {
    let mut line = serde_json::to_vec(value).map_err(|err| err.to_string())?;
    line.push(b'\n');
    writer
        .write_all(&line)
        .await
        .map_err(|err| format!("写入 MCP 失败: {err}"))?;
    writer
        .flush()
        .await
        .map_err(|err| format!("写入 MCP 失败: {err}"))
}

async fn read_response<R: AsyncBufRead + Unpin>(
    lines: &mut tokio::io::Lines<R>,
    id: u64,
) -> Result<Value, String> {
    for _ in 0..MAX_SCAN_LINES {
        let line = lines
            .next_line()
            .await
            .map_err(|err| format!("读取 MCP 失败: {err}"))?
            .ok_or("MCP 进程提前退出")?;
        if line.len() > MAX_LINE_BYTES {
            return Err("MCP 响应行过大".into());
        }
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("id").and_then(Value::as_u64) == Some(id) {
            if let Some(error) = value.get("error") {
                return Err(format!("MCP 返回错误: {error}"));
            }
            return Ok(value);
        }
    }
    Err("MCP 未返回期望的响应".into())
}

fn parse_tools(app_id: &str, response: &Value) -> Result<Vec<McpTool>, String> {
    let tools = response
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .ok_or("tools/list 缺少 result.tools")?;
    let mut output = Vec::new();
    for tool in tools.iter().take(MAX_TOOLS_PER_APP) {
        let Some(original_name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        if !valid_tool_name(original_name) {
            continue;
        }
        output.push(McpTool {
            name: namespaced(app_id, original_name),
            app: app_id.to_string(),
            original_name: original_name.to_string(),
            description: tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            input_schema: tool
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object"})),
        });
    }
    Ok(output)
}

pub fn namespaced(app_id: &str, tool: &str) -> String {
    let app = app_id.replace(['-', '.'], "_");
    format!("{app}__{tool}")
}

fn valid_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

async fn drain_stderr<R: tokio::io::AsyncRead + Unpin>(reader: R, buf: Arc<Mutex<String>>) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut buf = buf.lock().await;
        if buf.len() < 8192 {
            buf.push_str(&line);
            buf.push('\n');
        }
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_tool_names() {
        assert_eq!(namespaced("lycomc", "install"), "lycomc__install");
        assert_eq!(
            namespaced("lilyco.manager", "health"),
            "lilyco_manager__health"
        );
        assert_eq!(namespaced("my-app", "run"), "my_app__run");
    }

    #[test]
    fn parses_tools_list() {
        let response = json!({
            "id": 2,
            "result": {"tools": [
                {"name": "install", "description": "安装", "inputSchema": {"type": "object"}},
                {"name": "bad name", "description": "skip"}
            ]}
        });
        let tools = parse_tools("lycomc", &response).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "lycomc__install");
        assert_eq!(tools[0].original_name, "install");
    }
}
