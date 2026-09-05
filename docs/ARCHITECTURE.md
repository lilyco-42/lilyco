# Lilyco 架构说明（高内聚 · 低耦合）

本文记录一次整合的架构决策：借鉴 unilang / mininterface 的优秀设计，把
lilyco 从"CLI/TUI/Web 三个独立入口 + 三份重复宿主代码"收敛为
**core 单一执行宿主 + 后端插件 + facade 自动选端**。

## 1. 依赖方向（严格单向）

```
用户应用
   │ 只依赖
   ▼
lilyco (facade)          ← 组合根：唯一知道所有后端的 crate
   ├──► lilyco-cli       │
   ├──► lilyco-tui       │  每个后端只依赖 core，互不感知
   ├──► lilyco-gui       │
   ├──► lilyco-mcp       │
   └──► lilyco-core      │
        ▲                │
        └──（无任何反向依赖；core 零 UI 依赖，只有 serde/serde_json/thiserror）
```

规则：
1. **core 不依赖任何后端**（纯领域：schema / registry / executor / progress）。
2. **后端之间不互相依赖**（cli 不再管 `--gui`，gui 不再管 CLI）。
3. **facade 是唯一组合根**，用户只依赖它；加新后端 = 新 crate + facade 一行匹配。

## 2. 高内聚：每个 crate 只做一件事

| crate | 职责 | 不做什么 |
|-------|------|----------|
| `lilyco-core` | 领域模型 + 执行语义：`App` trait、`CommandSchema`（含 `validate_args` 唯一校验）、`Registry`、`executor`、`Progress` 协议 | 不做任何 I/O / UI |
| `lilyco-macros` | `#[derive(App)]` / `#[derive(ValueEnum)]` 代码生成 | 不含运行时逻辑 |
| `lilyco-cli` | schema → clap 渲染 + 内置标志 + 事件格式化输出 | 不再实现线程/宿主 |
| `lilyco-tui` | ratatui 表单状态机（`TuiApp`：单命令 + 多命令选择页） | 不持有执行逻辑 |
| `lilyco-gui` | axum 服务器 + SSE + 安全中间件 + HTML 模板 | 不关心任务怎么跑 |
| `lilyco-mcp` | MCP 协议（JSON-RPC stdio） | 不关心命令语义 |
| `lilyco` | 后端选择与装配（组合根） | 不含业务逻辑 |

## 3. 消灭的重复：统一执行宿主 `core::executor`

整合前，宿主循环（线程 + channel + 消费进度事件）在 `lilyco-cli::run`、
`lilyco-gui::serve_app`、`lilyco-example::main` 各写了一遍。

整合后只有一处：

```
executor::spawn(handler, args) -> Task { cancel, rx, handle }
   │  线程内执行 handler(&Context, &args)
   ▼
rx: Receiver<Progress>   ← 事件流协议保证以 Done / Error 结尾
```

- CLI `--json-stream`：逐行打印 `rx`（流式，不缓冲）
- GUI：`rx` 转发为 SSE
- TUI：`rx` 灌进 `TuiApp` 进度 API
- MCP：`executor::execute` 同步收集后返回结构化结果

`execute()` 额外保证：即使 handler 忘记上报终态事件，也会合成
`Done`/`Error`，事件流永远合法（协议不变量，有测试覆盖）。

### 3.1 消灭的重复 #2：统一校验 `CommandSchema::validate_args`

校验规则（required 缺省/空串、Number 类型与范围、Enum 可选值、Path must_exist、
List 递归）只存在于 core 一处（`schema.rs`），三端映射：

- CLI：clap 在解析层校验（同一套规则的最直白表达）
- MCP：`tools/call` 前置校验 → `INVALID_PARAMS`（Agent 直传参数没有 clap 兜底）
- Web GUI：`run_handler` 服务端校验 → 400 + 浏览器 `required`/`min`/`max`
- TUI：`FormField::validate` 提交前拦截（与 core 同语义的表单侧映射）

### 3.2 消灭的重复 #3：多命令导航

多命令语义（可见/别名/隐藏/默认命令）收敛在 `Registry`，四端各做一层薄渲染：
CLI 子命令树（`run_registry`）、TUI 命令选择页（`TuiApp::new_multi`，
借鉴 mininterface subcommand picker）、Web `?cmd=` 切换（`serve_registry`）、
MCP tools/list。**导航可回退、执行不可回退**（`?cmd` 未知 → 回退第一个可见；
`/run` 显式指定未知命令 → 400）。

