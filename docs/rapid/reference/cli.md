# CLI 参考

安装：

```sh
cargo install --locked lilyco-rapid
```

全局：

```text
Usage: lilyco-rapid <COMMAND>

Commands:
  new       生成独立项目
  generate  生成资源或预览前端
  data      导出/导入记录
  dev       启动 API 与前端
```

---

## `new`

```text
lilyco-rapid new [OPTIONS] <NAME>
```

| 选项 | 默认 | 可选值 |
|---|---|---|
| `<NAME>` | 必填 | 项目目录名 |
| `--preset` | `fullstack` | `api`, `fullstack` |
| `--db` | `sqlite` | `sqlite`, `postgres`, `mysql`, `mongodb`, `surrealdb`, `turso` |
| `--db-driver` | `auto` | `auto`, `rusqlite`, `sqlx`, `seaorm`, `mongodb`, `surrealdb`, `libsql` |
| `--gateway` | 预设决定 | `direct`, `caddy`, `pingap` |
| `--ui` | `react` | `react`, `vue`, `svelte` |
| `--ui-kit` | `shadcn` | `shadcn`, `native` |
| `--dir` | `.` | 输出父目录 |

示例：

```sh
lilyco-rapid new notes --preset api
lilyco-rapid new shop --db postgres --db-driver seaorm --gateway pingap
lilyco-rapid new admin --ui vue --ui-kit native
```

行为：

- 已存在的同名目录会被拒绝，不覆盖；
- 名称不能逃逸 `--dir`（`../escape` 会失败）；
- 生成后打印项目绝对路径与启动命令。

---

## `generate resource`

```text
lilyco-rapid generate resource [OPTIONS] --field <NAME:TYPE> <COLLECTION>
```

| 选项 | 说明 |
|---|---|
| `<COLLECTION>` | 集合名（URL 路径与 Rust 模块名） |
| `--field` | `NAME:TYPE`，可重复；类型仅 `string`, `bool`, `i32` |
| `--dir` | 项目根目录，默认 `.` |

示例：

```sh
lilyco-rapid generate resource notes \
  --field title:string --field priority:i32 --field archived:bool
```

生成 `core/resources/<collection>.rs`，`core/build.rs` 自动注册。需要其它类型时手写 Rust 结构体。

---

## `generate frontend`

```text
lilyco-rapid generate frontend [OPTIONS]
```

| 选项 | 说明 |
|---|---|
| `--ui` | `react`, `vue`, `svelte` |
| `--ui-kit` | `shadcn`, `native` |
| `--dir` | 项目根目录，默认 `.` |

生成 `web-next/`，不覆盖现有 `web/`。配合 `dev --preview-ui` 预览。

---

## `dev`

```text
lilyco-rapid dev [OPTIONS]
```

| 选项 | 说明 |
|---|---|
| `--dir` | 项目根目录，默认 `.` |
| `--api-only` | 跳过前端与依赖安装 |
| `--preview-ui` | 用 `web-next/` 对接现有 API 与数据库 |
| `--runtime` | `docker`（默认）或 `kiri`；也读 `RAPID_CONTAINER_RUNTIME` |

行为：

- 自动启动本地数据库（PostgreSQL/MySQL 等需要 Compose）；
- 编译 `server` 并监听 `Rapid.toml` 的 `[http].address`；
- 有 `web/` 时并行安装依赖并启动 Vite；
- `--preview-ui` 只替换前端目录，不改数据库。

---

## `data`

```text
lilyco-rapid data [OPTIONS] <export|import> <PATH>
```

| 选项 | 说明 |
|---|---|
| `<PATH>` | 快照 JSON 路径 |
| `--dir` | 项目根目录，默认 `.` |
| `--config` | 指定另一份 TOML 配置 |
| `--runtime` | `docker` / `kiri` |

规则：

- 导出包含完整资源载荷；
- 导入只接受空目标库，保留 ID，拒绝覆盖；
- 导入在事务内完成，失败不留下半份数据。

---

## 环境变量

| 变量 | 作用 |
|---|---|
| `RAPID_DATABASE_URL` | 数据库连接串（默认注入的变量名） |
| `RAPID_DATABASE_TOKEN` | Turso / SurrealDB 令牌 |
| `RAPID_WEB_PORT` | Vite 开发端口 |
| `RAPID_PORT` | Compose 宿主端口 |
| `RAPID_WEB_BIND` | 宿主 Caddy 监听地址 |
| `RAPID_WEB_ROOT` | 宿主 Caddy 静态目录 |
| `RAPID_UPSTREAM` | 宿主 Caddy 的 API 上游 |
| `RAPID_CONTAINER_RUNTIME` | `docker` / `kiri` 默认运行时 |

连接串的变量名可在 `Rapid.toml` 的 `[storage].connection.url_env` 改，改后需同步注入对应环境变量。

---

## 退出码

| 码 | 含义 |
|---|---|
| `0` | 成功 |
| `1` | 参数、校验或运行时错误（错误信息在 stderr） |

CLI 不在生产环境调用 `.unwrap()`；失败会打印可读原因，不打印堆栈建议用户「重试」。需要调试时看 stderr 的完整上下文。