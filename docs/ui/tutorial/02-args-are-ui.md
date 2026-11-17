# 第 2 章 · 参数就是界面

> **本章你会学到**
> 1. 用类型而不是字符串表达参数，让 `--schema` 自己说清约束；
> 2. `PathBuf` + `must_exist` 如何在进入业务函数前拦住错误路径；
> 3. 枚举、数字区间、开关分别变成什么控件；
> 4. 为什么 `range` 里不能写下划线。

---

## 2.1 从字符串到类型

第 1 章的 `file: String` 能用，但它在 schema 里只能说「这是一段文本」。
换成 `PathBuf` 之后：

```rust
/// 要统计的文件路径
#[arg(must_exist = true)]
file: PathBuf,
```

schema 就从 `{"type":"text"}` 变成：

```json
{
  "name": "file",
  "about": "要统计的文件路径",
  "kind": { "type": "path", "must_exist": true },
  "required": true,
  "default": null
}
```

两种写法的差别不在「更方便」，而在**约束被放到哪一层**：

| 写法 | 校验发生在 | 报错形态 |
|---|---|---|
| `String` + 自己检查 | 你的函数里 | 一句你自己写的错，agent 读不出更多 |
| `PathBuf` + `must_exist` | 框架统一校验 | 标准参数错误，四端一致 |

选中端到端受益的写法。

---

## 2.2 完整代码

把 `src/main.rs` 换成：

```rust
use std::path::PathBuf;
use lilyco::prelude::*;

#[derive(Debug, ValueEnum)]
enum Mode {
    Lines,
    Bytes,
    Words,
}

/// 统计文本文件的行数、字节数或词数
#[derive(App)]
#[app(name = "count", about = "统计文本文件的行数、字节数或词数", run = "run_count")]
struct Count {
    /// 要统计的文件路径
    #[arg(must_exist = true)]
    file: PathBuf,

    /// 统计口径
    #[arg(default = "lines")]
    mode: Mode,

    /// 只输出总数，不打印明细
    #[arg(default = false)]
    quiet: bool,

    /// 最多统计多少行（0 = 不限）
    #[arg(default = 0, range = 0..=1000000)]
    limit: u32,

    /// 附加标签（可写多个，仅演示列表控件）
    tags: Option<Vec<String>>,

    /// 显示用的标签（留空则不写）
    #[arg(default = "")]
    label: String,
}

fn run_count(app: &Count, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let data = std::fs::read(&app.file)?;
    let text = String::from_utf8_lossy(&data);

    let words = text.split_whitespace().count() as u64;
    let lines = if app.limit > 0 {
        text.lines().take(app.limit as usize).count() as u64
    } else {
        text.lines().count() as u64
    };

    let (mode_name, value) = match &app.mode {
        Mode::Lines => ("lines", lines),
        Mode::Bytes => ("bytes", data.len() as u64),
        Mode::Words => ("words", words),
    };

    let result = serde_json::json!({
        "file": app.file.display().to_string(),
        "label": app.label,
        "mode": mode_name,
        "value": value,
        "tags": app.tags.clone().unwrap_or_default(),
    });
    ctx.done(result.clone(), 0);
    Ok(result)
}

fn main() {
    lilyco::run::<Count>();
}
```

编译后你会看到一条警告：

```text
warning: field `quiet` is never read
```

这是真的：`quiet` 现在还没被用到，第 3 章接日志时才用上。
先留着 —— 编译器在提醒你「这个字段目前是装饰」，不是错误。

---

## 2.3 枚举：值域写进契约

```rust
#[derive(Debug, ValueEnum)]
enum Mode { Lines, Bytes, Words }
```

`ValueEnum` 派生做了两件事：

1. 变体名转 `snake_case` 作为字符串值（`Lines` → `lines`，`UpperCase` → `upper_case`）；
2. schema 里给出 `values` 数组。

于是 `--help` 直接列出可选值：

```text
--mode <mode>            统计口径 [default: lines] [possible values: lines, bytes, words]
```

Web 端根据 `values` 画 `<select>`，TUI 端循环切换。**你没有写任何映射代码。**

`schema` 里对应：

```json
"kind": { "type": "enum", "values": ["lines", "bytes", "words"] }
```

---

## 2.4 数字：区间要两边都说

```rust
#[arg(default = 0, range = 0..=1000000)]
limit: u32,
```

```json
"kind": { "type": "number", "min": 0.0, "max": 1000000.0 }
```

三条规则：

1. **`range` 只支持 `lo..=hi` 形式**，两端都会写进 schema；
2. **不要写下划线**：`0..=1_000_000` 会让宏的 `parse::<f64>()` 失败，`max` **静默丢失**
   （实测变成 `"max": null`）。写成 `0..=1000000`。
3. 只给一端也可以（`min = 0` 或 `max = 100`）；页面的提示行会说「取值 >= 0」，
   读屏也能听到 —— 单边区间照样要念出来。

---

## 2.5 开关：一行就是一个语义

```rust
#[arg(default = false)]
quiet: bool,
```

