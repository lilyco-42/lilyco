# 附录 C · 术语表

> 按字母/拼音混排，只收这套文档里反复出现的词。每条一句人话，不追求严谨定义。

---

## A–D

| 术语 | 一句话 |
|---|---|
| **App** | 一个 `#[derive(App)]` 的 struct，等于一条命令的 schema + 入口 |
| **ArgKind** | 参数的形状（flag/text/number/enum/path/list），决定四端各自渲染什么控件 |
| **alias** | 命令的短名，如 `count` → `c`；只影响 CLI 写法，不改变命令身份 |
| **container query** | 按「组件自己拿到的宽度」响应，而不是浏览器视口 |
| **Context** | 业务函数拿到的上报句柄：`tick` / `log` / `emit` / `done` / `is_cancelled` |
| **contract（契约面）** | `--schema` 输出的那份 JSON；Web、MCP 都从它派生 |
| **DenyElevated** | MCP 调用的安全策略：T1+ 命令可见但执行被拒 |

## E–L

| 术语 | 一句话 |
|---|---|
| **executor** | 唯一执行宿主，负责跑 handler、消费进度、合成收尾 |
| **flag 参数** | `bool` 字段，CLI 是无值开关，Web 是整行可点的复选框 |
| **hairline** | 装饰性细边（`--hairline`），不承担「认出控件」的 3:1 对比度要求 |
| **headless** | 不带 TUI/Web 的构建（`features = []`），只有 CLI + MCP |
| **hidden** | 命令可见性开关；四端一致：help/下拉/选择页/`tools/list` 都不出现 |
| **JSON stream** | `--json-stream` 输出的逐行事件流，适合管道消费 |
| **live region** | `role=status` / `role=log` + `aria-live`，让读屏自动播报变化 |
| **list 参数** | `Vec<String>` / `Option<Vec<String>>`，Web 上是可增删的动态行 |

## M–R

| 术语 | 一句话 |
|---|---|
| **MCP** | Model Context Protocol；stdio 上的 JSON-RPC，让 agent 发现并调用工具 |
| **Progress** | 事件枚举：`Started` / `Tick` / `Log` / `Telemetry` |
| **reduce motion** | 系统「减少动态效果」偏好；动画停掉后状态必须有静态替代 |
| **Registry** | 多命令的命令表；`run_registry` 用它做四端分发 |
| **RegisteredCommand** | `Registry::register` 的条目，由 `from_app::<T>()` 生成 |
| **runner / RunnerFn** | 自定义执行路；不登记取消句柄，所以页面不画取消按钮 |

## S–Z

| 术语 | 一句话 |
|---|---|
| **safety / T0–T3** | 命令的危险等级；决定 agent 面能不能自动执行 |
| **schema** | 命令的机器可读描述：名字、说明、参数、类型、必填、区间、安全等级 |
| **SSE** | Server-Sent Events；Web 端用它把进度推给页面 |
| **TDZ** | 暂时性死区；页面脚本里「装配早于常量初始化」会踩到 |
| **telemetry** | `Progress::Telemetry`；持续状态流（传感器/PLC），不是任务进度 |
| **ValueEnum** | 枚举的派生宏；变体名转 `snake_case` 作为可选值 |
| **令牌（token）** | 这里有两义：① CSS 变量（设计令牌）；② 本地会话令牌（安全） |
| **占位符** | 页面骨架里的 `__FIELDS__` 等，由服务端替换；名字是契约 |

---

## 容易混的两组

### 1. 「令牌」的两种意思

- **设计令牌**：`--accent`、`--s-4`、`--r-panel`，管颜色与尺寸；
- **会话令牌**：`<meta name="lilyco-token">` 与 `X-Lilyco-Token`，管本机请求鉴权。

文档里出现「令牌」时看上下文：讲 CSS 是前者，讲请求头是后者。

### 2. 「进度」与「日志」

- **进度**：`tick(current, total)`，有比例，驱动进度条；
- **日志**：`log(level, message)`，没比例，驱动终端里的行。

两者不是一条通道：一条被取消的日志不该让进度条动。`telemetry` 是第三种，
它描述「被控对象」，不是「任务走到哪」。