# 附录 A · 注解速查

> 一页查完 `#[app]` / `#[arg]` / 类型映射 / 入口函数 / Context API。
> 所有条目都在本仓库实测可用；带「默认」的项不写就是这个行为。

---

## A.1 入口函数

| 写法 | 端 | 说明 |
|---|---|---|
| `lilyco::run::<T>()` | 四端自动 | 单命令，最常用 |
| `lilyco::run_with::<T>(backend)` | 指定 | `Backend::{Cli,Tui,Web,Mcp}` |
| `lilyco::run_registry("name", reg)` | 四端自动 | 多命令 |
| `lilyco::run_registry_with("name", reg, backend)` | 指定 | 多命令 + 指定端 |
| `lilyco::run_registry_with_policy(name, reg, backend, policy)` | 指定 | 自定义安全策略 |
| `lilyco::run_cli::<T>()` 等 | 单端 | 见 `lilyco/src/lib.rs` 的 `run_*` |
| `lilyco::run_cli_registry(name, reg)` | **仅 CLI** | 没有 `--mcp` |
| `lilyco::serve_mcp(reg)` | 仅 MCP | 直接把注册表挂成 stdio 服务器 |

业务函数签名固定：

```rust
fn run_xxx(app: &YourStruct, ctx: &Context) -> Result<serde_json::Value, AppError>
```

---

## A.2 `#[app(...)]`

| 键 | 类型 | 默认 | 作用 |
|---|---|---|---|
| `about` | 字符串 | 结构体的文档注释 | 命令说明（help 首行、卡片说明、MCP description） |
| `name` | 字符串 | struct 名 | 命令名 / MCP tool 名 / Web `?cmd=` 值 |
| `run` | 字符串 | 必填 | 业务函数名 |
| `safety` | 字符串 | `read_only` | 安全等级：`read_only`/`confirm`/`token`/`never_auto` |
| `crate` | 字符串 | `lilyco` | 宏展开引用的 crate 路径（一般不用改） |

`safety` 的别名（都可写）：

| 等级 | 可写 |
|---|---|
| T0 只读 | `t0` `read_only` `readonly` `read` `open` |
| T1 需确认 | `t1` `confirm` `confirmation` |
| T2 需令牌 | `t2` `token` `capability` |
| T3 禁止自动 | `t3` `never_auto` `neverauto` `never` `forbidden` |

MCP 面走 `DenyElevated`：T1+ 可被看见，但执行被拒。

---

## A.2b `#[view(...)]`

| 键 | 必填 | 作用 |
|---|---|---|
| `id` | 是 | 槽位标识；运行时 `ctx.view(id, …)` 按它填充 |
| `title` | 否 | 区域标题 |
| `kind` | 否 | 预期组件类型（如 `progress`），前端可先画骨架 |

可重复写多个。生成 `App::views()`，进入 `run_registry` 的 `--schema`。

---

## A.2c 视图组件（`Component`）

52 个变体，按 `kind`（snake_case）序列化。常用：

| kind | 关键字段 |
|---|---|
| `progress` | label / current / total（`total: None` = 不确定） |
| `badge` | text / tone |
| `alert` | title / description / tone |
| `table` | caption / columns / rows / sortable |
| `key_value` | items[{key,value}] |
| `log` | lines[{level,text}] |
| `result` | ok / title / value |
| `card` | title / description / children |
| `tabs` | items[{id,label,children}] / active |
| `dialog` | id / title / description / open / children |
| `chart` | chart_kind(bar/line) / labels / series |

`Tone` = `neutral` / `info` / `success` / `warn` / `danger`；
`ButtonVariant` = `primary` / `outline` / `ghost` / `danger`；
`SheetSide` = `top` / `right` / `bottom` / `left`。

全量与 shadcn 对照见 [视图组件协议](../view-protocol.md)。

---

## A.3 `#[arg(...)]`

| 键 | 写法 | 作用 |
|---|---|---|
| `about` | 字符串 | 参数说明；默认取字段的 `///` 文档注释 |
| `default` | 表达式 | 默认值；有默认值即非必填 |
| `range` | `lo..=hi` | 同时写 `min` 与 `max`；**不要写下划线** |
| `min` | 数字 | 只给下界 |
| `max` | 数字 | 只给上界 |
| `must_exist` | `true` / `false` | 路径参数必须存在（解析期校验） |

