# Lilyco 资源图谱（lyco 预研收集）

> lyco 模糊/近义词搜索循环的沉淀：同类先例、生态情报、规范演进、战略结论。
> 维护规则：每次预研轮次后追加；每条资源必须有来源链接与"对 lilyco 的意义"。

## 1. Schema→UI / 表单生成（核心赛道先例）

| 项目 | ⭐ | 意义 |
|---|---|---|
| [alibaba/formily](https://github.com/alibaba/formily) | 12567 | JSON Schema 动态表单的规模化标杆 —— 证明 schema→UI 在工业界成立 |
| [lljj-x/vue-json-schema-form](https://github.com/lljj-x/vue-json-schema-form) | 2269 | 同概念 Vue 生态多渲染器实现 |
| [ncform/ncform](https://github.com/ncform/ncform) | 1188 | 配置驱动表单生成 |
| [ginkgobioworks/react-json-schema-form-builder](https://github.com/ginkgobioworks/react-json-schema-form-builder) | 373 | 可视化 schema 表单编辑器 |
| [objectstack-ai/objectui](https://github.com/objectstack-ai/objectui) | 27 | "schema-driven UI for the AI era" —— 概念最贴近的新生代 |
| [DavidLiedle/ratatui-form](https://github.com/DavidLiedle/ratatui-form) | 20 | Rust TUI 表单唯一先例（单表单，无多命令） |

**结论**：schema→form 在 JS 生态是万星级成熟范式（lilyco 方向正确）；**Rust TUI 表单生成器近乎无人区** —— lilyco 的 TUI 层是罕见占位。

## 2. MCP 生态与规范演进（情报密集区）

| 资源 | 值 | 意义 |
|---|---|---|
| [modelcontextprotocol/rust-sdk](https://github.com/modelcontextprotocol/rust-sdk) | 3882⭐ 活跃 | 官方 Rust SDK；lilyco-mcp 保持零依赖自研，SDK 仅作协议参考 |
| [ThinkInAIXYZ/go-mcp](https://github.com/ThinkInAIXYZ/go-mcp) | 676⭐ | Go SDK —— 跨语言实现对照 |
| [MCP 2026-07-28 changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog) | 规范 | **必读**：无状态化（去掉 initialize 握手）、`server/discover`、MRTR 取代服务端主动请求、`resultType` 字段 |
| [Claude Code sampling request #1785](https://github.com/anthropics/claude-code/issues/1785) | open | Claude Code 对 sampling 仍是 feature request |
| [Reddit: which clients support sampling](https://www.reddit.com/r/mcp/comments/1ltcbz5/which_mcp_clients_support_sampling/) | 讨论 | 社区确认 sampling 客户端落地稀薄 |

**关键情报（2026-07-28 规范）**：
- **Roots / Sampling / Logging 被列为 Deprecated**（[SEP-2577](https://github.com/modelcontextprotocol/modelcontextprotocol/pull/2577)），官方迁移建议：采样→直连 LLM API；roots→工具参数传路径
- **MRTR**（[SEP-2322](https://github.com/modelcontextprotocol/modelcontextprotocol/pull/2322)）取代 `sampling/createMessage` 等服务端主动请求：服务端返回 `InputRequiredResult`（`inputRequests`），客户端带 `inputResponses` 重试原请求
- 结果必须带 `resultType`（`complete` / `input_required`）；新协议下旧服务端结果视为 `complete`（向后兼容）
- `tools/list` 应确定性排序（利于客户端缓存与 LLM prompt cache）

**对 lilyco 的影响**：`HostBridge` 单点抽象**赌对了** —— 协议漂移（MRTR / 直连 LLM API）时只换桥实现，handler 与 core 零改动。2024-11-05 实现对现有客户端仍完全有效。

## 3. Agent-friendly CLI（品类正在成形）

| 项目 | ⭐ | 意义 |
|---|---|---|
| [kenn-io/kata](https://github.com/kenn-io/kata) | 415 | "AI-assisted work 的本地优先 issue 跟踪" —— agent 工作流工具 |
| [aeroxy/chrome-devtools-cli](https://github.com/aeroxy/chrome-devtools-cli) | 250 | "developer and agent friendly CLI" —— 命名与定位先例 |
| [jaredpalmer/mogcli](https://github.com/jaredpalmer/mogcli) | 212 | Agent-friendly CLI for M365 |

**结论**："agent-friendly CLI" 正在成为独立品类；lilyco 的 `--schema`/`--json-stream`/`--mcp` 三件套天然属于此列，README 可强调该定位。

## 4. 待验证 / 后续预研候选

- [ ] MCP `server/discover` + 无状态模式支持（2026-07-28 SEP-2575）—— 现有 `initialize` 握手保留为旧客户端兼容
- [ ] MRTR 形态的 `HostBridge` 适配器（`InputRequiredResult`/`inputResponses`）—— 待真实客户端落地再动
- [ ] `tools/list` 确定性排序（低成本，缓存友好，规范 SHOULD）
- [ ] objectui 的 "agents write compact JSON" 交互模式是否值得 lilyco-ultra-ui 借鉴
