# 教程：从零到可部署的 Lilyco Rapid 项目

这套教程仿照 `docs/ui/tutorial` 的章节式写法：一个贯穿示例 `notes`，每章都在上一章基础上增加能力，命令与输出均可在本机复现。

## 贯穿示例

`notes` 是一个笔记服务：

- 一个 `tasks` 资源（生成项目自带）作为最小 CRUD；
- 第 2 章加一个类型化问候端点；
- 第 3 章加 `notes` 资源与字段校验；
- 第 4 章切换存储；
- 第 5 章生成前端；
- 第 6 章部署到 Compose；
- 第 7 章写契约测试；
- 第 8 章排错。

## 环境

| 依赖 | 用途 |
|---|---|
| Rust（`rust-version` 1.94，随项目 `Cargo.toml`） | 后端与 CLI |
| Node.js + pnpm 12 | 仅全栈前端 |
| Docker 或 Kiri | 本地数据库容器（SQLite 不需要） |

只跟随后端章节可用 `--preset api`，不需要 Node.js。

## 章节

| 章节 | 你会得到 |
|---|---|
| [第 1 章](01-first-project.md) | 一个能跑的 API |
| [第 2 章](02-typed-endpoints.md) | 自定义强类型端点 |
| [第 3 章](03-resources.md) | 资源生成、校验、分页 |
| [第 4 章](04-database.md) | 六种数据库与迁移 |
| [第 5 章](05-frontend.md) | React/Vue/Svelte 工作台 |
| [第 6 章](06-deploy.md) | Caddy/Pingap/Compose/Kiri |
| [第 7 章](07-testing.md) | 契约测试与 CI |
| [第 8 章](08-troubleshooting.md) | 排错与升级 |

## 约定

- 命令提示符统一为 `$`；`notes/` 指生成项目根目录。
- 端口默认：API `8421`，Vite `5173`，Compose 全栈 `8080`。
- 所有配置优先写 `Rapid.toml`，环境变量只用于连接串等运行时注入。
- 每章末尾的「本章验收」必须实际执行后再进入下一章。

## 不想按顺序读

- 只想改接口：[第 2 章](02-typed-endpoints.md) + [API 参考](../reference/api.md)
- 只想部署：[第 6 章](06-deploy.md) + [CLI 参考](../reference/cli.md)
- 遇到报错：[第 8 章](08-troubleshooting.md) + [错误索引](../appendix/errors.md)