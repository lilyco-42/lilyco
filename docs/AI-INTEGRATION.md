## AI Integration

Every Lilyco app is an AI tool:

```bash
$ imgpress --anthropic-tool
```

```json
{
  "name": "ImgCompress",
  "description": "Compress image files",
  "input_schema": {
    "type": "object",
    "properties": {
      "input": { "type": "string", "description": "Input image file" },
      "quality": { "type": "number", "minimum": 1, "maximum": 100, "description": "Quality" },
      "format": { "type": "string", "enum": ["jpeg", "png", "webp"], "description": "Format" },
      "dry_run": { "type": "boolean", "description": "Dry run" }
    },
    "required": ["input"]
  }
}
```

This is a valid Anthropic tool-use definition. Drop it into your Claude API call, and the model can invoke your Rust tool directly.

```bash
$ imgpress --openai-tool             # Chat Completions 格式（嵌套 function）
$ imgpress --openai-responses-tool   # Responses API 格式（扁平，strict:false）
$ imgpress --openai-strict-tool      # strict mode（结构化输出：剥约束关键词 +
                                     #   additionalProperties:false + 全字段 required）
$ imgpress --gemini-tool             # Gemini functionDeclarations（OpenAPI 子集）
$ imgpress --anthropic-tool          # Anthropic tool_use
$ imgpress --schema                  # Generic JSON Schema (for other LLMs)
$ imgpress --mcp            # 标准 MCP 服务器：Agent 直接调用（含进度通知）
```

更进一步 —— **采样桥**（`HostBridge`）：MCP 形态下，工具执行中途可以反向调用
Agent 客户端的 LLM（`sampling/createMessage`），让"工具用上模型"而不只是
"模型用工具"：

```rust
fn run(app: &VisionTool, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let caption = ctx.sample("描述这张图片的内容", 256)?;   // 反向采样
    ctx.done(serde_json::json!({ "caption": caption }), 0);
    Ok(serde_json::Value::Null)
}
```

客户端未声明 `sampling` 能力时返回带指引的错误（审批权始终在客户端手里）。
$ imgpress --json-stream    # Each Progress event as one JSON line — ideal for agent consumption
```

### MCP Server（AI 调用的事实标准）

```bash
$ imgpress --mcp
```

`lilyco-mcp` 把命令注册表暴露为标准 **MCP stdio 服务器**（协议 2024-11-05）。
任何支持 MCP 的 Agent（Claude Desktop、Cursor、OpenHands 等）都可以直接调用你的 Rust 工具：

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ImgCompress","arguments":{"input":"photo.jpg","quality":50}}}
```

相比手写 `--anthropic-tool` / `--openai-tool` 单次 schema，MCP 是标准化的长连接协议，
一次 `tools/list` 拿全量工具定义，`tools/call` 直接执行并返回结构化结果。

### AI Agent Consumption Pattern

```jsonl
{"type":"started","total":5,"message":"Loading photo.jpg..."}
{"type":"tick","current":1,"total":5,"message":"Reading input file","percent":0.2}
{"type":"tick","current":2,"total":5,"message":"Original: 4000x3000","percent":0.4}
{"type":"tick","current":3,"total":5,"message":"Encoding...","percent":0.6}
{"type":"tick","current":4,"total":5,"message":"Writing compressed.jpg","percent":0.8}
{"type":"done","result":{"output_size":142000,"compression_ratio":35.5},"duration_ms":1200}
```

---

## Progress Protocol

Every interface consumes the same `Progress` events:

```rust
ctx.emit(Progress::Started { total: Some(100), message: Some("Starting...".into()) });

for i in 0..=100 {
    if ctx.is_cancelled() { return Err(AppError::Cancelled); }
    ctx.tick(i, Some(100), format!("Processing frame {i}"));
}

ctx.log(LogLevel::Info, "Compression complete");
ctx.done(serde_json::json!({"size_mb": 4.2}), 3200);
```

| Interface | `Started` | `Tick` | `Log` | `Done` |
|-----------|-----------|--------|-------|--------|
| **CLI** (`--json-stream`) | JSON line | JSON line with percent | JSON line | JSON line + exit |
| **CLI** (Human) | -- | `\r` progress line | `[INFO]` line | summary + exit |
| **TUI** | Progress bar at 0% | Bar fills + message | Scroll log | Result screen |
| **Web** | SSE: bar at 0% | SSE: bar fills | SSE: log append | SSE: result JSON |

---

