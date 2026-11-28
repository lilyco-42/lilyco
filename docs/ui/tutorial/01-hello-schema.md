# 第 1 章 · 第一个命令

> **本章你会学到**
> 1. 建一个依赖 lilyco 的 crate，并开对特性；
> 2. 用 `#[derive(App)]` 把一个 struct 变成一个命令；
> 3. 读懂 `--help` 与 `--schema` 两处输出；
> 4. 为什么 `main` 只有一行也能跑出四个入口。

---

## 1.1 建立 crate

```bash
cargo new filecount
cd filecount
```

`Cargo.toml`：

```toml
[package]
name = "filecount"
version = "0.1.0"
edition = "2021"

[dependencies]
lilyco = { version = "0.3.1", features = ["tui", "web"] }
serde_json = "1"
```

三个决定，每一个都有后果：

| 写法 | 后果 |
|---|---|
| 只依赖 `lilyco` | 应用不直接碰 `lilyco-core`；所有类型从 `lilyco::prelude::*` 来 |
| `features = ["tui", "web"]` | 才有终端表单和 `--gui`；**默认 `default = []` 只有 CLI + MCP** |
| `serde_json` | 业务函数返回 `serde_json::Value`，结果直接就是机器可消费的 |

> 本机设了 `CARGO_TARGET_DIR`，产物在 `~/.cache/lilyco-build/target`。
> 项目里看不到 `target/` 不代表没编译。

---

## 1.2 写下第一个 App

`src/main.rs`：

```rust
use lilyco::prelude::*;

/// 统计一个文本文件的规模
#[derive(App)]
#[app(name = "count", about = "统计文本文件的行数、字节数或词数", run = "run_count")]
struct Count {
    /// 要统计的文件路径
    file: String,
}

fn run_count(app: &Count, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let result = serde_json::json!({ "file": app.file });
    ctx.done(result.clone(), 0);
    Ok(result)
}

fn main() {
    lilyco::run::<Count>();
}
```

先编译：

```bash
cargo build
```

**你应该看到**：编译通过，没有任何错误。有 warning 也先放着，第 2 章会解释其中一种。

### 逐行解释

| 代码 | 含义 |
|---|---|
| `/// 统计一个文本文件的规模` | 文档注释自动成为命令的 `about`（`--help` 首行、页面卡片说明） |
| `#[derive(App)]` | 生成 `schema()` / `from_args()` / `run()` 的实现 |
| `#[app(name = "count", ...)]` | 命令名（MCP 工具名也取它）；不写就用 struct 名 |
| `run = "run_count"` | 业务入口函数名，签名固定 |
| `/// 要统计的文件路径` | 参数的 `about`，同时成为页面里的字段标签与占位文案 |
| `ctx.done(result, 0)` | 告诉前端「跑完了，这是结果」——事件流的收尾 |
| `lilyco::run::<Count>()` | 按环境自动选后端 |

业务函数签名是固定的，不要改：

```rust
fn run_xxx(app: &你的Struct, ctx: &Context) -> Result<serde_json::Value, AppError>
```

`&App` 给你参数，`&Context` 给你上报能力，返回值就是 agent 侧拿到的结果。

---

## 1.3 跑起来

先看帮助：

```bash
cargo run -- --help
```

实测输出：

```text
统计文本文件的行数、字节数或词数

Usage: ch01 [OPTIONS] --file <file>

Options:
      --file <file>            要统计的文件路径
      --schema                 打印 JSON Schema 并退出
      --openai-tool            打印 OpenAI tool 定义并退出
      --anthropic-tool         打印 Anthropic tool 定义并退出
      --openai-responses-tool  打印 OpenAI Responses API 工具定义并退出（扁平格式）
      --openai-strict-tool     打印 OpenAI strict mode 工具定义并退出（结构化输出）
      --gemini-tool            打印 Gemini functionDeclarations 并退出
      --json                   输出格式：单个 JSON
      --json-stream            输出格式：JSON 流
      --gui                    启动 Web GUI
  -h, --help                   Print help
```

> 上面的 `Usage: ch01` 是示例文件的二进制名；你的项目里会显示 `filecount`。

