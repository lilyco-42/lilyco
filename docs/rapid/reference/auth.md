# Auth 集成（P0）

> 复用 axum 生态，不自研认证/授权/限流引擎。`lilyco-rapid --auth` 已能把认证路由与迁移生成进项目；业务 API 的 casbin 中间件仍是下一阶段。

## 生成

```sh
lilyco-rapid new student --preset api --auth
```

生成项目会得到：

| 位置 | 内容 |
|---|---|
| `Cargo.toml` | `rapid-server = { version = "0.1.8", features = ["auth"] }` |
| `server/src/lib.rs` | 合并 `/auth/*` 路由 |
| `server/migrations/auth.sql` | 用户/角色/权限表 |
| `server/src/main.rs` | `into_make_service_with_connect_info`（tower_governor 需要连接信息） |

## 能力

| 能力 | 实现 |
|---|---|
| 会话登录 | `axum-login 0.19` + `tower-sessions 0.15`（MemoryStore） |
| 密码哈希 | `argon2 0.5`（Argon2id 默认参数） |
| RBAC | `casbin 2.20`（策略内存/外置文件） |
| 限流 | `tower_governor 0.8`（按 IP，429） |
| 用户属性 | `User { id, username, role, display_name }`（P0 内存种子） |

许可证：MIT（axum-login / tower-sessions / argon2 / tower_governor）与 Apache-2.0（casbin）。

## 端点（生成项目）

| 方法 | 路径 | 行为 |
|---|---|---|
| POST | `/auth/login` | `{ "username", "password" }` → 200 + session cookie；错误 401 |
| POST | `/auth/logout` | 清空会话 → 204 |
| GET | `/auth/me` | 已登录 200；未登录 401 |

业务 `/api/*` 仍由 Rapid 资源 CRUD 提供；把 casbin 授权应用到业务 API 是 P1。

默认种子用户（仅 P0 测试）：

| 用户 | 密码 | 角色 |
|---|---|---|
| `admin` | `admin123` | `admin` |
| `viewer` | `viewer123` | `viewer` |

> 生产必须替换种子、把用户与策略落到数据库，并持久化 session store；不要把默认密码带进部署。

## 限流

生成模板默认 `20 req/s`、`burst 60`：

```rust
AuthOptions {
    requests_per_second: 20,
    burst: 60,
    ..Default::default()
}
```

`rapid-server` 的测试默认是 `1/3`，用于验证 429。Pingap 边缘限流与应用层限流可叠加。

## 验证

```sh
cargo test -p rapid-server --features auth --test auth
```

结果：**2 passed**，覆盖未登录 401、登录 200、admin 授权、viewer 越权 403、连续请求 429、会话续期。

生成项目端到端（rapid-server 0.1.8 + lilyco-rapid 0.1.10）：

```text
health=204
me_anon=401
login=200
me_auth=200
logout=204
```

## 尚未完成

- casbin 授权中间件应用到业务 `/api/*`（生成项目目前只挂认证路由）。
- 用户/角色/权限落库并执行 `auth.sql`（P0 是内存用户）。
- 登录页（AntD / shadcn）。
- 用户属性扩展（JSON metadata）与审计日志。

## 设计取舍

- 不把权限引擎写进 Rapid：认证、会话、哈希、RBAC、限流都用成熟 crate。
- 生成项目只做安全默认值（ConnectInfo、宽松但存在的限流）；授权策略由业务显式接入。
- AntD 只作为独立前端模板，不进 `lilyco-gui`（零依赖/CSP 不变）。