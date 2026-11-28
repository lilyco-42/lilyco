# Auth 集成（P0 原型）

> 目标：复用 axum 生态，不自研认证/授权/限流引擎。当前为 `rapid-server` 的可选 `auth` feature，尚未进入 `lilyco-rapid --auth` 模板。

## 能力

| 能力 | 实现 |
|---|---|
| 会话登录 | `axum-login 0.19` + `tower-sessions 0.15`（MemoryStore） |
| 密码哈希 | `argon2 0.5`（Argon2id 默认参数） |
| RBAC | `casbin 2.20`（策略外置字符串/文件） |
| 限流 | `tower_governor 0.8`（按 IP，429） |
| 用户属性 | `User { id, username, role, display_name }`（P0 内存种子） |

许可证：MIT（axum-login / tower-sessions / argon2 / tower_governor）与 Apache-2.0（casbin）。

## 启用

```toml
# crates/rapid-server/Cargo.toml
[features]
auth = ["dep:axum-login", "dep:tower-sessions", "dep:argon2", "dep:casbin", "dep:tower_governor", "dep:rand_core"]
```

```rust
let app = rapid_server::auth::router(rapid_server::auth::AuthOptions::default()).await?;
```

## 端点

| 方法 | 路径 | 行为 |
|---|---|---|
| POST | `/auth/login` | `{ "username", "password" }` → 200 + session cookie；错误 401 |
| POST | `/auth/logout` | 清空会话 → 204 |
| GET | `/auth/me` | 已登录 200；未登录 401 |
| GET | `/api/tasks` | 需登录 + casbin `GET` 授权；未登录 401，越权 403 |
| DELETE | `/api/tasks/{id}` | 需登录 + casbin `DELETE` 授权；viewer 403 |

默认种子用户（仅 P0 测试）：

| 用户 | 密码 | 角色 |
|---|---|---|
| `admin` | `admin123` | `admin`（GET + DELETE） |
| `viewer` | `viewer123` | `viewer`（仅 GET） |

> 生产必须在首次启动时替换种子、把用户与策略落到数据库，并持久化 session store；不要把默认密码带进部署。

## 授权策略

P0 用 casbin 内存策略：

```text
p, admin,  /api/tasks, GET
p, admin,  /api/tasks, DELETE
p, viewer, /api/tasks, GET
```

`AuthOptions` 允许调整限流：

```rust
AuthOptions {
    users: vec![...],
    requests_per_second: 1,
    burst: 3,
    ..Default::default()
}
```

## 验证

```sh
cargo test -p rapid-server --features auth --test auth
```

当前结果：**2 passed**，覆盖：

1. 未登录 `GET /api/tasks` → 401
2. `admin` 登录成功并拿到 session cookie
3. `admin` GET → 204、DELETE → 204
4. `/auth/me` 连续请求 → 200（会话续期）
5. `viewer` GET → 204、DELETE → **403**
6. 错误密码 → 401
7. 连续请求 → **429**（tower_governor）

## 尚未完成

- `lilyco-rapid new --auth` 模板化（生成项目依赖 `rapid-server 0.1.7` 的 `auth` feature；发布前需用户确认）。
- 用户/角色/权限持久化与 SQLite 迁移（P0 内存用户）。
- 登录页（AntD/shadcn）。
- 用户属性扩展（JSON metadata）与审计日志。

## 设计取舍

- 不把权限引擎写进 Rapid：认证、会话、哈希、RBAC、限流都用成熟 crate。
- P0 先验证兼容与行为，再决定模板形态；不先改生成器，避免把未验证的 API 固化进发布版本。
- AntD 只作为独立前端模板，不进 `lilyco-gui`（零依赖/CSP 不变）。