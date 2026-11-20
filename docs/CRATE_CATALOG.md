# Lilyco 开源 Crate 选型手册（`https://lib.rs` 优先）

> **核心原则**：在 `lilyco` 任何新模块开工前，必须先查本手册与 [https://lib.rs](https://lib.rs)。如果成熟开源 crate 已覆盖 ≥ 80% 需求且许可证兼容（MIT / Apache-2.0），**严禁手写基础轮子**；必须自研时需在代码头部写明硬约束（如零 C 依赖、静态 musl 体积上限或离线安全边界）。

---

## 1. Web 服务与全栈框架

| 场景 | 推荐 Crate | `lib.rs` 链接 | 说明与约束 |
|---|---|---|---|
| HTTP 服务 / 路由 | **`axum` (0.8)** | https://lib.rs/crates/axum | Workspace 统一锁定 `0.8`（禁止新增 `0.7`）；配合 `tower` / `tower-http` |
| HTTP 中间件（CORS/压缩/静态目录） | **`tower-http`** | https://lib.rs/crates/tower-http | 不要手写 CORS/Gzip/静态文件头 |
| 编译期类型安全 HTML 模板 | **`askama`** / **`maud`** | https://lib.rs/crates/askama · https://lib.rs/crates/maud | 替代 Rust 字符串拼 HTML；自带编译期校验与 XSS 自动转义 |
| 全栈快速脚手架（类似 Rails） | **`loco-rs`** | https://lib.rs/crates/loco-rs | 整合 `axum + sea-orm + CLI + workers + auth`，适合独立业务站 |
| 轻量 HTTP 客户端 | **`reqwest` (0.12)**（异步） / **`ureq` (3)**（同步） | https://lib.rs/crates/reqwest · https://lib.rs/crates/ureq | 统一使用 `rustls-tls`（避免 OpenSSL 跨平台编译问题）；禁止再引 `ureq 2` |

---

## 2. 数据库与持久化（严禁自研多后端 SQL 抽象层）

| 场景 | 推荐 Crate | `lib.rs` 链接 | 适用场景与踩坑提示 |
|---|---|---|---|
| **多后端异步 SQL 工具包** | **`sqlx`** | https://lib.rs/crates/sqlx | 原生 `AnyPool` 支持 SQLite / Postgres / MySQL + `sqlx::migrate!()` + `SQLX_OFFLINE=true` CI 离线检查 |
| **多后端异步 ORM** | **`sea-orm` (2.0)** | https://lib.rs/crates/sea-orm | 基于 SQLx，`apps/lilyco-manager` 已采用；适合复杂 Entity 关系与表迁移 |
| **纯 Rust 嵌入式 SQLite** | **`turso`** | https://lib.rs/crates/turso | MIT 许可，**`default-features = false` 时 100% 纯 Rust（零 C/FFI）**，单文件、低内存，最适配 1C1G 华为云与 `x86_64-unknown-linux-musl` 静态编译 |
| **纯 Rust 极简 ACID KV** | **`redb`** | https://lib.rs/crates/redb | 零 SQL 开销、极速编译、强类型表，适合本地状态/缓存/小工具 |
| **可插拔多存储 SQL 引擎** | **`gluesql`** | https://lib.rs/crates/gluesql | 需把 SQL 跑在 Memory / Redb / Sled / JSON / Parquet 上时直接复用 |
| ⚠️ **慎用：SurrealDB** | `surrealdb` | https://lib.rs/crates/surrealdb | **注意 BSL 商业源码许可限制**；嵌入式编译极重且内存占用偏高（80–150 MB），不推荐用于轻量 musl 嵌入式部署 |

---

## 3. AI 原生协议、MCP 与 OpenAPI

| 场景 | 推荐 Crate | `lib.rs` 链接 | 说明 |
|---|---|---|---|
| **官方 Rust MCP SDK** | **`rmcp`** | https://lib.rs/crates/rmcp | 原生支持 stdio 与 Streamable HTTP 传输、`#[tool]` 宏与 `schemars`；新 MCP 服务优先使用，逐步替换手写 JSON-RPC |
| **JSON Schema 自动派生** | **`schemars`** | https://lib.rs/crates/schemars | 从 Rust struct 直接派生 JSON Schema 供 LLM Tool Calling / MCP 使用 |
| **OpenAPI 3.1 自动生成** | **`utoipa`** | https://lib.rs/crates/utoipa | 代码即文档（`#[derive(ToSchema)]`），替代手写 OpenAPI JSON 拼装 |

---

## 4. 网页解析、Cookie 与浏览器自动化

| 场景 | 推荐 Crate | `lib.rs` 链接 | 说明 |
|---|---|---|---|
| **HTML5 解析 + CSS3 选择器** | **`scraper`** | https://lib.rs/crates/scraper | 已在 `lilyco-web2cli/src/dom.rs` 替换手写 tokenizer 与选择器（基于 `html5ever + selectors`） |
| **RFC 6265 Cookie 解析/持久化** | **`cookie`** / **`cookie_store`** | https://lib.rs/crates/cookie · https://lib.rs/crates/cookie_store | 已在 `lilyco-web2cli/src/session.rs` 替换手写 `Set-Cookie` 拆分 |
| **全功能 Chrome CDP 控制** | **`chromiumoxide`** | https://lib.rs/crates/chromiumoxide | 异步高阶 CDP 控制；若追求极小同步二进制（如 `lilyco-web2cli`）可用 `tungstenite` 轻量直连并注明约束 |

---

## 5. 安全、加密、授权与图像处理

| 场景 | 推荐 Crate | `lib.rs` 链接 | 说明 |
|---|---|---|---|
| **企业离线授权与插件门控** | **`lilyco-license`**（内部套件） | 仓库内 `lilyco-license/` | 基于 [`ring`](https://lib.rs/crates/ring) HMAC-SHA256 恒定时间验签 + [`base64`](https://lib.rs/crates/base64)，供所有商业应用共享 |
| **可插拔多渠道支付套件** | **`lilyco-pay`**（内部插件，按 feature 门控） | 仓库内 `lilyco-pay/`；底层推荐 [`async-stripe`](https://lib.rs/crates/async-stripe) · [`alipay-rs`](https://lib.rs/crates/alipay-rs) | 独立于 `lilyco-core`，按需启用 `afdian` / `alipay` / `stripe` / `google-play` / `usdt`，密钥零硬编码 |
| **多语言 i18n（CLI/MCP/SSR）** | **`rust-i18n`** / **`fluent-templates`** | https://lib.rs/crates/rust-i18n · https://lib.rs/crates/fluent-templates | Web UI 由前端主导 i18n；后端仅对错误码/SEO/CLI 按需引入 |
| **图片裁剪 / 压缩 / 证件照** | **`image`** | https://lib.rs/crates/image | 务必 `default-features = false, features = ["jpeg", "png", "webp"]` 控制体积 |
| **错误类型定义** | **`thiserror` (2)** / **`anyhow`** | https://lib.rs/crates/thiserror · https://lib.rs/crates/anyhow | 库 crate 用 `thiserror`，二进制 app 可用 `anyhow` |

---

## 6. Workspace 统一依赖规则

1. 公共基础 crate（`axum` / `tokio` / `serde` / `serde_json` / `thiserror` / `ring` / `base64` / `scraper` / `cookie` / `url`）统一在根目录 [Cargo.toml](../Cargo.toml) 的 `[workspace.dependencies]` 锁定版本，子 crate 一律写 `xxx.workspace = true`。
2. **静态 musl 交叉编译红线**：面向华为云/Linux 生产部署的 crate 尽量选择纯 Rust 或关闭 C 分配器（如 `turso` 设 `default-features = false`、`reqwest` 用 `rustls-tls`），确保 `x86_64-unknown-linux-musl` 一键编出单文件静态二进制。
