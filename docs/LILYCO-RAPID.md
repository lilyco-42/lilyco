# Lilyco Rapid 设计与审查

## Goal

用 Lilyco 工具生成 API 或全栈项目。手写 Rust 函数与生成的资源都提供同源交互文档。数据驱动和部署网关可独立选择，可移植领域代码复用 `lilyco-no_std`。

## Design & Why

| 边界 | 实现 |
| --- | --- |
| 可移植核心 | `lilyco-no_std` 以 `heapless` 承载定容 agent 数据。Rapid 资源层用 `alloc` 字符串与关闭默认特性的 Serde，复用 Lilyco 的 UTF-8 字节校验。 |
| HTTP | Axum 提取强类型 DTO。生成的资源和手写端点同时注册路由和 Utoipa 文档。Schemars 从同一份 Serde 模型生成字段契约。 |
| 数据 | SQLite 默认使用 rusqlite 和 WAL。SQLite、PostgreSQL 和 MySQL 可选 SQLx 或 SeaORM。按 collection 隔离记录并用 ID 键集分页。跨引擎快照在空目标库提交前核对内容，保留 ID 和分配水位。 |
| UI | 独立 React/Vite 构建，沿用 Lilyco starter 的 Geist、Tailwind 与 shadcn 按钮。工作台只通过同源 API 操作真实数据。 |
| 分发 | CLI 内嵌模板与核心快照。生成后可独立运行。全栈 Compose 可选择 Caddy 或 Pingap 提供静态文件和同源代理。 |

从旧版本迁移，请把 Cargo 包或本地依赖名从 `tiny_lilyco` 改为 `lilyco-no_std`，Rust 导入改为 `lilyco_no_std`。命令行二进制仍叫 `tiny-agent`，UDA 桥仍用自己的包名。类型执行入口 `dispatch_command` 接收已解析的参数和进度接收函数。

Lilyco Rapid 的标量资源默认以 JSON 作为 SQLite 行载荷。适合快速创建内部工具和 API。关联查询、从旧字段迁移和按业务字段建索引由项目扩展。带多租户或用户权限的服务需要应用层的数据授权和认证。

## Rust no_std 生态

- 使用 GitHub CLI 检索了 Rust Embedded 的精选清单、`heapless`、`postcard`、`serde-json-core` 和 `cap-std`。保留具备固定容量的数据结构，Rapid 的 JSON 用现有 Serde 栈。当前流程使用 HTTP JSON，因此未引入独立 MCU 编解码协议。[Awesome Embedded Rust](https://github.com/rust-embedded/awesome-embedded-rust#no-std-crates) · [Serde no-std](https://serde.rs/no-std.html)
- 文件边界选用 `cap-std`、`cap-fs-ext` 与 `cap-tempfile`，许可证为 MIT/Apache-2.0 体系，仅在 Linux、Windows 等宿主上的 `std` 特性启用。默认核心移除了未被代码使用的直接 Serde 与 WTX 依赖。[cap-std](https://github.com/sunfishcode/cap-std)
- ego-lite 尝试打开 lib.rs 的精选分类和 crate 页面时遇到渲染超时，直接访问收到 Cloudflare 挑战。选型细节通过上游仓库和 Cargo 包元数据复核。

## 稳定性、安全与性能

- RustSec 扫描后将 `heapless` 升级到 0.9.4，将 `rusqlite` 升级到 0.41.1，其内置 SQLite 包装层为 `libsqlite3-sys` 0.39.1。复扫核心、项目和脚手架的锁文件通过。[SQLite 公告](https://rustsec.org/advisories/RUSTSEC-2026-0459.html) · [heapless 公告](https://rustsec.org/advisories/RUSTSEC-2026-0582.html)

- 文件工具通过目录句柄验证路径，读取大小有上限，编辑写入经过同目录临时文件、同步与原子替换。目录后缀搜索无需打开文件。文本预览在取满所需行数后停止扫描。
- CLI 的文件修改先记录可恢复副本。未知工具、保存备份失败和恢复失败都进入明确的失败状态。恢复时只有查到创建标记才会删除文件。通用宿主子进程属于显式集成能力，内置 CLI 工具表保留有文件恢复能力的编辑。
- 旧 HTTP/桌面宿主提供只读能力。没有隔离的宿主命令仍使用调用进程的 OS 权限。可选 WASM 工具只用于可信插件，宿主禁用 WASI 与网络，并限制运行时间和内存。
- 核心与宿主分开验证。无 std 构建以 Cortex-M4、RISC-V、WASM 和移动目标为基准，主机 API 与 CLI 在 Linux、Windows 和 macOS 分别构建。浏览器覆盖窄屏、平板和桌面。

在本机容器运行时测试中，12 个并发客户端通过 Caddy 发起 200 次 CRUD 读写混合请求，均成功，p95 为 66.20 ms。Linux x86_64 musl API 发布二进制约 3.5 MiB。运行镜像以预编译的实际产物验收，覆盖同源静态文件、API 代理和重启后的 SQLite 卷。

从安装后的脚手架生成两种 preset。API preset 在原样 Compose 环境中注册并写入新生成的 Notes 资源。全栈 preset 从官方 Rust、Node 和 Caddy 构建镜像，验证了同源 API、数据卷、重启恢复以及中文版 Chromium 工作台。320、390、768 和 1440 px 均通过横向溢出检查。Cortex-M4、RISC-V、WASM、Android、iOS、OHOS、Windows 和 macOS 的纯核心均通过目标编译检查。

SQLx 0.9.1 支持与 rusqlite 共用打过补丁的 `libsqlite3-sys` 0.39.1。实测同一个 SQLite 文件可在 rusqlite、SQLx 与 SeaORM 间切换。PostgreSQL 15 与 MySQL 8.4 的实库测试覆盖 SQLx 和 SeaORM 的读取、写入、重连和快照回读。Pingap 的生成项目验证了静态资源和 API 转发，交互文档在上述四种宽度提交了带保留字符与 Unicode 的参数。[SQLx](https://github.com/transact-rs/sqlx) · [SeaORM](https://www.sea-ql.org/SeaORM/docs/install-and-config/database-and-async-runtime/) · [Pingap](https://pingap.io/)

Lilyco 原有主 workspace 的本机全量验收仍受已有格式差异与 Tauri 缺失 `binaries/lbin-x86_64-unknown-linux-gnu` 影响。Rapid 和改名后的独立核心通过各自的格式、Clippy 和测试门禁。
