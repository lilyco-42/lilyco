# 术语表

| 术语 | 含义 |
|---|---|
| **资源（resource）** | 一个可 CRUD 的集合定义，位于 `core/resources/<name>.rs` |
| **集合（collection）** | 资源在 URL 中的名字，如 `/api/notes` |
| **端点（endpoint）** | `server/src/app.rs` 注册的强类型处理函数 |
| **驱动（driver）** | 存储实现，如 `rusqlite`、`sqlx`、`seaorm`、`mongodb`、`libsql` |
| **预设（preset）** | `api` 仅后端；`fullstack` 含前端与 Caddy |
| **入口（gateway）** | `direct` 直连、`caddy`、`pingap` |
| **版本依赖** | 生成项目通过 crates.io 依赖 `rapid-core` / `rapid-server`，不复制框架源码 |
| **工作台** | `web/` 前端，消费 `/openapi.json` 与资源 API |
| **契约** | `/openapi.json` 描述的请求/响应 schema |
| **游标** | 列表分页的 `after` / `next` 值，基于自增 ID |
| **结构化错误** | 顶层 `code` / `message` / `field` 的 JSON 错误体 |
| **Kiri** | Docker 兼容容器运行时，`--runtime kiri` 切换 |
| **Compose** | `ops/` 与根目录的容器编排文件 |
| **Pingap** | 按路径分流的反向代理 |
| **Caddy** | 全栈默认入口，静态文件 + API 反代 |
| **`Rapid.toml`** | 项目外置配置：端口、存储、分页、文本上限 |
| **workspace** | 生成项目的 Cargo 工作区，成员为 `app-core` 与 `app-server` |
| **`lilyco-no_std`** | 无分配 Agent 核心，`rapid-core` 的依赖 |
| **Claude / MCP** | 与本教程无关；Rapid 的入口是 HTTP，不是 MCP |