只写了一个字段，为什么多出这么多选项？因为**这些不是参数，是接口**：
`--schema`、`--json`、`--json-stream`、`--gui` 都是框架给每个命令配的端入口。
你写的是参数区那几行；接口区是白送的。

> `--mcp` 是例外：它是**前置路由参数**，在 clap 解析之前就被拦下来，所以 `--help` 里不列，
> 但确实可用。同理 `LILYCO_UI=mcp` 也能切过去。

再看 schema：

```bash
cargo run -- --schema
```

```json
{
  "name": "count",
  "about": "统计文本文件的行数、字节数或词数",
  "args": [
    {
      "name": "file",
      "about": "要统计的文件路径",
      "kind": {
        "type": "text"
      },
      "required": true,
      "default": null
    }
  ],
  "subcommands": [],
  "safety": "read_only"
}
```

这张 JSON 就是**契约面**：Web 表单按它渲染，MCP 的 `inputSchema` 按它生成，
agent 不读你的源码，只读它。

`required: true` 不是随便来的：`String` 字段没有默认值，所以必填。
第 2 章你会看到 `Option<T>`、`#[arg(default = ...)]` 如何改变这一项。

---

## 1.4 一个 main，四个入口

```text
                    ┌──────── schema ────────┐
                    │ name / about / args    │
                    └───┬─────┬─────┬─────┬──┘
                        │     │     │     │
                      CLI    TUI   Web   MCP
```

`lilyco::run::<Count>()` 的选择顺序：

1. 命令行里有 `--mcp` → MCP；
2. 有 `--gui` → Web（需开 `web` 特性）；
3. 环境变量 `LILYCO_UI`（`cli` / `tui` / `web` / `mcp`）→ 对应后端；
4. 否则自动：**标准输入是终端且 `TERM` 非空 → TUI；管道/重定向 → CLI**。

第 4 条很重要：这意味着脚本里跑它永远不会卡在表单上。TUI 万一启动失败也会回退 CLI，不会裸崩。

试试四种：

```bash
cargo run -- --file Cargo.toml            # 非交互环境 → CLI
cargo run -- --file Cargo.toml --json     # 单行 JSON
cargo run -- --file Cargo.toml --gui      # 浏览器页面
cargo run -- --file Cargo.toml --mcp      # MCP stdio 服务器（隐藏参数，会等输入）
```

---

## 1.5 现在你应该看到

| 命令 | 应该看到 |
|---|---|
| `cargo build` | 编译通过 |
| `cargo run -- --help` | 上面那段帮助，参数区只有 `--file` |
| `cargo run -- --schema` | `kind.type == "text"`、`required: true` |
| `cargo run -- --file Cargo.toml` | 终端里打出 `{"file":"Cargo.toml"}` 形态的结果 |
| `cargo run -- --file Cargo.toml --gui` | HTTP 200，页面可访问，一个输入框 + 一颗「▶ 运行」（是否自动拉起浏览器取决于桌面环境，本环境未验证） |

如果 `--gui` 报 `the argument '--gui' cannot be used with one or more of the other specified arguments`：
说明依赖没开 `web` 特性，回到 1.1 检查 `features`。

---

## 小结

- 一个 `#[derive(App)]` struct 就是一个命令；`main` 只有一行。
- 文档注释即 `about`；字段注释即参数说明。
- `--schema` 是契约面，Web 与 MCP 都从它派生。
- 后端按环境自动选；管道/脚本落到 CLI，不会卡住。
- 业务函数签名固定：`(&App, &Context) -> Result<Value, AppError>`。

---

## 练习

1. **改说明，看三处变化**：把命令的 `about` 改一句话，重新跑 `--help`、`--schema`、`--gui`。
   提示：三处改的是同一份来源，不该出现不一致。
2. **加一个参数**：给 `Count` 加 `label: String`，先不写默认值，看 `--schema` 里它的 `required`。
   提示：再加 `#[arg(default = "未命名")]`，对比前后。
3. **验证自动探测**：把输出重定向到文件（`cargo run -- --file Cargo.toml > /tmp/out.txt`），
   确认它走的是 CLI 而不是 TUI。提示：管道/重定向时 stdin 不是终端。

---

## 下一章

[第 2 章 · 参数就是界面](02-args-are-ui.md)：把 `String` 换成 `PathBuf`、枚举、数字、开关，
看四种端如何跟着变。