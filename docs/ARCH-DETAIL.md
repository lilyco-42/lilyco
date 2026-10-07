## Architecture

高内聚低耦合：执行语义只存在于 core（`executor`），四个后端只做"渲染/传输"，
门面 `lilyco` 是唯一的组合根（依赖所有后端），用户只依赖 `lilyco`。

```
+----------------------------------------------------------+
|              Your Struct  #[derive(App)]                  |
+----------------------------------------------------------+
                  |
     lilyco (facade)：自动后端选择（显式参数 > LILYCO_UI > 探测）
                  |
   +--------------+----------------+----------------+-----+
   |  lilyco-cli |  lilyco-tui    |  lilyco-gui    | lilyco-mcp
   |   clap      |  ratatui 表单  |  axum + SSE    | stdio JSON-RPC
   |             |                |                | tools/list·call
   +--------------+----------------+----------------+-----+
                  |                  |
      +-----------v------------------v-----------------+
      |              lilyco-core                        |
      |  App trait · CommandSchema · Registry(别名/隐藏) |
      |  executor（唯一执行宿主：参数→执行→进度事件）     |
      |  Progress 协议 · Context · AppError              |
      +--------------------------------------------------+
```

### Design Principles

1. **Type-driven**: `bool` -> checkbox, `u8` -> number input, custom enum -> dropdown. No manual widget mapping.
2. **CLI-first**: CLI is the most structured interface. TUI and Web are derived from the same schema.
3. **Progress as first-class citizen**: Every interface understands `Progress::Tick` / `Log` / `Done`.
4. **One execution host**: `core::executor` 是唯一的"参数→执行→进度事件"实现，
   CLI / TUI / GUI / MCP 只渲染事件流，不再各自实现宿主循环（消灭了三份重复代码）。
5. **AI-native**: 导出 LLM function-calling schema + 标准 MCP 服务器（`--mcp`），Agent 直接调用。
6. **Facade 自动选端**: 借鉴 mininterface 的接口工厂 —— 显式参数 > `LILYCO_UI` > 自动探测，
   TUI 起不来回退 CLI。
7. **Registry 动态注册**: 借鉴 unilang —— 运行期注册命令（插件 / AI 动态注册 / REPL），
   声明式 JSON 加载（`Registry::register_from_json`）。

---

