# 附录 B · 常见报错

> 九条来自本仓库实测的报错/坑。每条给：原文 → 原因 → 修法。
> 前七条会明确报错；后两条不报错但更隐蔽。

---

## B.1 `--gui` 用不了

```text
error: the argument '--gui' cannot be used with one or more of the other specified arguments

Usage: imgpress [OPTIONS] --input <input>
```

**原因**：依赖没开 `web` 特性。facade 默认 `default = []`，只编 CLI + MCP；
`--gui` 的 clap 定义存在，但没有 Web 后端，于是被当成互斥参数。

**修法**：

```toml
lilyco = { path = "...", features = ["tui", "web"] }
```

改完 `cargo build` 再跑。只想要终端表单可以只开 `tui`。

**复现**：`cargo run -p lilyco-example -- --input ... --gui`（该示例未开 `web`）→ 退出码 2。

---

## B.2 多命令下 `--mcp` 不存在

```text
error: unexpected argument '--mcp' found

Usage: multi [OPTIONS] [COMMAND]
```

**原因**：用了 `run_cli_registry`（纯 CLI 入口）。它不注册 MCP 相关参数。

**修法**：改用 `lilyco::run_registry("name", reg)`。
（`run_cli_registry` 仍然有用：明确只想发 CLI 时，能省掉 MCP 依赖。）

**复现**：`cargo run -p lilyco-example --example multi -- --mcp` → 退出码 2。

---

## B.3 多命令下顶层写 `--json`

```text
error: unexpected argument '--json' found

Usage: ch04 [OPTIONS] [COMMAND]
```

**原因**：多命令的顶层只管注册表（`--schema` / `--mcp` / `--gui`），
`--json` 这类端参数挂在**子命令**上。

**修法**：

```bash
filecount --json                     # ❌
filecount count --file x --json      # ✅
```

同一个坑还有 `--json-stream`、`--anthropic-tool`、`--openai-tool`、`--gemini-tool`。

---

## B.4 路径校验在业务函数之前

```text
error: invalid value '/nope' for '--file <file>': path does not exist: /nope
```

**原因**：`#[arg(must_exist = true)]` 生效了。这是**正常行为**，不是 bug ——
校验由框架统一做，四端一致。

**修法**：不需要修。若你真的允许路径不存在（比如「创建新文件」），去掉 `must_exist`，
在自己的函数里判断。

值得注意：这条错误发生时，你的 `run_xxx` **根本没被调用**。
不要在这里找业务日志。

---

## B.5 `range` 带下划线，上限静默丢失

没有报错，但 schema 变了：

```json
"kind": { "type": "number", "min": 0.0, "max": null }
```

**原因**：宏用 `parse::<f64>()` 解析区间两端，`1_000_000` 解析失败，`max` 被丢弃。

**修法**：

```rust
#[arg(default = 0, range = 0..=1000000)]   // ✅
#[arg(default = 0, range = 0..=1_000_000)] // ❌ max 变 null
```

这是最危险的一类：不红、不警告，页面只是少了一行「取值 0 – 1000000」的提示。

**复现**：写 `#[arg(range = 0..=1_000_000)]` 后跑 `--schema` → 退出码 0，`"max": null`。

---

## B.6 `Vec<u32>` 编译不过

```text
error[E0277]: a value of type `Vec<u32>` cannot be built from an iterator over elements of type `std::string::String`
   --> examples/ch_vec.rs:4:10
    |
  4 | #[derive(App)]
    |          ^^^ value of type `Vec<u32>` cannot be built from `std::iter::Iterator<Item=std::string::String>`
    |
help: the trait `FromIterator<std::string::String>` is not implemented for `Vec<u32>`
      but trait `FromIterator<u32>` is implemented for it
```

**原因**：宏取 List 值时一律按字符串收集（`filter_map(as_str).collect()`），
所以只有 `Vec<String>` 能构造成功。但 schema 会把 item 正确声明成 `Number`，
Web 页面也能画出数字行 —— 不一致点在宏侧。

**修法（现阶段）**：

- 用 `Vec<String>`，在函数里自己 `parse()`；
- 或等宏侧按 item 类型转换（这属于框架问题，不是页面问题，别在 Web 里绕）。

---

## B.7 MCP 参数错误的形态

```json
{"error":{"code":-32602,"message":"参数错误: 路径不存在: file = /nope"},"id":4,"jsonrpc":"2.0"}
```

**原因**：参数校验没通过，调用**没有成立**。

**修法**：改参数后重试。不要把它当成「工具执行失败」——
业务失败的形态是 `result.content` 里带错误文案（`isError: true` 或错误文本），
两者的重试策略不同。

---

## B.8 字段无人读的警告（不红但坏）

```text
warning: field `quiet` is never read
   --> src/main.rs:25:5
    |
 25 |     quiet: bool,
    |     ^^^^^
```

**原因**：声明了参数但业务函数没用到。它仍然会出现在 schema 与表单里 ——
用户填了，什么也没发生。

**修法**：二选一：

1. 用起来（第 3 章把它接进日志分支）；
2. 删掉。**不要**用 `#[allow(dead_code)]` 掩盖 —— 那会让「表单上有个永远没效果的开关」
   一直留在产品里。

---

## B.9 取消失败：会话不存在

页面日志会出现：

```text
取消失败：no such running session
```

**原因**：`POST /cancel/{sid}` 只在执行路登记了取消句柄时才找得到会话。
自定义 `RunnerFn` 的执行路不登记。

**修法**：用 `run` / `run_registry` 这条主路；页面会按服务端能力决定画不画取消按钮。
如果按了才报错，说明前端与服务端能力对齐有问题，应该让按钮不出现，而不是给一句 404。

**复现**：从页面 `meta[name="lilyco-token"]` 取令牌后
`POST /cancel/does-not-exist` → HTTP 404，body 为 `no such running session`。

---

## 速查表

| 症状 | 条目 |
|---|---|
| `--gui` 报参数互斥 | B.1 |
| `--mcp` 不存在 | B.2 |
| `--json` 不存在 | B.3 |
| 路径报 does not exist | B.4 |
| 数字上限没显示 | B.5 |
| `Vec<u32>` 编译错 | B.6 |
| MCP 返回 -32602 | B.7 |
| 参数没效果、有警告 | B.8 |
| 取消返回 404 | B.9 |