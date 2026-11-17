# 第 3 章 · 让长任务看得见

> **本章你会学到**
> 1. `Progress` 事件流的四个动作：`Started` / `tick` / `log` / `done`；
> 2. 同一条事件在 CLI、JSON 流、Web、MCP 上分别变成什么；
> 3. 取消为什么要靠 `ctx.is_cancelled()`，而不是杀掉进程；
> 4. 为什么事件流必须以 `Done` 或 `Error` 收尾。

---

## 3.1 四种上报动作

| 动作 | 你写 | 它表示 |
|---|---|---|
| 开始 | `ctx.emit(Progress::Started { total, message })` | 我要开始跑了，总共几步（`None` = 不确定） |
| 推进 | `ctx.tick(current, total, message)` | 走到第几步；框架算百分比 |
| 日志 | `ctx.log(LogLevel::Info, message)` | 给人看的一行话（不是进度） |
| 收尾 | `ctx.done(result, duration_ms)` | 跑完了，这是结果 |

还有一个查询：`ctx.is_cancelled()`。它读的是取消信号，不是进程状态。

---

## 3.2 改代码

在 `run_count` 里加上报与取消检查（其余不变）：

```rust
use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

// … enum Mode 与 struct Count 同第 2 章 …

fn run_count(app: &Count, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started {
        total: Some(3),
        message: Some("读取文件".into()),
    });

    let data = std::fs::read(&app.file)?;
    ctx.tick(1, Some(3), format!("{} 字节", data.len()));

    if ctx.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    let text = String::from_utf8_lossy(&data);
    ctx.tick(2, Some(3), "统计中");

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
    });

    ctx.tick(3, Some(3), "完成");
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}
```

上一章那条 `quiet is never read` 警告现在消失了 —— 它进了日志分支。

---

## 3.3 看事件流

```bash
cargo run -- --file Cargo.toml --json-stream
```

实测输出（逐行 JSON，方便管道 `jq` / 脚本消费）：

```json
{"type":"started","total":3,"message":"读取文件"}
{"type":"tick","current":1,"total":3,"message":"182 字节","percent":0.33333334}
{"type":"tick","current":2,"total":3,"message":"统计中","percent":0.6666667}
{"type":"log","level":"info","message":"8 行 · 25 词"}
{"type":"tick","current":3,"total":3,"message":"完成","percent":1.0}
{"type":"done","result":{"file":"Cargo.toml","label":"","mode":"lines","tags":[],"value":8},"duration_ms":0}
```

三个观察：

1. `percent` 由框架按 `current / total` 算好，你不要自己算；
2. `done` 带着 `result`，这就是 `--json` 模式下会直接打出来的那个对象；
3. 每行都是完整 JSON，可以边跑边读，不需要等进程结束。

---

## 3.4 同一条事件，四个界面

| 事件 | CLI / 流 | Web | TUI | MCP |
|---|---|---|---|---|
| `started` | 一行事件 | 日志区「运行中…」 | 状态行 | progress 通知 |
| `tick` | 带 percent 的事件 | 8px 进度条 + 日志行 | 进度条 | 进度通知 |
| `log` | 带 level 的事件 | 终端里一行，warn/error 上色 | 日志面板 | 通知文本 |
| `telemetry` | 事件 | 终端 `key = value`（tele 色） | 状态区 | 进度消息 |
| `done` | 收尾事件 | 进度转绿、结果区展开 | 完成提示 | 工具返回值 |
| `error` | 收尾事件 | 进度转红 + 红字 | 错误提示 | 错误结果 |

所以业务代码不要判断「我现在在哪个端」。你只管发事件，渲染由各端负责。

---

## 3.5 取消：一条信号，不是一次 kill

```rust
if ctx.is_cancelled() {
    return Err(AppError::Cancelled);
}
```

取消按钮按下去，Web 端会 `POST /cancel/{sid}`，服务端设置信号；你的循环在下一个检查点退出。
**没人会杀你的进程**，所以：

- 检查点要放在能安全停下的地方（不要在一半写入文件时退出）；
- 资源清理照常做（`Drop` 会执行）；
- 取消返回 `AppError::Cancelled`，前端把进度条涂红并写一行「已取消」。

取消按钮只在服务端登记了会话句柄时才出现。走 `run` / `run_registry` 这条主路就有；
自己写 `RunnerFn` 的自定义执行路没有 —— 页面不会画那颗骗人的按钮。

---

## 3.6 收尾是不变量

事件流**必须**以 `Done` 或 `Error` 结束。原因很实际：

- Web 端在等收尾事件解除「忙碌」状态；没有收尾，按钮永远点不动；
- MCP 调用在等最终结果；没有收尾，agent 一直挂着；
- JSON 流消费者在等最后一行。

`core::executor` 会在你的函数返回后兜底合成收尾，但这不意味着可以省掉 `ctx.done`：

- `ctx.done` 带上的是**你算好的结果与耗时**，兜底合成给不出这个；
- 主动收尾能让前端更早开始渲染。

失败路径**不要**手动发 `Progress::Error` 再返回 `Err`，会重复；直接 `?` 或返回 `Err`，
执行层会转成 `error` 事件。

---

## 3.7 现在你应该看到

| 命令 | 应该看到 |
|---|---|
| `cargo build` | 通过，且 `quiet` 警告消失 |
| `cargo run -- --file Cargo.toml --json-stream` | 6 行左右事件，最后一行 `done` |
| `cargo run -- --file Cargo.toml --json` | 只有一个结果对象，没有事件 |
| `cargo run -- --file Cargo.toml --gui` | 点运行：进度条、终端日志（含「8 行 · 25 词」）、结果 JSON |
| 页面上点「■ 取消」 | 只有命令响应了取消检查才会停下；否则会提示已发送取消请求 |

---

## 小结

- 四个动作：`Started` → `tick` → `log` → `done`；不要自己算百分比。
- 一条事件，四端各自渲染；业务代码不需要知道端。
- 取消是信号，不是 kill；检查点要放在安全位置。
- 事件流恒以 `Done` / `Error` 收尾；`ctx.done` 要带真实结果与耗时。

---

## 练习

1. **加一步**：把 `total` 从 3 改成 4，在读文件和统计之间插一次 `ctx.tick`，看 `percent` 怎么变。
   提示：百分比是框架算的，你只改 `current/total`。
2. **换成不确定进度**：`Started { total: None, .. }`，观察 Web 进度条与 JSON 流里的 `percent` 字段。
   提示：不确定态的进度条是「铺满 + 压淡」，不是停在半路。
3. **制造一次取消**：在 `--gui` 里选一个大文件，运行后立刻点取消。
   提示：`ctx.is_cancelled()` 只在你的检查点生效；文件读得太快时可能来不及取消，这是正常的。

---

## 下一章

[第 4 章 · 一个二进制多个命令](04-multi-command.md)：用注册表把 `count` 和 `echo` 放进同一个二进制。