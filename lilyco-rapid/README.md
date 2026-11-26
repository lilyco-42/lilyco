# Lilyco Rapid

从一份 Rust 资源模型生成 CRUD 和 OpenAPI，通过独立的 React 工作台管理数据。CLI 把配置、数据库迁移、前后端和 Compose 配方一起生成到新项目。

## 快速开始

在 Lilyco 仓库根目录开始。

```sh
cargo install --locked --path lilyco-rapid/ops/cli
lilyco-rapid new notes-workspace
cd notes-workspace
lilyco-rapid dev
```

默认 API 为 `http://127.0.0.1:8421`，Web 为 `http://127.0.0.1:5173`。
在 `Rapid.toml` 调整 API 地址，或通过 `RAPID_WEB_PORT` 调整 Vite 端口。修改地址后重启开发进程。
本地全栈开发需要 Rust、Node.js 和 pnpm。仅需 API 时添加 `--preset api`，它只需要 Rust。

```sh
lilyco-rapid generate resource notes --field title:string --field priority:i32 --field archived:bool
```

开发进程会重编 API，刷新浏览器后，`/api/notes` 提供列表、新建、读取、替换、删除。工作台同步出现 Notes，API reference 可直接发送请求。使用 `core/resources/notes.rs` 补充校验逻辑，运行 `cargo fmt --all` 格式化。

```sh
docker compose up --build -d
```

全栈由 Caddy 提供静态 UI 并转发 API，默认本机端口为 `8080`。API preset 默认本机端口为 `8421`。两者都可以通过 `RAPID_PORT` 改宿主端口。数据库随 Compose 命名卷持久化。服务面向可信的本地开发者。对公网开放时，由入口提供身份认证、授权和 HTTPS。

### 使用宿主上的 Caddy

在生成的全栈项目根目录准备 API 和静态文件。

```sh
cargo build --release --locked -p rapid-server
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
```

分别启动 API 与 Caddy。下面的地址仅供本机访问。

```sh
./target/release/rapid-server
```

```sh
RAPID_WEB_BIND=127.0.0.1 RAPID_WEB_ROOT="$PWD/web/dist" RAPID_UPSTREAM=127.0.0.1:8421 \
  caddy run --config ops/Caddyfile --adapter caddyfile
```

## 接口

| 方法 | 路径 |
| --- | --- |
| GET | `/healthz` 和 `/readyz` |
| GET | `/openapi.json` |
| GET / POST | `/api/{collection}` |
| GET / PUT / DELETE | `/api/{collection}/{id}` |

列表返回 `items` 和 `next`。继续查询时用 `?after=<next>`。写请求须提供完整的强类型 JSON，未知字段和无效数据返回结构化错误。配置在 `Rapid.toml`。英文和简体中文词条在 `web/src/strings*.toml`，按浏览器语言选择。

## 工作区结构

- `core/` 复用 `lilyco-no_std` 校验。默认用 `core + alloc` 表达业务资源，可在无操作系统目标编译。Cargo 构建脚本注册 `resources/` 下的模块。
- `server/` 用 Axum、SQLite WAL、Schemars 和 Utoipa 提供 API、数据持久化与契约。HTTP 运行环境使用 `std`。 自定义 HTTP 路由可以在 `server/src/main.rs` 中用 Axum 的 `Router::merge` 接入。
- `web/` 基于 Lilyco 的 React、Vite、Tailwind 和 shadcn 按钮模板。工作台消费 OpenAPI 的标量字段，复杂模型仍可在 API reference 中用 JSON 操作。
- `ops/` 包含脚手架 CLI 和部署配方。生成项目内含 `lilyco-no_std` 源码快照，可独立构建。

参考了 FastAPI 的契约与交互文档、Spring Boot 的外置配置与健康检查、Rails 和 Loco 的资源生成工作流。设计与审查记录见 [docs/LILYCO-RAPID.md](../docs/LILYCO-RAPID.md)。
