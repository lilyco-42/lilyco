# Rapid.toml 配置

配置随项目生成，位于项目根目录。业务参数外置；代码里不写死端口、分页、文本上限。

## `[project]`

```toml
[project]
name = "Lilyco Rapid"
```

| 字段 | 作用 |
|---|---|
| `name` | 项目显示名；生成模板时替换默认名 |

## `[http]`

```toml
[http]
address = "127.0.0.1:8421"
max_body_bytes = 16384
max_in_flight = 32
```

| 字段 | 默认 | 作用 |
|---|---|---|
| `address` | `127.0.0.1:8421` | API 监听地址 |
| `max_body_bytes` | `16384` | 请求体硬上限，超限返回 413 |
| `max_in_flight` | `32` | 并发请求上限 |

`address` 绑定 `0.0.0.0` 前先确认入口已有 HTTPS 与鉴权。

## `[storage]`

SQLite：

```toml
[storage]
path = "data/rapid.sqlite3"
busy_timeout_ms = 2500
```

| 字段 | 作用 |
|---|---|
| `path` | SQLite 文件路径 |
| `busy_timeout_ms` | 写锁等待时间 |

远程/服务型数据库使用连接配置：

```toml
[storage.connection]
driver = "seaorm"          # auto / rusqlite / sqlx / seaorm / mongodb / surrealdb / libsql
url_env = "RAPID_DATABASE_URL"
max_connections = 8
```

| 字段 | 说明 |
|---|---|
| `driver` | 存储驱动；`auto` 按数据库引擎推荐 |
| `url_env` | 读取连接串的环境变量名 |
| `max_connections` | 连接池上限 |

> 驱动是编译期特性，切换后需要重建 `server`（见 [第 4 章](../tutorial/04-database.md)）。

## `[resources]`

```toml
[resources]
max_text_bytes = 200
page_size = 40
```

| 字段 | 作用 |
|---|---|
| `max_text_bytes` | 字符串字段长度上限 |
| `page_size` | 列表默认页大小 |

## 覆盖顺序

1. 命令行显式参数；
2. 环境变量（连接串、端口等）；
3. `Rapid.toml`；
4. 内置默认值。

`--config <TOML>` 可指定另一份配置。指定后，连接等运行时变量仍由调用者注入。

## 校验

配置非法（端口、负数分页、缺失必填）应在启动阶段失败，并给出字段名与原因。不要等到第一次请求才暴露配置错误。

修改配置后重启 `dev`；`Rapid.toml` 不会被热加载。