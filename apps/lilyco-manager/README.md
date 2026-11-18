# Lilyco Manager MVP

独立于主 Cargo workspace 的本地浏览器管理器，首期选择 Axum + SeaORM 2 (SQLx SQLite)。仅回环监听、随机端口，浏览器启动后通过 URL fragment 将用户提供的随机会话 token 传到页面；每次 API 调用携带 x-lilyco-token。SQLite 保存只读工具执行历史。

## 运行
设置强随机环境变量 `LILYCO_MANAGER_TOKEN` 后启动 lilyco-manager。Linux 可自动使用 /dev/urandom；Windows/Linux 默认生成强随机 token，可通过环境变量覆盖。测试或服务模式可设置 `LILYCO_MANAGER_NO_BROWSER=1`。

巡检或演示时设置 `LILYCO_MANAGER_READ_ONLY=1`：面板显示只读提示并禁用写控件，所有写接口在鉴权后返回 403；GET、日志读取与日志流票据仍然可用。

## 当前范围
已实现两个内置只读工具、发现、调用、历史查看和自动打开浏览器；应用总览（清单发现、启动/停止、日志、`--schema`）、项目/方案、MCP 工具聚合/配置导出、GitHub Release 安装/更新/卸载、仓库内开发任务编排，以及基于 `lilyco-42/ui-spec` 的统一设计系统（浅色优先 + 深色主题）。尚未实现 Turso / wtx 后端。后续先通过 facade 读出 CommandSchema，再重用 core executor 做真实工具调度，不允许 Web 层直接执行任意外部命令。

## 应用安装与更新
- `GET /api/apps/:id/release` 查询 GitHub 最新 Release、当前托管版本与平台资产。
- `POST /api/apps/:id/install` 下载并校验 SHA-256 后安装到 `$XDG_DATA_HOME/lilyco/apps/<id>`；安装前会先停止由管理器启动的旧进程。
- `POST /api/apps/:id/uninstall` 删除托管安装目录；不触碰源码仓库或 PATH 中的版本。
- 二进制解析优先级：`LILYCO_MANAGER_BIN_<ID>` > 托管安装 > `PATH`。
- 只安装带 digest 的正式 Release 资产，不执行安装脚本。
- 安装链路有本机 HTTP 发布源集成测试：下载 → SHA-256 校验 → 落位 → 更新 → 卸载，全程不访问 GitHub。

## 开发任务
- `GET /api/apps/:id/dev` 列出清单中的固定任务与最近任务记录。
- `POST /api/apps/:id/dev/:task` 在仓库目录执行固定 argv，返回 job id。
- `GET /api/dev/jobs/:job/logs` 读取任务输出与退出码；HTTP 不能传 argv。
- 面板默认用 SSE 实时接收：`POST /api/dev/jobs/:job/stream-ticket` 换一张 30 秒、单次、
  绑定该 job 的票据，再 `GET /api/dev/jobs/:job/stream?ticket=…` 订阅增量行与 `done` 事件。
  `EventSource` 不能带自定义 header，票据让长期 token 不进入 URL 或访问日志。
- 流先推已有行，再按 150 ms 轮询内部缓冲推送新行；任务结束后发 `done`（含退出码）并关闭连接。
  增量定位用累计行号而不是缓冲长度，500 行环形缓冲开始裁剪后仍会继续推送且不重复。
  票据重放、换 job 使用或过期都返回 401。

## 项目 / 方案
- `GET /api/projects` 返回 `{projects, count}`，方案直接嵌套在项目的 `plans` 里；
  `POST /api/projects` 创建项目，`GET/PATCH/DELETE /api/projects/:id` 读取、更新或级联删除。
- `POST /api/projects/:id/plans` 新增方案；`PATCH/DELETE /api/plans/:id` 更新标题/说明/状态或删除。
- 项目状态 `active | paused | done`，方案状态 `draft | doing | done`；名称与标题必填，
  长度或状态不合法返回 400，未知 id 返回 404。无 token 的写请求在 JSON 解析前就返回 401。
- 数据与运行历史同库（管理器自己的 SQLite），重启后仍在；面板「项目/方案」一次请求即可渲染。
- 六类成功变更写入运行历史（`project.create/update/delete`、`plan.create/update/delete`），
  面板「运行历史」可展开查看审计 JSON；失败请求与只读拒绝不落审计。

## 端口管理

- 段表与规则见 [docs/PORT-ALLOCATION.md](../../docs/PORT-ALLOCATION.md)：4200 金融、4300 游戏、4400 媒体、4800 AI、8000 运维、8700 直播、57890 内部服务。
- 应用在清单 `run` 段声明 `preferred_port` / `port_env` / `port_band`；启动时在段内探测空闲端口并注入环境变量，冲突自动 +1 漂移（最多 20），不跨段、不抢外部端口。
- `GET /api/ports` 返回各段 `used/free` 与每个应用的 `suggestedPort`、`drifted`，可当冲突预警。
- 漂移只在日志里提示，调用方不得改 `port_env` 绕过。

## 应用清单
应用通过仓库根目录的 `lilyco.app.json` 自助注册，规范见 [docs/MANIFEST.md](docs/MANIFEST.md)。开发时可直接指向源码目录：

```bash
LILYCO_MANAGER_APPS=/home/lilyco/code lilyco-manager
```

清单目录默认是 `$XDG_CONFIG_HOME/lilyco/apps` 或 `$HOME/.config/lilyco/apps`。启动参数与端口来自清单，HTTP 只能传应用 id。多个应用默认都抢 8766，需在 `run.env` 分配不同端口。

