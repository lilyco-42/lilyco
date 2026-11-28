# 第 4 章 · 选择数据库

> **本章你会学到**
> 1. 六种数据库与驱动怎么选；
> 2. 连接串与环境变量的优先级；
> 3. 在 SQLite 驱动之间切换而不丢数据；
> 4. 用 `data export/import` 做跨引擎迁移。

---

## 4.1 选型表

| `--db` | 可用 `--db-driver` | 本地启动 |
|---|---|---|
| `sqlite` | `rusqlite`（默认）、`sqlx`、`seaorm` | 单个 SQLite 文件 |
| `postgres` | `sqlx`（默认）、`seaorm` | Compose 起 PostgreSQL |
| `mysql` | `sqlx`（默认）、`seaorm` | Compose 起 MySQL |
| `mongodb` | `mongodb` | 副本集或分片集群 |
| `surrealdb` | `surrealdb` | SurrealDB HTTP RPC |
| `turso` | `libsql` | Turso Cloud 或本机 libSQL |

默认是 SQLite：不需要 Docker，适合先跑通业务。

```sh
lilyco-rapid new notes-pg --db postgres --db-driver seaorm --gateway pingap
```

`--db-driver auto`（默认）会按数据库选择推荐驱动。

---

## 4.2 连接配置

生成的 `Rapid.toml` 里，SQLite 使用：

```toml
[storage]
path = "data/rapid.sqlite3"
busy_timeout_ms = 2500
```

PostgreSQL/MySQL 项目会在 `dev` 时自动启动 Compose 数据库并注入连接信息。若你已经有数据库：

```sh
export RAPID_DATABASE_URL='postgres://user:pass@host:5432/db'
lilyco-rapid dev
```

优先级：显式 `RAPID_DATABASE_URL` > Compose 生成值 > 项目默认。

远程服务还需要令牌：

```sh
export RAPID_DATABASE_URL='libsql://<host>'
export RAPID_DATABASE_TOKEN='<token>'
```

MongoDB 使用副本集/分片集群地址；SurrealDB 使用：

```sh
export RAPID_DATABASE_URL='https://<host>/ns/<namespace>/db/<database>'
export RAPID_DATABASE_TOKEN='<token>'
```

> 密码和令牌放环境变量或生成项目里被忽略的 `.env`，不要写进 `Rapid.toml` 提交。

---

## 4.3 驱动切换

SQLite 三个驱动共用同一个文件。停止写入后，在 `server/Cargo.toml` 的默认特性里切换：

```toml
[features]
default = ["sqlx-server"]   # 或 "seaorm"，回退则删除该行
```

然后重建：

```sh
cargo build -p rapid-server
```

`Rapid.toml` 的 `[storage].connection.driver` 同步修改。SQLite 文件不需要转换，表结构一致。

> 驱动切换是编译期特性，不是运行时开关。生产环境切换前先备份数据库文件。

---

## 4.4 导出与导入

快照只包含 Rapid 资源记录，包含完整载荷：

```sh
# 从旧项目导出
lilyco-rapid data export /path/to/backup.json --dir old-app

# 导入到空库
lilyco-rapid data import /path/to/backup.json --dir new-app
```

导入在事务内校验：

- 保留原 ID 与已分配的最大 ID；
- 目标已有记录时拒绝覆盖；
- 格式损坏直接失败，不写入半份数据。

跨容器引擎（Docker ↔ Kiri）可各导一次：

```sh
RAPID_CONTAINER_RUNTIME=docker lilyco-rapid data export snapshot.json
RAPID_CONTAINER_RUNTIME=kiri   lilyco-rapid data import snapshot.json
```

> 快照含业务数据，需按项目的数据保护要求保管；自定义表、业务索引或数据库特有能力要另备迁移。

---

## 4.5 数据库自定义

需要自定义表或索引时，写在 `server/migrations/`（SQLite/Postgres/MySQL/Surreal 各有入口文件），并在 `server/` 的存储层注册。不要在 `core/` 里直接访问数据库——`core` 必须保持可在无操作系统目标编译。

---

## 本章验收

```sh
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8421/readyz   # 200
lilyco-rapid data export /tmp/notes-snapshot.json
lilyco-rapid data import /tmp/notes-snapshot.json --dir notes-copy 2>&1 | head
```

第二条必须成功生成快照；第三条在空目标库（先 `new notes-copy`）应成功，重复导入应拒绝。

> 下一章：为这个项目加上 React / Vue / Svelte 工作台。