# Lilyco Rapid

注册 Rust 函数即可得到类型化 HTTP 端点、OpenAPI 和交互文档。CLI 可生成 CRUD 资源，也可按项目选择数据库、驱动和代理，并启动独立的 Web 工作台。

## 快速开始

在 Lilyco 仓库根目录开始。

```sh
cargo install --locked --path crates/lilyco-rapid
lilyco-rapid new notes-workspace
cd notes-workspace
lilyco-rapid dev
```

### 本仓库位置

Rapid 由四个 crate 组成，都在主 workspace 的 `crates/` 下：

| 路径 | 职责 |
|---|---|
| `crates/lilyco-rapid` | 本 CLI（`new` / `generate` / `dev` / `data`）；`templates/` 内嵌生成模板 |
| `crates/rapid-core` | 类型化端点与资源配置核心；`schema` feature 产出 JSON Schema |
| `crates/rapid-server` | Axum 运行时、OpenAPI/交互文档、SQLite/SQLx/Mongo/SeaORM 存储 |
| `crates/lilyco-no_std` | 无分配 Agent 核心；生成项目会携带其源码快照 |

主 workspace 的验证命令：

```sh
cargo test -p rapid-core -p rapid-server -p lilyco-rapid
cargo test -p lilyco-no_std --features std
```

默认 API 为 `http://127.0.0.1:8421`，Web 为 `http://127.0.0.1:5173`。
在 `Rapid.toml` 调整 API 地址，或通过 `RAPID_WEB_PORT` 调整 Vite 端口。修改地址后重启开发进程。
本地全栈开发需要 Rust、Node.js 和 pnpm 12。`--preset api` 只生成后端，默认使用 SQLite，启动只需要 Rust。

已有全栈项目也可以用 `lilyco-rapid dev --api-only` 直接启动后端，跳过 Node.js 和依赖安装。全栈开发时，pnpm 优先复用本机缓存，并与数据库启动和 Rust 编译并行执行。

### 选择 Docker 或 Kiri

