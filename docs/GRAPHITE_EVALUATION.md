# Graphite 融合评估（lyco 预研，2026-09-19）

> 目标：让 [Graphite](https://github.com/GraphiteEditor/Graphite)（⭐27.3K，Apache-2.0/MIT 双许可，Rust 内核 + Web 前端）成为 lilyco 框架的画图引擎 —— **AI 画画，支持其全部操作**。
> 结论先行：**可行性高**。Graphite 内核是纯 Rust 消息总线，原生可驱动；融合以 git dep 锁 rev 引入，`lilyco-graphite` 工具族分四批落地。

## 1. 架构体检（实测 /d/Code/graphite-recon，depth-1）

- Cargo workspace：`editor/`（消息总线+工具）、`document/`（文档模型）、`node-graph/`（过程化节点系统）、`frontend/`（Svelte + wrapper）、`desktop/`、`proc-macros/`
- **操作面 = 消息总线**：`Dispatcher::new(resource_storage, working_copy_root)` + `handle_message<T: Into<Message>>()`（dispatcher.rs:92/128）——每个 UI 动作都是一条类型化消息
- 规模：`editor/src/messages/` 244 个 rs 文件、22 组消息枚举、**约 405 个消息变体**（DocumentMessage ~47、select_tool 15、text/freehand 9、brush 6…）——这就是"所有操作"的机器可清点清单
- 平台无关：内核原生编译（官方 CI 原生跑测试）；wasm 触点仅在 frontend/wrapper 少量文件
- 许可：Apache-2.0 OR MIT —— 与 lilyco 双许可兼容，可 vendor/git-dep

## 2. 融合方案（三层）

| 层 | 内容 | 状态 |
|---|---|---|
| **P0 头less 桥** | `lilyco-graphite` crate（git dep 锁 rev）：Dispatcher headless 驱动——新建文档/加形状/设填充/序列化，包成 lilyco App + 安全门 + 遥测 | 本轮 spike |
| **P1 全量操作映射** | 405 变体全量清点 → 工具族 backlog 分批实现（渲染导出类需验证 wgpu headless 后端） | 规划 |
| **P2 GUI 嵌入** | lilyco Web 控制台挂载 Graphite 前端 + MCP 桥：AI 操作经消息总线实时反映在编辑器 UI（总线天然双向） | 规划 |
| **P3 画画 agent** | mpkg 存画稿（T2 门控）、遥测报图层/笔画、bitnet 桥给创作建议 | 愿景 |

## 3. 安全分级草案（Token 2 Anything 语义）

- T0：新建/查询/序列化/导出 PNG-SVG
- T1：改图层属性、删除图层、合并（不可逆默认拒，Interactive 面放行）
- T2：批量脚本化操作（循环加 100 图层）
- 遥测：图层数/节点数/导出字节 → Agent 实时看见画布状态

## 4. 风险与对策

| 风险 | 对策 |
|---|---|
| editor crate 依赖 frontend 回调（FrontendMessage sink） | headless 时 sink 收集即可（官方 test_utils 先例）；spike 首验点 |
| node-graph 渲染可能依赖 wgpu/GPU | 先落文档/序列化类操作（无 GPU）；导出类 CI 标 `#[ignore]`，真机再验 |
| graphite workspace 内部 path 依赖 | git dep 引用 workspace 成员由 cargo 解析同仓依赖；失败则 vendor 三目录 |
| 仓库大（56MB）+ 依赖重（CI 时长） | git dep 锁 rev + shallow；CI 增量缓存 |
