# 第 4 章 · 一个二进制多个命令

> **本章你会学到**
> 1. 用 `Registry` 把多个 `App` 装进一个二进制；
> 2. 一个二进制在 CLI / Web / MCP 上各自长什么样；
> 3. 别名与 `hidden` 的四端一致语义；
> 4. 多命令下端参数挂在哪一层（容易踩）。

---

## 4.1 什么时候需要多命令

单个 struct 适合「一件事」：一个命令一张表单，页面最干净。
当工具开始有「统计」和「回显」两种用途时，有两种做法：

| 做法 | 结果 | 适合 |
|---|---|---|
| 两个 crate / 两个二进制 | 各自独立，互不干扰 | 用途差别大、依赖不同 |
| 一个二进制 + 注册表 | 顶栏出现命令下拉，`--mcp` 一次列全 | 同一领域的多个动作 |

这一章做后者。

---

## 4.2 代码

保留第 3 章的 `Count`，再加一个更简单的 `Echo`，然后改 `main`：

```rust
use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

// … Mode 与 Count 同第 3 章 …

/// 原样回显输入的一段文本
#[derive(App)]
#[app(name = "echo", about = "原样回显输入的一段文本", run = "run_echo")]
struct Echo {
    /// 要回显的文本
    text: String,
}

fn run_echo(app: &Echo, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let r = serde_json::json!({ "text": app.text });
    ctx.done(r.clone(), 0);
    Ok(r)
}

fn main() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<Count>().alias("c"))
        .expect("注册 count");
    reg.register(RegisteredCommand::from_app::<Echo>())
        .expect("注册 echo");
    lilyco::run_registry("filecount", reg);
}
```

三件事：

1. `RegisteredCommand::from_app::<T>()` 把任意 `App` 变成一条可注册命令；
2. `alias("c")` 给命令行加一个短名字；
3. `run_registry` 而不是 `run` —— 这是四端分发多命令的入口。

---

## 4.3 四端形态

CLI 帮助（实测）：

```text
Usage: ch04 [OPTIONS] [COMMAND]

Commands:
  echo   原样回显输入的一段文本
  count  统计文本文件的行数、字节数或词数
  help   Print this message or the help of the given subcommand(s)

Options:
      --schema  打印注册表 JSON 清单（全部命令）并退出
  -h, --help    Print help
```

> 命令列出的顺序不固定（注册表是映射结构），不要依赖顺序。

Web：顶栏出现「切换命令」下拉，切换走 `?cmd=`：

```text
http://127.0.0.1:PORT/                → 注册表里的默认命令（顺序不固定，别假定是哪条）
http://127.0.0.1:PORT/?cmd=echo       → echo
http://127.0.0.1:PORT/?cmd=count      → count
```

MCP：`tools/list` 一次返回全部命令（`count` 与 `echo` 两个 tool）。

---

## 4.4 端参数挂在哪一层（最容易踩）

多命令下，**顶层只放注册表级选项**，子命令级的端参数要写在子命令后面：

| 参数 | 放哪 | 实测 |
|---|---|---|
| `--schema` | 顶层 | `ch04 --schema` → 注册表 JSON |
| `--mcp` | 顶层 | `ch04 --mcp` → 可握手 |
| `--gui` | 顶层 | `ch04 --gui` → 页面 |
| `--json` / `--json-stream` | 子命令后 | `ch04 c --file x --json` ✅；`ch04 --json` ❌ |
| `--anthropic-tool` 等工具定义 | 子命令后 | `ch04 count --anthropic-tool` ✅ |

顶层直接写 `--json` 的报错是：

```text
error: unexpected argument '--json' found
```

记住一句：**顶层管「有哪些命令」，子命令管「这条命令怎么跑」。**

---

## 4.5 单命令与多命令的两个差别

### 差别一：`run_cli_registry` 不是四端入口

```rust
lilyco::run_cli_registry("filecount", reg);   // 纯 CLI
```

它只跑 CLI。此时 `--mcp` 会报：

```text
error: unexpected argument '--mcp' found
```

要四端就用 `run_registry`。

### 差别二：只有一个可见命令时不画下拉

页面只渲染一个命令时，顶栏不出现「切换命令」——放一个只有一项的下拉，比留白更吵。
再多注册一个命令，下拉自动出现，不需要改页面代码。

---

## 4.6 别名与隐藏

- `alias("c")`：CLI 里 `filecount c ...` 等价于 `filecount count ...`。
- `hidden`：隐藏的命令不出现在 CLI help、TUI 选择页、Web 下拉、MCP `tools/list`，
  但**仍可被显式调用**（CLI 与 MCP 都能调）。适合内部/危险动作。

四端语义必须一致 —— 这是仓库的硬性规则之一（改语义要四端一起改）。

---

## 4.7 安全策略：按调用面注入

`run_registry` 会按后端注入不同的安全策略：

- MCP 面 → `DenyElevated`（agent 可以看见 T1+ 命令，但执行会被拦）；
- CLI / TUI / Web → `Interactive`（人来确认）。

所以同一个命令「agent 能不能自己跑」不取决于命令本身，而取决于它声明的 `safety`
与调用面。默认 `safety = "read_only"`，会写盘 / 外呼的命令要显式声明（见 [附录 A](../appendix/A-attributes.md)）。

---

## 4.8 现在你应该看到

| 命令 | 应该看到 |
|---|---|
| `cargo build` | 通过 |
| `cargo run -- --help` | `Commands:` 下有 `echo` 与 `count`，顺序不定 |
| `cargo run -- --schema` | 一个数组，两条命令，各自带完整 schema |
| `cargo run -- c --file Cargo.toml --json` | 结果 JSON（别名生效） |
| `cargo run -- echo --text hi --json` | `{"text":"hi"}` |
| `cargo run -- --gui` | 顶栏出现命令下拉，切换后整页重载 |
| 管道发 MCP 请求 | `tools/list` 返回两个 tool，名字是 `count` / `echo` |

---

## 小结

- `Registry` + `RegisteredCommand::from_app::<T>()` 组成一个二进制的命令表。
- 顶层管注册表，子命令管端参数（`--json` 要写在子命令后面）。
- `run_registry` 是四端入口；`run_cli_registry` 只跑 CLI。
- 别名、隐藏、安全策略在四端语义一致。
- 只有一个可见命令时，页面不画下拉。

---

## 练习

1. **加第三个命令**：写一个 `upper`（把 `--text` 转大写），注册进去，确认 Web 下拉与 `tools/list` 都多一项。
   提示：复制 `Echo`，改 name / about / 函数名。
2. **验证别名**：给 `echo` 加 `.alias("e")`，跑 `cargo run -- e --text hi --json`。
3. **制造那次报错**：先 `cargo run -- --json` 看报错，再 `cargo run -- count --file Cargo.toml --json` 对比。
   提示：顶层没有这条参数，这就是「端参数挂错层」。

---

## 下一章

[第 5 章 · 让页面像产品](05-make-it-product.md)：换肤、主题、窄容器，以及改样式前必须知道的闸门。