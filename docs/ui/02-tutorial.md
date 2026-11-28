# 速览版（一页抄完）

> 这一步是给「已经知道要做什么、只想复制骨架」的人。
> 想逐章学会、看每步的真实输出与练习，去 [章节教程](tutorial/README.md)。

---

## 1. 依赖

```toml
[dependencies]
lilyco = { version = "0.3.1", features = ["tui", "web"] }
serde_json = "1"
```

`features` 不能省：默认 `default = []` 只有 CLI + MCP，没有 `--gui`。

---

## 2. 一个完整的单命令

```rust
use std::path::PathBuf;
use lilyco::prelude::*;

#[derive(Debug, ValueEnum)]
enum Mode { Lines, Bytes, Words }

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

    /// 附加标签（可写多个）
    tags: Option<Vec<String>>,
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

    if !app.quiet {
        ctx.log(LogLevel::Info, format!("{lines} 行 · {words} 词"));
    }

    let (mode_name, value) = match &app.mode {
        Mode::Lines => ("lines", lines),
        Mode::Bytes => ("bytes", data.len() as u64),
        Mode::Words => ("words", words),
    };

    let result = serde_json::json!({
        "file": app.file.display().to_string(),
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

---

## 3. 四端命令

```bash
cargo run -- --file Cargo.toml              # CLI（管道/脚本自动走这里）
cargo run -- --file Cargo.toml --json       # 单行 JSON
cargo run -- --file Cargo.toml --json-stream# 事件流
cargo run -- --file Cargo.toml --schema     # 契约面
cargo run -- --file Cargo.toml --gui        # Web
cargo run -- --file Cargo.toml --mcp        # MCP stdio
```

---

## 4. 多命令

```rust
fn main() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<Count>().alias("c")).unwrap();
    reg.register(RegisteredCommand::from_app::<Echo>()).unwrap();
    lilyco::run_registry("filecount", reg);
}
```

多命令时端参数挂在子命令上：`filecount c --file x --json`，不是 `filecount --json`。

---

## 5. 注解速查

| 位置 | 常用 |
|---|---|
| `#[app(...)]` | `name` / `about` / `run` / `safety` |
| `#[arg(...)]` | `about` / `default` / `range = lo..=hi` / `min` / `max` / `must_exist` |
| 类型 | `bool`→开关、`String`→文本框、数字→数字框、`enum`→下拉、`PathBuf`→拖拽区、`Option<Vec<String>>`→列表 |

完整表见 [附录 A](appendix/A-attributes.md)。

---

## 6. 四个最常见的坑

| 现象 | 原因 | 修法 |
|---|---|---|
| `--gui` 报参数互斥 | 没开 `web` 特性 | `features = ["web"]` |
| `--mcp` 不存在 | 用了 `run_cli_registry` | 改 `run_registry` |
| 数字上限提示不见 | `range` 写了 `1_000` | 改 `1000` |
| 页面点运行没反应 | 少了 `ctx.done` | 每条路径都收尾 |

其余见 [附录 B · 常见报错](appendix/B-errors.md)。

---

## 7. 往下读

- 想按章节学：[教程目录](tutorial/README.md)
- 想改外观：[设计语言](03-design-language.md) / [令牌参考](04-tokens.md)
- 想查控件：[组件参考](components/README.md)
- 想上 React：[React 映射](05-react-shadcn-mapping.md)
- 想声明进度条/表格/弹窗：[视图组件协议](view-protocol.md)（教程第 8 章）