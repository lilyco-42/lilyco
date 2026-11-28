# 第 8 章 · 排错与升级

> **本章你会学到**
> 1. 按症状定位端口、依赖、容器与数据库问题；
> 2. 分辨「模板旧」与「代码旧」；
> 3. 安全升级 CLI 与已生成项目；
> 4. 哪些问题必须回到源码修，而不是绕过。

---

## 8.1 启动类问题

### 端口被占用

症状：`dev` 启动失败，提示地址已被使用。

```sh
ss -ltnp | grep -E '8421|5173|8080'
```

处理：改 `Rapid.toml` 的 `[http].address`，或用 `RAPID_WEB_PORT` / `RAPID_PORT` 调整前端与 Compose 端口。不要同时改多个地方，保持单一来源。

### pnpm 缺失或版本不符

仅全栈项目需要。先只跑后端确认业务：

```sh
lilyco-rapid dev --api-only
```

再装 Node.js 与 pnpm 12，重新 `lilyco-rapid dev`。`--api-only` 不安装前端依赖，不会碰 `web/`。

### Docker / Kiri 不可用

`readyz` 失败且 API 卡在数据库连接时：

```sh
docker info >/dev/null && echo docker-ok
kiri --version
```

SQLite 项目不需要容器；用 `--db sqlite` 可先绕开环境问题验证业务。切换容器运行时用 `--runtime kiri`，不要两套引擎同时管理同一个卷。

---

## 8.2 数据库问题

### `readyz` 失败但 `healthz` 正常

说明进程活着，存储不可用。检查：

```sh
echo "$RAPID_DATABASE_URL"
curl -s http://127.0.0.1:8421/readyz
```

常见原因：

| 现象 | 原因 | 修复 |
|---|---|---|
| 连接被拒绝 | 容器未启动或端口未映射 | `docker compose up -d` |
| 认证失败 | 用户名/密码/令牌不对 | 修正环境变量后重启 |
| 找不到数据库 | 目标库未创建 | 先建库，或让 Compose 初始化 |
| 迁移未执行 | 手动起了外部库但没跑 migrations | 用 `dev` 启动或手工应用 `server/migrations/` |

### 导入被拒绝

`data import` 只在空目标库上工作，且校验目标记录。重复导入或目标已有 ID 时会失败——这是保护，不是 bug。先导出备份，再在空库导入。

---

## 8.3 模板与生成物

### 改了模板但新项目没变

CLI 模板是编译期内嵌的：

```sh
cargo install --locked --path crates/lilyco-rapid --force
```

或开发时：

```sh
cargo build -p lilyco-rapid
```

然后重新 `new`。仅修改 `templates/` 文件不会影响已安装的二进制。

### `Cargo.lock` 与 `--locked` 失败

生成项目自带锁文件。依赖被手动改动后：

```sh
cd notes
cargo update -p <crate>
```

不要在 `--locked` 构建中删锁文件绕过；先确认依赖变更是有意的。

---

## 8.4 升级

### 升级 CLI

```sh
cargo install --locked --path crates/lilyco-rapid --force
lilyco-rapid --version
```

### 升级已生成项目

生成项目是源码快照，不会自动跟随本仓库。升级方式：

1. 用新 CLI 在临时目录生成同配置项目；
2. 对比 `core/`、`server/`、`ops/` 的差异；
3. 把业务改动（`core/resources/`、`server/src/app.rs`、`web/src/`）迁移过去；
4. 跑第 7 章测试。

不要直接覆盖整个项目目录，也不要删除业务资源后重新生成。

---

## 8.5 什么时候该修框架而不是绕过

以下问题优先在 `crates/` 修，而不是在生成项目里打补丁：

- 生成模板路径错误（先改 `templates/` + 重新编译 CLI）；
- OpenAPI 与类型不一致；
- 驱动切换后迁移失败；
- 分页/错误形状不符合契约。

修框架后必须跑：

```sh
cargo test -p rapid-core -p rapid-server -p lilyco-rapid
cargo test -p lilyco-no_std --features std
cargo fmt --all -- --check
cargo clippy -p rapid-core -p rapid-server -p lilyco-rapid -p lilyco-no_std --all-targets
```

---

## 本章验收

按顺序执行并记录输出：

```sh
lilyco-rapid --version
lilyco-rapid new smoke --preset api --dir /tmp/rapid-smoke
cd /tmp/rapid-smoke/smoke && cargo metadata --locked --no-deps >/dev/null && echo metadata-ok
```

三条都成功，说明 CLI、模板与生成项目工作区一致。

> 回到 [教程导航](README.md) 或查阅 [CLI 参考](../reference/cli.md) 与 [错误索引](../appendix/errors.md)。