示例：

```rust
#[arg(must_exist = true)]
input: PathBuf,

#[arg(default = 75, range = 1..=100)]
quality: u8,

#[arg(default = "lines")]
mode: Mode,
```

---

## A.4 类型 → 控件

| Rust 类型 | schema kind | CLI | TUI | Web | MCP |
|---|---|---|---|---|---|
| `bool` | `flag` | `--quiet` | 空格 | 复选框（整行可点） | boolean |
| `String` | `text` | `--name <val>` | 文本框 | `<input type=text>` | string |
| `u8`…`f64` | `number`（可带 min/max） | `--limit <n>` | 数字输入 | `<input type=number>` + 提示 | number |
| `#[derive(ValueEnum)] enum` | `enum` + `values` | `--mode lines` | 循环切换 | `<select>` | string + enum |
| `PathBuf` | `path`（可 `must_exist`） | `--file <path>` | 路径输入 | 手填 + 拖拽区 + 本机 | string |
| `Option<T>` | 同内层，`required: false` | 可省略 | 非必填 | 非必填 | 不进 required |
| `Vec<String>` | `list{item:text}` | 可重复 `--tag a` | 多行 | 动态行 + 添加 | array |
| `Option<Vec<String>>` | `list`，`required: false` | 可省略、可重复 | 同上 | 同上 | array（可选） |

已知限制：`Vec<u32>` / `Vec<Enum>` 编译不过（见 [附录 B](B-errors.md) B.6）。

---

## A.5 命名规则

| 位置 | 规则 | 例子 |
|---|---|---|
| 字段名 → CLI 参数 | `snake_case` → `kebab-case` | `dry_run` → `--dry-run` |
| 枚举变体 → 值 | `PascalCase` → `snake_case` | `UpperCase` → `upper_case` |
| struct 名 → 命令名 | 默认原样，可用 `name` 覆盖 | `Count` → `Count` / `count` |

---

## A.6 `Context` API

| 方法 | 作用 |
|---|---|
| `ctx.tick(current, total, msg)` | 上报进度；`total: Option<u64>` |
| `ctx.log(level, msg)` | 一行日志；`LogLevel::{Info,Warn,Error,Debug}` |
| `ctx.emit(Progress)` | 发任意事件（`Started` / `Telemetry` …） |
| `ctx.done(value, duration_ms)` | 成功收尾（必须） |
| `ctx.is_cancelled()` | 是否收到取消信号 |
| `ctx.progress(...)` | 低层进度上报（一般用 `tick`） |

`Progress` 变体：

```rust
Started   { total: Option<u64>, message: Option<String> }
Tick      { current: u64, total: Option<u64>, message: Option<String>, percent: Option<f32> }
Log       { level: LogLevel, message: String }
Telemetry { key: String, value: serde_json::Value }
```

事件流恒以 `Done` / `Error` 收尾（executor 兜底合成），但成功路径应主动 `ctx.done`。

---

## A.7 特性与环境变量

| 项 | 值 | 效果 |
|---|---|---|
| facade feature `tui` | 开 | 终端表单（crossterm/ratatui） |
| facade feature `web` | 开 | `--gui`（axum/tokio） |
| facade feature `ultra` | 开 | `--ultra` playground |
| facade feature `full` | 开 | `tui + web + ultra` |
| 默认 | `[]` | 只有 CLI + MCP（可移植） |
| `LILYCO_UI` | `cli\|tui\|web\|mcp\|auto` | 强制后端 |
| `LILYCO_PORT` | 端口号 | Web 端口（默认 8080，只绑 127.0.0.1） |

---

## A.8 多命令 API

```rust
let mut reg = Registry::new();
reg.register(RegisteredCommand::from_app::<A>().alias("a"))?;
reg.register(RegisteredCommand::from_app::<B>())?;
lilyco::run_registry("toolname", reg);
```

| 能力 | 单命令 `run` | 多命令 `run_registry` |
|---|---|---|
| 顶层 `--schema` | 单命令 schema | 注册表清单 |
| 顶层 `--mcp` / `--gui` | ✅ | ✅ |
| `--json` / `--json-stream` | 顶层 | **子命令后** |
| `--anthropic-tool` 等 | 顶层 | **子命令后** |
| 别名 | — | `.alias("x")` |
| 可见性 | — | `hidden`（四端一致） |