## 4. 借鉴点与出处

### unilang（Wandalen，crates.io 14k+ 下载）→ core::Registry

| unilang 概念 | lilyco 落地 |
|--------------|-------------|
| 命令注册表（静态 build.rs + 动态 `register_with_routine`） | `Registry`：`register` + `RegisteredCommand::from_app::<A>()`（静态侧=derive，动态侧=运行期注册） |
| 别名解析到规范名（FR-REG-5） | `Registry::get` 自动解析 `aliases` |
| 隐藏命令 `hidden_from_list` | `RegisteredCommand::hidden`，`visible()` 过滤 |
| 声明式加载 `load_from_yaml_str/json_str`（FR-REG-3） | `Registry::register_from_json`（JSON，serde 直接反序列化） |
| 校验无静默失败（FR-REG-9） | `RegistryError`：空名 / 重名 / 无 handler 显式报错 |

### mininterface（CZ-NIC，288⭐）→ lilyco facade

| mininterface 概念 | lilyco 落地 |
|-------------------|-------------|
| `get_interface()` 工厂 + 回退链（gui→tui→text） | `lilyco::detect_backend`：`--mcp/--gui` > `LILYCO_UI` > 自动探测；TUI 起不来回退 CLI |
| 惰性加载后端（`__getattr__` 按需 import） | 后端各自独立 crate，facade 才聚合 |
| 每端独立 settings | `LILYCO_UI` / `LILYCO_PORT` 环境变量 + `Backend` 枚举显式指定 |
| 环境可注入可测试 | `Env { args, env, stdin_is_terminal }` 纯函数 `detect_backend(&Env)` |

### MCP（Model Context Protocol，官方 rust-sdk 3.8k⭐）→ lilyco-mcp

AI 工具调用正被 MCP 标准化。lilyco 原本只导出手写 `--anthropic-tool` /
`--openai-tool` 单次 schema；现在 `--mcp` 启动标准 stdio 服务器
（`initialize` / `ping` / `tools/list` / `tools/call`），任何 Agent 可直接调用。

- 手工实现最小子集（零额外依赖），协议逻辑是纯函数 `handle_line`，全单元可测
- 完整能力（进度通知 / 采样 / roots）留作 `lilyco-mcp-full`（基于官方 SDK），core 零改动

## 5. 关键决策记录（ADR 摘要）

| # | 决策 | 理由 |
|---|------|------|
| 1 | 执行宿主放 core，不放后端 | 4 个消费者共享同一语义；后端只做渲染 |
| 2 | 别名/隐藏放 `RegisteredCommand`，不改 `CommandSchema` | schema 保持向后兼容（serde roundtrip / 既有测试零改动） |
| 3 | facade 扫描 argv 决定后端，而不是往 clap 塞标志 | cli 不知道 mcp/gui 的存在，耦合不扩散 |
| 4 | MCP 手工最小实现，不引官方 SDK | 零依赖、纯函数可测；SDK 留作可选增强 |
| 5 | `from_args` 保持 `&HashMap`，Handler 内部转换 | 不动 trait 与宏（风险最小），转换成本可忽略 |

## 6. 已知限制（诚实清单）

- ~~CLI 多命令~~ 已实现：`lilyco_cli::run_registry` / `lilyco::run_cli_registry`（Registry → clap 子命令，别名/隐藏语义保留）
- ~~MCP 无进度通知~~ 已实现：`tools/call` 携带 `_meta.progressToken` 时流式返回 `notifications/progress`；采样 / roots 仍留待 rust-sdk 版
- ~~TUI/Web 无多命令导航~~ 已实现：TUI 命令选择页（`run_tui_registry`）+ Web `serve_registry`（`?cmd=` 切换）；schema 级嵌套 `subcommands` 仍仅 CLI 渲染
- OpenAI strict-mode JSON Schema 兼容性未验证（`to_json_schema` 含 `minimum`/`default` 等，
  部分 strict 实现会拒绝）——见预研报告中的 pydantic-ai#1561 教训
- TUI 数字键入值不即时夹紧（↑↓ 步进夹紧；键入越界由提交校验拦截）
- schema 生成无性能基准

> 代码导航：见 [CODEGRAPH.md](CODEGRAPH.md)（符号级图谱，带 `文件:行号`）。
