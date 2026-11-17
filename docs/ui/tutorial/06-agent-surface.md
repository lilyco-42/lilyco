# 第 6 章 · 给 agent 用

> **本章你会学到**
> 1. MCP 是什么、为什么四个端里它是「给 agent 的那一端」；
> 2. `initialize` / `tools/list` / `tools/call` 三个请求的真实形态；
> 3. tool 名与 `inputSchema` 从哪里来；
> 4. 参数错误在 MCP 上长什么样，以及 T1 命令为什么会被拦。

这一章不需要写新代码 —— 第 4 章的程序已经具备全部能力。你只是换一个客户端去用它。

---

## 6.1 为什么是 MCP

前面三个端是给人用的：CLI 给脚本，TUI 给手边，Web 给眼睛。
MCP（Model Context Protocol）是给 **agent** 用的：一个基于 JSON-RPC 的 stdio 协议，
Claude Desktop、Cursor、Codex 这类客户端都能直接接。

和其他「导出工具定义」的方式相比，MCP 的关键差别是**长连接 + 可发现**：

| 方式 | 形态 | 适合 |
|---|---|---|
| `--schema` | 一次性打印 JSON | 人看、脚本读 |
| `--anthropic-tool` / `--openai-tool` | 一次性打印某家格式 | 塞进单次请求 |
| `--mcp` | 常驻 stdio 服务 | agent 反复发现与调用 |

---

## 6.2 三步走

启动：

```bash
cargo run -- --mcp
```

它不会打印欢迎语，只是等 stdin 上的 JSON-RPC。用管道喂三个请求就能看全：

### 第一步：initialize

请求：

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"book","version":"1"}}}
```

实测响应：

```json
{"id":1,"jsonrpc":"2.0","result":{"capabilities":{"tools":{}},"protocolVersion":"2024-11-05","serverInfo":{"name":"lilyco","version":"0.3.0"}}}
```

### 第二步：tools/list

请求：

```json
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
```

实测响应（第 4 章那两个命令，节选）：

```json
{"id":2,"jsonrpc":"2.0","result":{"tools":[
  {"name":"count","description":"统计文本文件的行数、字节数或词数",
   "inputSchema":{"type":"object","required":["file"],
     "properties":{
       "file":{"type":"string","description":"要统计的文件路径"},
       "limit":{"type":"number","minimum":0.0,"maximum":1000000.0,"description":"最多统计多少行（0 = 不限）"},
       "mode":{"type":"string","enum":["lines","bytes","words"],"description":"统计口径"},
       "quiet":{"type":"boolean","description":"只输出总数，不打印明细"},
       "tags":{"type":"array","items":{"type":"string"},"description":"附加标签（可写多个，仅演示列表控件）"}
     }}},
  {"name":"echo","description":"原样回显输入的一段文本",
   "inputSchema":{"type":"object","required":["text"],"properties":{"text":{"type":"string","description":"要回显的文本"}}}}
]}}
```

三个细节：

- `name` 取 `#[app(name = "...")]`，不是 struct 名；
- `required` 与 `properties` 由 `required` / `default` 派生；
- `minimum` / `maximum` 就是 `range` 的两端。

### 第三步：tools/call