schema 里是 `{"type":"flag"}`，CLI 里是 `--quiet`，Web 里是整行可点的复选框，TUI 里是空格切换。
开关没有「中间态」，所以它不需要 loading / empty / error 三态 —— 这也是设计上的减法。

---

## 2.6 列表与可选：两种形状

```rust
/// 附加标签（可写多个，仅演示列表控件）
tags: Option<Vec<String>>,
```

`Option<Vec<String>>` 同时表达两件事：这个参数可以不给，给了可以是多个。

| 层 | 表现 |
|---|---|
| CLI | `--tags a --tags b`（可重复） |
| schema | `kind: {"type":"list","item":{"type":"text"}}`，`required: false` |
| Web | 动态行 + 「＋ 添加一项」；删到最后一行会自动补一行 |
| MCP | `type: "array"`（items 为 string） |

`Option` 在这里是必要的：`Vec<String>` 本身会被判为必填，`Option` 把它变成「可不给」。
命令侧用 `unwrap_or_default()` 落到空数组。

还有两种没在本章展开的写法：

| 写法 | 语义 | 什么时候用 |
|---|---|---|
| `Option<T>` | 可省略 | 参数不是必填，但「没给」和「给了空串」要区分 |
| `Vec<T>` | 必填的列表 | 至少要给一项才算合法 |

`Vec<T>` 目前只有 `Vec<String>` 端到端能跑通，`Vec<u32>` / `Vec<Enum>` 会在宏取值分支编译不过，
详见 [附录 B](../appendix/B-errors.md)。

最后一个字段补上「文本」这一格：

```rust
/// 显示用的标签（留空则不写）
#[arg(default = "")]
label: String,
```

`String` + `default = ""` 同时演示两件事：文本控件从哪里来、**有默认值即非必填**。
到这里页面上六类控件就齐了：`flag`（quiet）、`text`（label）、`number`（limit）、
`enum`（mode）、`path`（file）、`list`（tags）。

---

## 2.7 跑起来

```bash
cargo run -- --file Cargo.toml --mode bytes
```

实测：

```text
{
  "file": "Cargo.toml",
  "label": "",
  "mode": "bytes",
  "tags": [],
  "value": 182
}
```

（`value` 随文件内容变化，别把 182 当成固定值。）

故意给一个不存在的路径：

```bash
cargo run -- --file /nope
```

```text
error: invalid value '/nope' for '--file <file>': path does not exist: /nope
```

注意：**你的业务函数根本没被调用**。校验发生在参数解析阶段，四端共用同一条规则。

再看 `--help` 里多出来的东西：

```text
--mode <mode>            统计口径 [default: lines] [possible values: lines, bytes, words]
--quiet                  只输出总数，不打印明细
--limit <limit>          最多统计多少行（0 = 不限） [default: 0]
--tags <tags>...         附加标签（可写多个，仅演示列表控件）
--label <label>          显示用的标签（留空则不写） [default: ""]
```

一条注解换三处一致：CLI 帮助、Web 表单、MCP 的 `inputSchema`。

---

## 2.8 现在你应该看到

| 命令 | 应该看到 |
|---|---|
| `cargo build` | 通过，可能有 `quiet` 未使用的警告 |
| `cargo run -- --help` | 四个参数，`--mode` 带可选值，`--limit` 带默认值 |
| `cargo run -- --schema` | `path`/`enum`/`flag`/`number` 四种 kind，number 有 min 与 max |
| `cargo run -- --file Cargo.toml --mode words` | `value` 是词数 |
| `cargo run -- --file /nope` | 参数错误，业务函数未执行 |
| `cargo run -- --file Cargo.toml --gui` | 一页六类控件：路径带拖拽区、口径是下拉、开关是复选框、数字框带区间提示、`tags` 是动态列表、`label` 是文本框 |

---

## 小结

- 类型决定控件：`PathBuf`→路径、`enum`→下拉、`bool`→开关、数字→数字框。
- `must_exist` 之类校验由框架统一执行，别在自己函数里重写。
- `ValueEnum` 的值是 `snake_case`，CLI、Web、MCP 三处一致。
- `range` 只写 `lo..=hi`，**不要下划线分隔**。
- 警告不等于错误；`quiet` 现在没用，第 3 章会接上。
- `Option<Vec<String>>` 同时表达「可不给」与「可多个」，Web 上是动态列表。
- 一页之内就能覆盖六类控件：flag / text / number / enum / path / list。

---

## 练习

1. **把 `mode` 的默认值改掉**：改成 `bytes`，重跑 `--help` 与 `--schema`，
   确认只改了一处、三端都变。
2. **加一个 `Option<String>` 参数**：`prefix: Option<String>`，看 schema 里 `required` 与 `default` 有什么不同。
   提示：`Option<T>` 未给时是 `None`，不是空串。
3. **故意写错区间**：把 `range` 改成 `0..=1_000_000`，跑 `--schema` 观察 `max` 变成什么。
   提示：这就是 [附录 B](../appendix/B-errors.md) 里那条「静默丢失」的现场。

---

## 下一章

[第 3 章 · 让长任务看得见](03-progress-and-cancel.md)：给命令加上进度、日志与取消。