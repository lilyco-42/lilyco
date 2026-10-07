## Limitations & Roadmap

### Current Limitations

- **`#[derive(App)]`** only works on named-field structs (no tuple structs or enums)
- **`#[app(run = "fn")]`** requires the function to be in scope. Without this attribute, `run()` panics with a helpful message directing you to add it.
- **Input validation** is shared: `CommandSchema::validate_args` (required / number range / enum / path must-exist) runs server-side in MCP (`INVALID_PARAMS`) and Web GUI (400), and pre-submit in TUI; CLI validates via clap at parse time
- **Subcommands** are supported in CLI only — TUI and Web renderers do not handle them yet
- **Ultra UI** is experimental — JSON spec format may change

### Roadmap

#### 已完成（v0.1 → v0.2.x）

- [x] ~~Real `run()` dispatch in Web GUI with progress streaming~~ — done via `GuiRenderer::serve_app::<A>()`
- [x] ~~`#[app(run = "fn")]` macro attribute~~ — wire business logic with zero boilerplate
- [x] ~~Integration tests that exercise all three interfaces end-to-end~~ — 12 tests in `lilyco-example`
- [x] ~~共享执行宿主~~ — `core::executor`，CLI/TUI/GUI/MCP 同一执行路径
- [x] ~~运行期命令注册表~~ — `core::Registry`（别名 / 隐藏 / JSON 声明式加载）
- [x] ~~MCP 输出面~~ — `lilyco-mcp`：`--mcp` 启动标准 stdio 服务器
- [x] ~~门面自动选端~~ — `lilyco::run::<A>()`（借鉴 mininterface 工厂）
- [x] ~~CI 双矩阵~~ — GitHub Actions ubuntu + windows：fmt / clippy / test / doc
- [x] ~~CLI 多命令：注册表 → clap 子命令~~ — `lilyco_cli::run_registry(name, registry)` / 门面 `lilyco::run_cli_registry`；隐藏命令可调用不显示，根级 `--schema` 打印注册表清单
- [x] ~~MCP 进度通知~~ — `tools/call` 携带 `_meta.progressToken` 时流式返回 `notifications/progress`（零依赖实现）
- [x] ~~MCP 采样 / roots~~ — 零依赖实现（不引 rust-sdk）：`HostBridge`（core）+ 双向 JSON-RPC 分流（serve）；handler 经 `ctx.sample()` 反向调用 Agent 的 LLM（`sampling/createMessage`），`ctx.roots()` 获取宿主工作根；客户端能力门控 + `srv-N` 字符串请求 id + 超时保护
- [x] ~~Subcommand navigation in TUI and Web GUI~~ — TUI 命令选择页（`TuiApp::new_multi` + `run_tui_registry`，mininterface subcommand picker 模式；隐藏命令不显示）；Web GUI `serve_registry` + `?cmd=` 切换下拉；`/run` 显式执行未知命令 400
- [x] ~~Input validation in TUI/Web widgets (range, required, enum)~~ — 共享校验 `CommandSchema::validate_args`：MCP `tools/call` 前置校验（`INVALID_PARAMS`）、Web GUI 服务端 400 + 浏览器 `required`/`min`/`max`、TUI 提交前拦截并红色提示；TUI 数字 ↑↓ 夹紧到 schema 范围
- [x] ~~TUI 执行异步化（进度渲染 + 取消）~~ — 非阻塞事件循环；取消键 `Ctrl-C`/`c`/`q`/`Esc`；executor 自动补发 `Done`/`Error` 终止事件（防卡死）
- [x] ~~Path auto-complete in TUI (Tab triggers directory listing)~~ — Path 字段有输入时 Tab 触发目录补全，重复 Tab 循环候选（目录带 `/` 后缀）；空值时 Tab 照常切换字段；兼容 `/` 与 `\` 分隔符
- [x] ~~`#[app(name = "...")]` macro attribute~~ — 多命令场景自定义 kebab-case 命令名（默认取结构体名）
- [ ] `#[app(subcommands)]` macro support
- [ ] TUI 执行异步化（进度渲染 + 取消）
- [x] ~~Publish to crates.io~~ — `lilyco-core/macros/cli/tui/gui 0.2.1`、`lilyco-mcp/lilyco 0.2.0`、`lilyco-ffmpeg 0.1.0`（旧 core 0.1.0/0.2.0 因缺 `executor`/`registry` 已 yank）
- [x] ~~Performance benchmarks for schema generation~~ — 零依赖基准 `cargo bench -p lilyco-example`（schema 生成 / JSON 导出 / validate_args / Registry 装配，release 实测 0.03–3 µs/op）

---