Docker 是默认容器运行时。使用 Kiri 时，在装好 Kiri 的机器上运行下面的命令。SQLite 文件与已有外部数据库连接可直接用于本地开发。[Kiri 安装文档](https://docs.runkiri.com/install/)

```sh
lilyco-rapid dev --runtime kiri
lilyco-rapid dev --api-only --runtime kiri
lilyco-rapid data export snapshot.json --runtime kiri
```

`RAPID_CONTAINER_RUNTIME=kiri` 可设为同一终端内的默认值。整套容器部署可直接运行 `kiri compose up --build -d`，使用同一份 Compose 配置。两个引擎分别管理命名卷。已有项目切换引擎时，可先用 `data export` 从源引擎导出，再用 `data import` 导入目标引擎的空库。[Kiri Compose](https://docs.runkiri.com/docker/compose/)

### 生成业务资源

```sh
lilyco-rapid generate resource notes --field title:string --field priority:i32 --field archived:bool
```

开发进程会重编 API，刷新浏览器后，`/api/notes` 提供列表、新建、读取、替换、删除。工作台同步出现 Notes，API reference 可直接发送请求。使用 `core/resources/notes.rs` 补充校验逻辑，运行 `cargo fmt --all` 格式化。

### 选择前端和组件

全栈模板可选 `--ui react`、`--ui vue`、`--ui svelte`。React 默认使用 shadcn 按钮模板，也可用 `--ui-kit native`。Vue 和 Svelte 使用原生组件。API 项目用 `--preset api`，可以在其他前端框架中直接消费 `/openapi.json`。

```sh
lilyco-rapid new vue-notes --ui vue --db postgres --db-driver seaorm --gateway pingap
lilyco-rapid dev --dir vue-notes --runtime kiri
```

已有项目可以先在 `web-next/` 生成并运行新前端。预览使用原 API、数据库和记录 ID，原 `web/` 会保留。确认后把自定义的页面和交互迁入新框架，再用它替换部署用的 `web/`。

```sh
lilyco-rapid generate frontend --ui svelte --dir vue-notes
lilyco-rapid dev --preview-ui --dir vue-notes
```

```sh
docker compose up --build -d
```

全栈默认使用 Caddy，可用 `--gateway pingap` 选择 Pingap。API preset 默认直接监听，也可选 Caddy 或 Pingap。Compose 的全栈和 API 宿主端口分别默认为 `8080` 和 `8421`，可通过 `RAPID_PORT` 调整。数据库随 Compose 命名卷持久化。对公网开放时，由入口提供身份认证、授权和 HTTPS。

### 使用宿主上的 Caddy

在生成的全栈项目根目录准备 API 和静态文件。

```sh
cargo build --release --locked -p rapid-server --target-dir target
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

### 自定义类型化端点

在 `server/src/app.rs` 定义输入、输出和处理函数，并在 `register` 注册。生成项目已经包含一个可调用的例子。

```rust
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Greeting { name: String }

#[derive(serde::Serialize, schemars::JsonSchema)]
struct Reply { message: String }

async fn greet(input: Greeting) -> Result<Reply, crate::ApiError> {
    Ok(Reply { message: format!("Hello, {}!", input.name) })
}

fn register(api: &mut crate::Api) -> Result<(), crate::StartupError> {
    api.get_query("/api/hello", greet)
}
```

`get` 接收无输入的查询，`get_query` 和 `get_path` 接收查询参数和路径参数，`post_json` 接收 JSON 请求体。Rust 类型同时驱动请求解析和接口文档。启动后在工作台的 API 文档输入参数并发送请求，或访问 `/api/hello?name=Lilyco`。

### 数据库选择与迁移

| `--db` | `--db-driver` | 本地启动 |
| --- | --- | --- |
| `sqlite` | `rusqlite`（默认）、`sqlx`、`seaorm` | SQLite 文件 |
| `postgres` | `sqlx`（默认）、`seaorm` | PostgreSQL 容器或外部连接 |
| `mysql` | `sqlx`（默认）、`seaorm` | MySQL 容器或外部连接 |
| `mongodb` | `mongodb`（默认） | MongoDB 副本集或分片集群 |
| `surrealdb` | `surrealdb`（默认） | SurrealDB 4 HTTP RPC |
| `turso` | `libsql`（默认） | Turso Cloud 或本机 libSQL 服务 |

```sh
lilyco-rapid new notes-pg --db postgres --db-driver seaorm --gateway pingap
lilyco-rapid dev --dir notes-pg
```

对生成的 PostgreSQL 或 MySQL 项目，`dev` 通过 Docker Compose 启动数据库并自动注入连接信息。若已设置 `RAPID_DATABASE_URL`，`dev` 使用该连接。直接运行 API 时从 `[storage].connection.url_env` 指定的环境变量读取连接 URL，默认变量名为 `RAPID_DATABASE_URL`。数据库密码放在环境变量或生成的 `.env`。

使用 `--db turso` 时，传入 `RAPID_DATABASE_URL` 和 `RAPID_DATABASE_TOKEN`。云端地址可用 `libsql://` 或 `https://`。本机 libSQL 开发服务可用 `http://127.0.0.1:<port>`。Compose 预设连接带令牌的云端服务。[Turso HTTP API](https://docs.turso.tech/sdk/http/reference)

MongoDB 用 `RAPID_DATABASE_URL` 指定副本集或分片集群和已有数据库。SurrealDB 用 `RAPID_DATABASE_URL=https://<host>/ns/<namespace>/db/<database>` 及 `RAPID_DATABASE_TOKEN` 连接已建立的命名空间和数据库。SurrealDB 令牌需要有权定义 Rapid 的表和索引。MongoDB 和 SurrealDB 预设都连接独立数据库服务。[MongoDB 事务](https://www.mongodb.com/docs/drivers/rust/current/crud/transactions/) · [SurrealDB HTTP API](https://surrealdb.com/docs/reference/protocols/http-protocol)

SQLite 上更换驱动时可沿用原文件。停止写入后，在 `Rapid.toml` 调整 `[storage].connection.driver`，并在 `server/Cargo.toml` 的默认特性中启用 `sqlx-server` 或 `seaorm`。同一 PostgreSQL 或 MySQL 实例的两个驱动也共用存储表。

跨引擎迁移时，保留源库并暂停写入。目标库启用对应 Cargo 特性，配置单独的连接并准备空的 Rapid 记录表。

```sh
lilyco-rapid data export /path/to/backup.json --dir old-app
lilyco-rapid data import /path/to/backup.json --dir new-app
```

可用 `--config <TOML>` 指定单独配置。指定配置时，由调用者注入其中需要的连接环境变量。导入在事务内校验目标记录，保留 ID 和已分配的最大 ID，并拒绝覆盖已有记录。快照只含 Rapid 资源记录，包含完整载荷，需按项目数据保护。使用了自定义表、业务索引或数据库特有能力时，需要为那些数据另备迁移。

## 生成项目结构

`lilyco-rapid new` 产物是独立 workspace，不包含本 CLI 源码：

- `core/` 复用 `lilyco-no_std` 校验。默认用 `core + alloc` 表达业务资源，可在无操作系统目标编译。Cargo 构建脚本注册 `resources/` 下的模块。
- `server/` 用 Axum、Schemars 和 Utoipa 提供类型化 API，数据库驱动通过存储接口接入。HTTP 和数据库驱动运行在 `std`。
- `web/` 使用 Vite、Tailwind 和可选前端框架。工作台消费 OpenAPI 的标量字段，复杂模型仍可在 API reference 中用 JSON 操作。
- `ops/` 存放 Compose、Caddy/Pingap 与数据库编排等部署配方；`ops/lilyco-no_std/` 是核心源码快照，保证项目可独立构建。

参考了 FastAPI 的契约与交互文档、Spring Boot 的外置配置与健康检查、Rails 和 Loco 的资源生成工作流。CLI 源码与模板在本仓库 `crates/lilyco-rapid`。
