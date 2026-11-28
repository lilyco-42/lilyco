# 第 6 章 · 部署

> **本章你会学到**
> 1. 三种入口：直连、Caddy、Pingap；
> 2. Compose 全栈的端口与数据卷；
> 3. 用 Kiri 替换 Docker；
> 4. 在宿主上手工运行 API 与 Caddy。

---

## 6.1 入口选择

| `--gateway` | 适用 |
|---|---|
| `direct` | 本地开发或前面已有反向代理 |
| `caddy` | 全栈默认；静态文件 + 反代 API |
| `pingap` | 已有 Pingap 体系，按路径分流 |

API 预设默认 `direct`；全栈默认 `caddy`。

```sh
lilyco-rapid new notes-prod --gateway pingap --db postgres
```

生成的 `ops/` 包含对应 Caddyfile / Pingap TOML 与 Compose 片段。

---

## 6.2 Compose

全栈：

```sh
docker compose up --build -d
```

默认宿主端口：

| 服务 | 端口 | 调整 |
|---|---|---|
| 全栈入口 | `8080` | `RAPID_PORT` |
| API（API 预设） | `8421` | `RAPID_PORT` |
| 数据库 | 仅容器内网 | Compose 服务名 |

数据库使用命名卷持久化。删除容器不会删数据，除非显式 `docker compose down -v`。

对公网开放前，入口必须补上身份认证、授权与 HTTPS。Rapid 不内置公网鉴权；不要把没有鉴权的 API 直接暴露到公网。

---

## 6.3 Kiri

Kiri 是 Docker 兼容的容器运行时。装好 Kiri 后：

```sh
lilyco-rapid dev --runtime kiri
lilyco-rapid dev --api-only --runtime kiri
lilyco-rapid data export snapshot.json --runtime kiri
```

同一终端内可设默认值：

```sh
export RAPID_CONTAINER_RUNTIME=kiri
```

整套部署：

```sh
kiri compose up --build -d
```

> 两个运行时各自管理命名卷。切换引擎时先 `data export` 导出，再 `data import` 到目标引擎的空库。

---

## 6.4 宿主 Caddy

不使用容器时，在生成项目根目录构建产物：

```sh
cargo build --release --locked -p rapid-server --target-dir target
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
```

启动 API：

```sh
./target/release/rapid-server
```

启动 Caddy（仅本机监听示例）：

```sh
RAPID_WEB_BIND=127.0.0.1 \
RAPID_WEB_ROOT="$PWD/web/dist" \
RAPID_UPSTREAM=127.0.0.1:8421 \
  caddy run --config ops/Caddyfile --adapter caddyfile
```

变量含义：

| 变量 | 作用 |
|---|---|
| `RAPID_WEB_BIND` | Caddy 监听地址 |
| `RAPID_WEB_ROOT` | 前端静态目录 |
| `RAPID_UPSTREAM` | API 上游地址 |

---

## 6.5 发布前检查

```sh
cargo test -p rapid-server
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
docker compose config >/dev/null
```

对生成的独立项目，`--workspace` 指该项目自身的 workspace（`core` + `server`）。

---

## 本章验收

```sh
test -f notes-prod/compose.yaml
docker compose -f notes-prod/compose.yaml config >/dev/null   # 或 kiri compose config
```

再启动一次全栈，确认：

```sh
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/healthz
```

> 下一章：把这些验收写进自动化测试。