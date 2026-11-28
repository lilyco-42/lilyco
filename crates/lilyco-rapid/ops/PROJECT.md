# Lilyco Rapid 项目

在项目目录执行 `lilyco-rapid dev`。API 地址见 `Rapid.toml`，有 `web/` 时
Vite 会同时启动，终端会显示本机地址。开发 UI 需要 Node.js 与 pnpm 12。

只开发 API 时执行 `lilyco-rapid dev --api-only`。Docker 为默认容器运行时。装有 Kiri 时可用 `lilyco-rapid dev --runtime kiri`，或设置 `RAPID_CONTAINER_RUNTIME=kiri`。整套容器部署可用 `kiri compose up --build -d`。

已有项目可用 `lilyco-rapid generate frontend --ui vue` 生成独立 `web-next/`，再用 `lilyco-rapid dev --preview-ui` 连接同一个 API 和数据库预览。

```sh
lilyco-rapid generate resource notes --field title:string --field archived:bool
cargo run --locked -p rapid-server
docker compose up --build -d
```

- `core/resources/` 下每个模型注册一个资源，生成列表、读取、新建、替换和删除。
- `/openapi.json` 提供 OpenAPI 3.1。健康检查是 `/healthz` 与 `/readyz`。
- 在 `server/src/app.rs` 用 `api.get_query`、`api.get_path`、`api.get` 或 `api.post_json` 注册自定义处理函数。请求和响应结构体派生 `schemars::JsonSchema`，即可出现在 API 文档。示例请求为 `/api/hello?name=Lilyco`。
- 资源代码改动会触发 API 重编，刷新页面后会读取新的 OpenAPI。
- `Rapid.toml` 定义地址、数据库与请求限制。托管 PostgreSQL 和 MySQL 连接信息通过 `RAPID_DATABASE_URL` 提供，或由 `dev` 从本地 Compose 数据库获得。
- Turso 项目通过 `RAPID_DATABASE_URL` 和 `RAPID_DATABASE_TOKEN` 连接已有数据库。
- MongoDB 项目通过 `RAPID_DATABASE_URL` 连接指定数据库的副本集。SurrealDB 项目需要带命名空间和数据库路径的 `RAPID_DATABASE_URL`，以及 `RAPID_DATABASE_TOKEN`。
- `lilyco-rapid data export /path/to/backup.json` 导出资源记录，`lilyco-rapid data import /path/to/backup.json` 导入已经配置好的空目标库，保留记录和 ID。迁移时暂停写入，保留源库，核验数据后再切流量。
- Compose 默认只绑定本机，并以命名卷保存数据。对公网开放时，在入口接入身份认证、授权和 HTTPS。