请求：

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"count","arguments":{"file":"/tmp/lilyco-book/Cargo.toml","mode":"bytes"}}}
```

实测响应：

```json
{"id":3,"jsonrpc":"2.0","result":{"content":[{"text":"{\"file\":\"/tmp/lilyco-book/Cargo.toml\",\"mode\":\"bytes\",\"value\":182}","type":"text"}],"isError":false}}
```

`content[0].text` 里是你返回的 `serde_json::Value` 序列化后的字符串。
**agent 拿到的就是它**，所以结果本身要自解释：字段名清楚、别只返回一个裸数字。

---

## 6.3 错误有两种，别混淆

### 参数不合法 → JSON-RPC error

请求 `{"name":"count","arguments":{"file":"/nope"}}` 实测：

```json
{"error":{"code":-32602,"message":"参数错误: 路径不存在: file = /nope"},"id":4,"jsonrpc":"2.0"}
```

这不是「工具返回了失败」，而是「这次调用根本没成立」。agent 看到 `-32602` 应该修参数再试。

### 业务失败 → 工具结果里的错误

你的函数返回 `Err(AppError::Runtime(...))` 时，框架把它转成工具的失败结果（`isError` / 错误文案），
agent 会把它当成一次已执行的失败，而不是参数错误。

两者的区别对 agent 很重要：前者重试前先改参数，后者重试前先改环境。

---

## 6.4 进度通知

第 3 章的 `ctx.tick` / `ctx.log` 在 MCP 上变成 progress 通知（复用已协商的 progress 通道）。
长任务因此不必「闷着跑」—— agent 能边等边看到进展。

`Progress::Telemetry` 也会被转发为 `key=value` 消息，适合传感器 / PLC 这类持续状态。

---

## 6.5 安全门：能看见不等于能执行

`run_registry` 在 MCP 面注入 `DenyElevated` 策略。结果是：

| 命令声明 | agent 侧 |
|---|---|
| `safety = "read_only"`（默认） | 可发现、可执行 |
| `safety = "confirm"`（T1：写盘 / 外呼） | **可发现，执行被拒** |

这是有意的：agent 可以「提案」一个危险动作，执行由人在 CLI / Web 面确认。
所以为命令声明正确的 `safety` 不是形式主义 —— 它决定 agent 有没有权限自己动手。

---

## 6.6 工具定义导出

单命令二进制支持四种导出：

```bash
cargo run -- --anthropic-tool     # input_schema
cargo run -- --openai-tool        # function.parameters
cargo run -- --openai-strict-tool # 结构化输出版本
cargo run -- --gemini-tool        # functionDeclarations
```

多命令时，这些参数挂在**子命令**上（第 4 章那条规矩）：

```bash
cargo run -- count --anthropic-tool   # ✅
cargo run -- --anthropic-tool         # ❌ 顶层没有这条参数
```

---

## 6.7 接进客户端

任何支持 MCP stdio 的客户端，需要的就是「怎么启动这个进程」：

```json
{
  "mcpServers": {
    "filecount": {
      "command": "/absolute/path/to/filecount",
      "args": ["--mcp"]
    }
  }
}
```

两个建议：

1. 用**绝对路径**。客户端不一定继承你的 `PATH`；
2. 先手动跑一次 `printf ... | filecount --mcp` 确认能握手，再把配置贴进去 ——
   否则客户端里只会显示一句「连接失败」。

---

## 6.8 现在你应该看到

用下面的命令一次跑完三步（先确认二进制路径）：

```bash
BIN=$(cargo build --message-format=json 2>/dev/null | python3 -c 'import sys,json
for l in sys.stdin:
    m=json.loads(l)
    if m.get("reason")=="compiler-artifact" and m.get("executable"):
        print(m["executable"]); break')
printf '%s\n' \
 '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' \
 '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
 | "$BIN" --mcp
```

| 检查 | 应该看到 |
|---|---|
| initialize | `serverInfo` 里是 `lilyco` 与版本号 |
| tools/list | 你的每个可见命令一条 tool，名字与 `#[app(name)]` 一致 |
| tools/call（合法参数） | `content[0].text` 是结果 JSON，`isError: false` |
| tools/call（坏路径） | JSON-RPC `error.code == -32602` |
| T1 命令 | 在 `tools/list` 可见，`tools/call` 被拒 |

---

## 小结

- MCP 是 stdio 上的 JSON-RPC：`initialize` → `tools/list` → `tools/call`。
- tool 名与 `inputSchema` 全部由 schema 派生，你不需要写协议代码。
- 参数错误是 JSON-RPC `-32602`，业务失败是工具结果里的错误 —— 别混。
- `safety` 决定 agent 能不能执行，而不只是能不能看见。
- 多命令的工具定义导出挂在子命令上。

---

## 练习

1. **用 `jq` 读一次真实调用**：把 `tools/call` 的输出管道给 `jq -r '.result.content[0].text' | jq .`，
   确认拿到的是结构化结果而不是转义字符串。
2. **给 `echo` 声明 `safety = "confirm"`**，重启 MCP，再调一次，观察它是否被拒。
   提示：改完要重新编译；MCP 面策略在启动时注入。
3. **比较三种导出**：`--anthropic-tool`、`--openai-tool`、`--gemini-tool` 各跑一次，
   找出三者对 `enum` 与 `number` 区间的表达差异。

---

## 下一章

[第 7 章 · 发布与验收](07-ship-and-verify.md)：release 构建、跨平台、交付前清单。