## MCP 聚合
- `/api/mcp/tools` 动态执行各应用的 `--mcp` 并读取 `tools/list`，工具名统一为 `<app>__<tool>`。
- `/api/mcp/config` 生成 `lilyco-toolhub` 的 `servers` 映射；`POST` 写入 `~/.config/lilyco/tools.json`。
- 设置 `LILYCO_TOOLHUB=/绝对路径/apps/lilyco-toolhub/hub.py` 后，页面会给出 Claude / Cursor / Codex 客户端配置片段。

## 设计积木与接口清单
- 左侧「设计积木」是本地原型工作台：组件来自 `frontend/src/components/ui/` 的 shadcn 风格原语
  （Button / Card / Badge / Input / Textarea / Select / Switch / Alert / Separator / Label），
  使用 shadcn CSS 变量桥和 ui-spec 令牌。
- 左侧选积木、中间实时预览、右侧改属性；底部 JSON 调试面板支持粘贴、校验、复制和导出。
  未知积木或类型错误只报错，不覆盖最后一个可用预览；不会写入后端或磁盘。
- 「页面组合」把积木拼成原型页：追加（上限 12 块）、上移/下移、删除、点击选中后改属性，
  整页 JSON（`{title, blocks[]}`）可校验、复制、导出；校验失败保留最后一次应用的页面。
- `GET /api/openapi.json` 返回同一套路由的 OpenAPI 3.1 形状清单（同样要求 token）。
  契约测试会遍历文档里的每个 path + method，无 token 必须得到 401，避免文档与路由漂移。
- 左侧「接口文档」直接渲染这份清单：按 tag 分组（工具 / 应用 / 项目 / 发布 / 开发任务 / MCP / 元数据），
  每行显示方法徽标、路径与摘要，可刷新、可复制整份 JSON。
- GET 操作可「试调用」：填写路径参数后带当前会话 token 发请求，显示状态码、耗时与格式化 JSON；
  写操作（POST）不在文档页暴露试调用，仍从各自面板执行。

## 发布

管理器自身的三平台资产由 `.github/workflows/lilyco-manager-release.yml` 产出：推送 `manager-v*`
标签或手动 `workflow_dispatch`（需填标签）都会在对应平台编译、打包、解包自检，并上传

- `lilyco-manager-linux-x64.tar.gz`
- `lilyco-manager-windows-x64.zip`
- `lilyco-manager-macos-universal.tar.gz`（arm64 + x64 `lipo` 合并）

发布 job 还会核对 GitHub 返回的资产 `digest` 是否为 `sha256:`，并附带 `SHA256SUMS`；安装端据此校验。
标签前缀用 `manager-v`，不会触发 `ci.yml` 里针对应用二进制（`v*`）的发布 job。

## 安全
只监听 127.0.0.1，默认不暴露外网；token 不写入持久化数据库。不要将打印的完整 URL 或 token 外传。浏览器本地页面有意不加载第三方脚本。


## 新版前端

Vue 3 + TypeScript + Vite + Tailwind CSS 4；发布时嵌入 frontend/dist/index.html。设计令牌来自 `lilyco-42/ui-spec`，浅色优先，`html[data-theme="dark"]` 切换深色。Tailwind 通过 `@theme inline` 暴露 `bg-canvas`、`text-ink`、`border-line` 等语义类，并提供 shadcn CSS 变量桥（`--background`、`--foreground`、`--card`、`--primary` 等）。开发约束见 AGENTS.md、docs/TECHNICAL_SPEC.md 和 docs/UI_SPEC.md。

开发闭环、测试分层、CI 矩阵与跨平台约束见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)；性能预算与测量方法见 [docs/PERFORMANCE.md](docs/PERFORMANCE.md)。

发布源有两条验证路径：常规 `cargo test` 用本机 HTTP 假发布源覆盖安装/更新/卸载（不访问网络）；
被忽略的 `live_install_lycodl_from_github_release` 走真实 GitHub Release（`lilyco-42/lycodl`），
本地用 `cargo test -- --ignored live_` 手动跑，CI 由 `live-release` job 在手动触发与每周定时执行。

全栈浏览器用例 `frontend/tests/fullstack.spec.ts` 会启动真实 release 二进制（临时 SQLite + 随机端口），
在浏览器里执行工具、查看历史、打开积木页与接口文档；本地先 `cargo build --release`，再
`LILYCO_MANAGER_E2E_BIN=target/release/lilyco-manager bun x playwright test tests/fullstack.spec.ts`。

## Read-only executable adapters

API: GET /api/adapters returns availability for desktop.status and lyco.version.
The manager only calls a fixed executable and fixed arguments:
- lyco-desktop-switcher: status --json
- lyco: --version

Executable resolution: LILYCO_DESKTOP_SWITCHER_EXE and LILYCO_CLI_EXE override PATH. Overrides must be absolute existing files. The UI reports installed/unavailable status. Calls have a 4-second timeout and bounded stdout. No shell, plugin installation, writes or desktop switching are exposed in this phase.

To use: install the signed/trusted executable from its upstream project, then set an absolute override path or add the program to PATH. Restart manager to refresh its process environment. An executable in PATH is trusted code; do not add writable/untrusted directories to PATH.

## 本地电商商品图工坊
- 启动 Manager EXE 后打开左侧 商品图工坊；本机浏览器处理，上传服务器为零。
- 透明 PNG 可由 rembg-ui 生成，再按 SKU 批量居中、换底色、亮度调整、生成 ZIP。
- WebGPU 用于亮度调整，CPU 自动回退；WebGPU/WASM AI 抠图模型不属于当前版本。
- 自己或第三方项目投稿使用 GitHub 生态项目 Issue 模板，经人工审核后进入发布目录。
