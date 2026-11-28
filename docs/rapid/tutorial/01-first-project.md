# 第 1 章 · 第一个项目

> **本章你会学到**
> 1. 安装 `lilyco-rapid` 并生成一个独立项目；
> 2. 认识生成目录里每个位置负责什么；
> 3. 启动 `dev`，用 HTTP 完成一次 CRUD；
> 4. 读 `/openapi.json` 确认契约。

---

## 1.1 安装 CLI

从官方仓库安装：

```sh
cargo install --locked --git https://github.com/Lyco-team/lilyco lilyco-rapid
```

> 在 Lilyco 仓库内开发时，可用 `cargo install --locked --path crates/lilyco-rapid` 安装本地版本。

确认版本：

```sh
lilyco-rapid --version
```

应输出 `lilyco-rapid 0.1.4`（以仓库 `Cargo.toml` 为准）。

> CLI 的模板通过 `include_dir!` 编译期内嵌。修改 `crates/lilyco-rapid/templates/` 后，必须重新安装或 `cargo build -p lilyco-rapid`，否则生成的项目仍是旧模板。

---

## 1.2 生成项目

```sh
lilyco-rapid new notes --preset api
cd notes
```

`--preset api` 只生成后端；`--preset fullstack`（默认）还会生成 React 前端与 Caddy 配方。

生成的目录（`api` 预设，实测）：

```text
notes/
├── Cargo.toml          # 独立 workspace：core + server
├── Cargo.lock
├── Rapid.toml          # 端口、存储路径、资源分页
├── compose.yaml
├── .gitignore
├── .dockerignore
├── LICENSE
├── README.md
├── core/
│   ├── Cargo.toml      # rapid-core 的源码快照
│   ├── build.rs        # 注册 resources/ 下的模块
│   ├── resources/tasks.rs
│   └── src/lib.rs
├── server/
│   ├── Cargo.toml      # rapid-server 的源码快照
│   ├── migrations/     # SQLite/Postgres/MySQL/Surreal 建表
│   ├── src/
│   │   ├── main.rs
│   │   ├── app.rs      # 你的业务端点注册在这里
│   │   ├── routes.rs
│   │   ├── data.rs
│   │   ├── store.rs
│   │   ├── config.rs
│   │   ├── error.rs
│   │   ├── endpoint.rs
│   │   └── openapi.rs
│   └── tests/http.rs
└── ops/
    ├── Dockerfile.api
    └── lilyco-no_std/  # 核心源码快照
```

三个决定，每个都有后果：

| 位置 | 后果 |
|---|---|
| `core/` 是源码快照，不是路径依赖 | 项目可脱离本仓库独立构建、发布 |
| `server/Cargo.toml` 默认启用 SQLite | 不装 Docker 也能启动 |
| `ops/lilyco-no_std/` 被 `exclude` | 它参与编译但不是 workspace 成员，避免与主仓特性冲突 |

---

## 1.3 启动

```sh
lilyco-rapid dev
```

默认输出：

```text
API listening on http://127.0.0.1:8421
```

只启后端（跳过前端依赖安装，`api` 预设默认就是这个行为）：

```sh
lilyco-rapid dev --api-only
```

配置在 `Rapid.toml`：

```toml
[http]
address = "127.0.0.1:8421"
max_body_bytes = 16384
max_in_flight = 32
```

`max_body_bytes` 是硬上限，超过会返回结构化错误，不会被读入内存后再判断。

---

## 1.4 第一次 CRUD

模板自带 `tasks` 资源。健康检查：

```sh
curl -i http://127.0.0.1:8421/healthz
curl -i http://127.0.0.1:8421/readyz
```

> `readyz` 会比 `healthz` 多检查存储可用性；数据库没起来时它应失败。

新建：

```sh
curl -s -X POST http://127.0.0.1:8421/api/tasks \
  -H 'content-type: application/json' \
  -d '{"title":"买牛奶","done":false}'
```

读取列表：

```sh
curl -s http://127.0.0.1:8421/api/tasks
```

响应形如：

```json
{"items":[{"id":1,"title":"买牛奶","done":false}],"next":null}
```

更新与删除：

```sh
curl -s -X PUT http://127.0.0.1:8421/api/tasks/1 \
  -H 'content-type: application/json' \
  -d '{"title":"买牛奶","done":true}'

curl -i -X DELETE http://127.0.0.1:8421/api/tasks/1
```

`PUT` 是**完整替换**：字段缺失会按反序列化规则报错，不做部分更新。

---

## 1.5 看契约

```sh
curl -s http://127.0.0.1:8421/openapi.json | head
```

`paths` 由 Rust 类型与注册的端点共同生成。第 2 章会验证：改字段只改 Rust 类型，文档自动跟随。

---

## 本章验收

```sh
test -f notes/Cargo.toml
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8421/healthz   # 200
curl -s http://127.0.0.1:8421/openapi.json | grep -q '"/api/tasks"'     # 有 tasks 路径
```

四条都通过再进入 [第 2 章](02-typed-endpoints.md)。

> 下一章：把 `tasks` 换成一个你自己定义的强类型端点，并理解「类型即文档」。