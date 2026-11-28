# Lilyco Rapid 文档

> 注册 Rust 函数 → 得到类型化 HTTP 端点、OpenAPI 与交互文档；一个 CLI 生成可独立部署的全栈项目。

## What

Lilyco Rapid 是主 workspace `crates/` 下的全栈框架，由四个 crate 组成：

| crate | 作用 |
|---|---|
| `crates/lilyco-rapid` | CLI：`new` / `generate` / `dev` / `data` |
| `crates/rapid-core` | 类型化端点与资源配置核心 |
| `crates/rapid-server` | Axum 运行时、OpenAPI、SQLite/SQLx/Mongo/SeaORM 存储 |
| `crates/lilyco-no_std` | 无分配 Agent 核心；作为 `rapid-core` 的依赖 |

`lilyco-rapid new` 生成的项目是**独立 workspace**，通过 crates.io 依赖 `rapid-core` / `rapid-server`，不复制框架源码。

## Why

- **契约先行**：Rust 类型同时驱动请求解析、校验和接口文档，不存在手写 OpenAPI 与代码漂移。
- **可独立部署**：生成的 `core/` / `server/` / `web/` / `ops/` 完整自包含。
- **数据库可选**：SQLite、PostgreSQL、MySQL、MongoDB、SurrealDB、Turso，驱动可切换。
- **前后端硬隔离**：后端只输出强类型 JSON，前端是独立 Vite 项目。
- **商业可运维**：Compose / Caddy / Pingap / Kiri 配方齐全，配置外置 `Rapid.toml`。

## How（5 分钟）

```sh
cargo install --locked lilyco-rapid

lilyco-rapid new notes --preset api
cd notes
lilyco-rapid dev
```

> 在 Lilyco 仓库内开发时，也可用 `cargo install --locked --path crates/lilyco-rapid` 安装本地版本。

另开终端：

```sh
curl http://127.0.0.1:8421/healthz
curl http://127.0.0.1:8421/api/tasks
```

默认端口 `8421`，配置在 `Rapid.toml` 的 `[http].address`。

## 学习路径

| 章节 | 内容 |
|---|---|
| [教程导航](tutorial/README.md) | 贯穿示例 `notes` 的学习路线 |
| [第 1 章 · 第一个项目](tutorial/01-first-project.md) | `new`、`dev`、第一个 CRUD |
| [第 2 章 · 类型化端点](tutorial/02-typed-endpoints.md) | Rust 类型驱动 API 与文档 |
| [第 3 章 · 资源与校验](tutorial/03-resources.md) | `generate resource`、字段类型、分页 |
| [第 4 章 · 选择数据库](tutorial/04-database.md) | 六种存储、驱动切换、导入导出 |
| [第 5 章 · 前端工作台](tutorial/05-frontend.md) | React/Vue/Svelte、预览、i18n |
| [第 6 章 · 部署](tutorial/06-deploy.md) | Caddy/Pingap、Compose、Kiri |
| [第 7 章 · 测试与 CI](tutorial/07-testing.md) | 契约测试、工作区验证 |
| [第 8 章 · 排错与升级](tutorial/08-troubleshooting.md) | 常见故障、升级路径 |

## 参考

- [CLI 参考](reference/cli.md)
- [Rapid.toml 配置](reference/rapid-toml.md)
- [HTTP API 与错误](reference/api.md)
- [Auth 集成（P0）](reference/auth.md)
- [错误索引](appendix/errors.md)
- [术语表](appendix/glossary.md)

## 校验文档链接

```sh
python3 docs/rapid/check-links.py
```

退出码 `0` 表示所有相对链接与锚点可达。

## 设计取舍

- 生成项目**不复制框架源码**：`core/` 与 `server/` 是业务骨架，依赖 crates.io 上的 `rapid-core` / `rapid-server`。
- CLI 的内嵌模板位于 `crates/lilyco-rapid/templates/`，修改模板后必须重新编译 CLI。
- 后端不渲染 UI；`web/` 是独立前端，通过 `/openapi.json` 获取契约。
- 展示性字段、分页大小等可调参数在 `Rapid.toml`，不要写死在业务代码里。