# Lilyco Rapid 项目

在项目目录执行 `lilyco-rapid dev`。API 地址见 `Rapid.toml`，有 `web/` 时
Vite 会同时启动，终端会显示本机地址。开发 UI 需要 Node.js 与 pnpm。

```sh
lilyco-rapid generate resource notes --field title:string --field archived:bool
cargo run --locked -p rapid-server
docker compose up --build -d
```

- `core/resources/` 下每个模型注册一个资源，生成列表、读取、新建、替换和删除。
- `/openapi.json` 提供 OpenAPI 3.1。健康检查是 `/healthz` 与 `/readyz`。
- 资源代码改动会触发 API 重编，刷新页面后会读取新的 OpenAPI。
- `Rapid.toml` 定义地址、数据库与请求限制。写入由 SQLite WAL 与事务保护。
- Compose 默认只绑定本机，数据保存在 `records` 卷。面向多人开放时，在入口接入身份认证、授权和 HTTPS。
