# Lilyco Codegraph — AI / Agent 代码图谱

> 给 AI Agent 和新贡献者的**唯一导航入口**。所有符号引用带 `文件:行号`，可直接跳转。
> **维护规则**：任何 PR 改动下述符号（新增/移动/改名/删除）必须同步更新本文件 —— 让图谱随代码漂移等于没有图谱。
> **开源 Crate 复用规范**：[docs/CRATE_CATALOG.md](CRATE_CATALOG.md)（新功能开工前必查，禁止重复造轮子）。

## 0. 30 秒心智模型

**一个 struct 派生四个界面**：用户在业务 struct 上 `#[derive(App)]`，宏生成 `CommandSchema`（机器可读的命令描述），四个后端把同一份 schema 渲染成 CLI / TUI / Web / MCP，执行统一走 `core::executor`。

```mermaid
flowchart TD
    A["用户 struct + #[derive(App)]"] --> B["CommandSchema<br/>core/schema.rs"]
    B --> C[lilyco-cli: clap]
    B --> D[lilyco-tui: ratatui 表单]
    B --> E[lilyco-gui: axum + shadcn/ui]
    B --> F[lilyco-mcp: stdio JSON-RPC]
    C & D & E & F --> G["core::executor<br/>唯一执行宿主"]
    G --> H["Handler(&Context, args JSON)<br/>→ Progress 事件流 → Done/Error"]
    R["core::Registry<br/>多命令 + 别名/隐藏"] --> C & D & E & F
```

## 1. Workspace 依赖（严格单向，禁止反向）

### Lain42 门户与运营管理

`apps/lain42-portal/web/src/pages/OperationsPage.tsx:220` 提供 `/admin`，从 `/tool` 的管理员入口进入。
`apps/lain42-portal/src/admin_routes.rs:21` 装配统计、两站商品对比和页面访问采集。
`apps/lain42-portal/src/admin_analytics.rs:120` 从 dev 本地账本汇总注册、活跃、积分和订单。
`apps/lain42-portal/src/visitor_region.rs:22` 根据连接来源查询 `LAIN_REGION_DB`，访问表仅记录地区代码。
管理员登录签发独立会话。`LAIN_ADMIN_SECRET` 用于管理签名，`LAIN_ADMIN_USERS` 决定可登录的管理员。
两站按商品 SKU 对照。主站账户、订单、积分、校园跑权益及订阅保持来源站点。
`apps/lain42-portal/web/src/pages/CampusServicePage.tsx:4` 连接主站权益和续费入口。
`apps/lain42-portal/src/proxy_health.rs:57` 从主站读取公开网关检测并缓存，测量不参与订阅配送。

### 应用扩展：素材工坊

`apps/asset-studio/` 是独立的本机 FastAPI + React/shadcn 前端，调用已编译 `laic`，不复制解析器或 core 执行器。
入口 `launcher.py:17`（Windows WebView2 / Linux 浏览器）；路由装配 `studio/app.py:35`；固定 CLI 适配器 `studio/adapters.py:29`；
子进程任务与取消 `studio/jobs.py:26`；受限工作区路径 `studio/paths.py:14`。
只在 loopback 提供服务，以 Host/Origin/随机会话 Cookie 保护文件与 API。
不接受 shell 命令或可执行路径，不向 WebView 脚本暴露 Python API。Spine 播放适配器只连接可信引擎；
公共 CI 构建 AIC 包，维护者独立授权流程构建 AIC + Spine 3.8 产品包，携带单独 Runtime 许可，不能把它当 MIT 分发。
前端入口 `ui/src/studio-entry.tsx`，界面 `asset-studio-workspace.tsx`，工作区和任务状态 `use-asset-workspace.ts`；
原始 shadcn 组件只读，语义色与明暗主题在 `studio-theme.css`，约束见 `DESIGN.md`。
验证和完整包构建仅由 GitHub Actions 执行；包含自制 PXLS 的真实原生引擎往返、桌面/手机布局、
打包后真实 WebView2、关闭清理、安装/卸载验收。NSIS 卸载验收显式复制原卸载器到测试目录，
以官方 `_?=$INSTDIR` 参数等待真正的卸载进程结束并核对退出码，再验证全部包文件、开始菜单及用户文件保留，
不把启动卸载器的父进程退出误判为全部清理结束。`packaging/build-app.py` 是统一冻结配方，
`packaging/windows-installer.py` 仅删除明确列出的应用文件，禁止递归卸载用户目录。
Windows SDK 署名由 `apps/asset-studio/tests/notices.py` 对固定官方 NuGet 包、完整许可及五个真实冻结 DLL 做字节核对。
`packaging/webview2-sdk.json` 固定官方来源和摘要；公共包与授权组合包都必须携带独立 SDK 许可，
不能用 pywebview 或 Spine 的许可替代。公共 Asset Studio workflow 也运行完整 workspace fmt/clippy/test 门。

### 应用扩展：图片工坊（Vision Studio）

已构建包的独立字节审查与发行由 `scripts/reviewed_vision_release.py:103`（`inspect`）和
`.github/workflows/reviewed-vision-release.yml` 执行。
只接受已成功的正式 main 产品 workflow；核对全部 source gates、外层 artifact 摘要、ZIP 全文件清单与摘要、
官方 SDK 五 DLL／完整许可、八个原工具、冻结／原生／安装卸载和各尺寸真实交互证据。
PR 与 push 仅审查，不发布；显式 main dispatch 且发布器主线与完整 workspace 门通过才创建六资产 draft，
GitHub 资产摘要与大小全部匹配后公开，已公开的不同文件永不覆盖。不通过本机下载／编译规避门禁。
未公开 draft 通过认证后的分页列表定位唯一 tag，再按固定 release ID 核验目标与资产；
不依赖仅查询已公开发行的 tag 端点，不在失败后重复创建同名草稿。

`apps/vision-studio/launcher.py:17` 提供本机 WebView2 / 浏览器入口；`studio/app.py:26` 是 loopback HTTP 组合根，
Host / Origin / 按端口随机 HttpOnly Cookie 隔离应用窗口，文件保存在临时工作区；`studio/paths.py:13` 限制下载路径。
`studio/adapters.py:44` 固定工具与文件路径、像素资源上限，禁止任意命令/输出路径；参数领域语义仍由原 Rust `CommandSchema::validate_args` 校验。
`studio/mcp.py:12` 通过标准 JSONL stdio 调用已编译 `lvision --mcp`，接收进度、120 秒超时、取消清理子进程树。
先从原 MCP tools/list 发现实际工具与参数拼写；Windows 用 `studio/windows_process_job.py` 的 nested Job 包住这次引擎及其后代，关闭前等待 ActiveProcesses=0；Unix 用独立进程组，不按浏览器名称全局杀进程。
`studio/jobs.py:13` 仅管理 HTTP 任务生命周期和真实导出，不重写 Rust core 的线程/进度执行宿主。
UI 使用 `ui/src/vision-workspace.tsx` / `use-vision-workspace.ts` / `vision-tools.ts`，原 shadcn 源组件保持只读。
`ui/src/vision-crop-preview.tsx` 把指针框选映射到原图像素坐标，保留键盘参数输入；图片信息直接显示格式、尺寸与文件大小。
八工具来自 `lilyco-vision/src/main.rs:44` 注册表；截图 URL `screenshot.rs:117` 归一化 Windows 扩展路径并转义，
截图命令 `screenshot.rs:146` / `run_html_screenshot:175` 使用临时 profile，
不会复用个人浏览器 Cookie。背景提取是确定性边界洪水填充，不宣传为 AI 抠图；HTML 需已安装 Chrome / Edge。
截图 profile 位于输出所在目录；`studio/profile_cleanup.py:8` 记录调用前已有 profile，只清理本次新增且未越界的目录。
在当前进程树退出后，Windows 临时文件句柄释放最多等待 5 秒；持续锁定仍报失败，不能在取消接口里覆盖成成功。
真实取消回归先等待浏览器创建 Default profile，再确认原进程退出及 profile 全部消失；另覆盖瞬时锁定、持续锁定和保留已有目录。
`.github/workflows/vision-studio.yml` 是唯一构建/测试入口：原八工具的真实像素、透明度、SVG 和 HTML 截图、
HTTP 边界、320–1440 响应式、冻结程序、原生 WebView2、正常关闭、安装与安全卸载。
包携带完整依赖许可、锁文件、原始输出和逐文件 SHA；当前分支未过门前不称为已发布。
Windows 包在 `tests/notices.py` 对 `packaging/webview2-sdk.json` 固定的官方 NuGet SDK、独立 BSD 文本及五个实际冻结 DLL 做字节核对；`packaging/archive.py` 携带 SDK 许可和来源清单，不能把 pywebview 源许可当成这些 SDK 程序集的许可。


| crate | 版本 | 依赖 | 职责一句话 |
|---|---|---|---|
| `lilyco-core` | 0.3.2 | serde/thiserror，`ui-schema` 可选 schemars | 领域模型、执行校验与 Web 数据契约 |
| `lilyco-macros` | 0.3.1 | syn/quote | `#[derive(App)]` / `#[derive(ValueEnum)]` 代码生成，可嵌入应用语言包 |
| `lilyco-cli` | 0.3.1 | core + clap | 参数类型、分组帮助、面向人员和程序的输出 |
| `lilyco-tui` | 0.3.1 | core + ratatui | ratatui 表单状态机（单/多命令）、运行进度和结果 |
| `lilyco-gui` | 0.3.3 | core + axum/tokio + toml | 默认 shadcn/ui、双语资源、SSE 与 Agent 自描述 |
| `lilyco-mcp` | 0.3.0 | core（零额外依赖） | MCP 2024-11-05 stdio 服务器 + 进度通知 |
| `lilyco-tauri` | 0.3.2 | Tauri + lbin sidecar | Office 原生选文件、拖放、双语界面和结果 |
| `lilyco-native-ui` | 0.3.1 | 独立 workspace，NSH/GTK4 | `UiSpec` 原生渲染、表单焦点和选择状态 |
| `lilyco` | 0.3.1 | 全部 | **唯一组合根**：后端自动选择 + 各形态入口 |
| `lilyco-dev` | 0.3.0 | facade | **元 CLI**（bin `lilyco`）：`new` 脚手架 / `build` / `run` / `doc` 能力表 —— 它自己就是四命令的 dogfooding 样板 |
| `lilyco-vision` | 0.3.1 | facade + image + vtracer + tempfile | 原有八视觉工具；截图独立临时 profile，图片工坊通过 MCP 复用 |
| `lilyco-web2cli` | 0.3.2 | facade + ureq + html5ever + tungstenite + base64 + url + regex + toml + tempfile | **Web2CLI**：网页 / OpenAPI / HAR 自动发现并编译为 `Registry`（CLI / TUI / Web / MCP），内置 Cloudflare 响应诊断、ego-lite 人工交接、分类资料库（`vault`）、多格式 Cookie 导入、CDP 多层 `iframe` 控制、多步向导与人类审核闸门（`review`/`wizard`）、成功模板沉淀（`recipe`） |
| `lilyco-chatgpt` | 0.3.0 | facade + ureq + serde_json + tungstenite（CDP 客户端：`login --from browser` 抓登录态、`ask` 的 chat 路径驱动网页，纯 Rust）+ axum/tokio/async-stream（可选 `server` feature：`serve` 命令的 OpenAI 兼容端） | **ChatGPT**（bin `lchatgpt`）：把 chatgpt.com 账号后端写成四端命令 —— 会话历史只读（`whoami`/`usage`/`models`/`list`/`show-conversation`/`search`/`auth`，全 T0）、两条发送路径（`ask`，T1）、登录态入库（`login`，T1）与一个 OpenAI 兼容 HTTP 端（`serve`，T1，给 dsh 之类的外部宿主当模型 provider）。**两种额度分开花**：`ask` 默认走 chat 额度（驱动已登录 chatgpt.com 的 chromium 发、用自己的 Bearer 走 REST 读回），codex 额度必须显式 `--allow-codex` 才动。产物是单一自包含二进制：凭证按 env → `LCHATGPT_AUTH_FILE` → 自己的仓库 `~/.local/share/lchatgpt/auth.json` → `~/.codex/auth.json` 回落，**可选**地用浏览器（登录态来源 + chat 路径要驱动的那台），端口没人照样可用 |
| `lilyco-account-pool` | 0.1.0 | 独立 workspace：`axum` + `rusqlite` + `aes-gcm` + `hmac` | BYO 私有账号池服务：dev HMAC / 生产 HS256 JWT 双鉴权、`X-Pool-API-Key` Agent 接入、凭证 AES-256-GCM 加密、服务器侧 Buddy Switch 签到调度；公网路由须单独验证发布，调试入口为 `127.0.0.1:8793` |

Android/Termux：`lilyco --no-default-features` 剩 CLI+MCP（crossterm/axum 被特性门控）。

## 2. lilyco-core（领域核心）

| 符号 | 位置 | 说明 |
|---|---|---|
| `trait App` | `lilyco-core/src/app.rs:10` | `schema()`、`from_args()`、`run(&Context)`、可选 `views()` 和 `ui_messages()`；宏自动实现 |
| `trait Renderer` | `lilyco-core/src/app.rs:34` | `render(&CommandSchema) -> Output`，各后端实现 |
| `struct CommandSchema` | `lilyco-core/src/schema.rs:40` | 命令的机器可读描述（四端渲染的唯一事实源） |
| `enum ArgKind` | `lilyco-core/src/schema.rs:21` | Flag/Text/Number{min,max}/Enum/Path{must_exist}/List |
| `validate_args()` | `lilyco-core/src/schema.rs:105` | **三端唯一参数校验实现**（required/range/enum/must_exist/List 递归） |
| `to_json_schema/openai/anthropic` | `lilyco-core/src/schema.rs:49,73,85` | AI tool 定义导出 |
| `type Handler` | `lilyco-core/src/registry.rs:23` | `Arc<dyn Fn(&Context, &Value) -> Result<Value, AppError>>` |
| `struct Registry` | `lilyco-core/src/registry.rs` | 多命令注册表；`register:148` `get(含别名):163` `visible:180` `from_json:194` `with_policy`（**必须在 `register` 之前调用**：门在注册时包住 handler） |
| `RegisteredCommand::from_app` | `lilyco-core/src/registry.rs:112` | App 类型 → 注册表条目，携带执行器、视图槽位和可选的应用语言包 |
| `spawn()` / `execute()` | `lilyco-core/src/executor.rs:35,92` | **唯一执行宿主**：后台线程 + 进度 channel |
| `struct Task` | `lilyco-core/src/executor.rs:20` | `cancel` + `rx` + `handle` |
| `enum Progress` | `lilyco-core/src/progress.rs:10` | Started/Tick/Log/Telemetry/Done/Error/**View**，serde tag=`type` |
| `enum Component` | `lilyco-core/src/view.rs:136` | 52 个语义视图组件（snake_case `kind`，容器带 `children`）；后端只声明，前端按 kind 渲染 |
| `struct ViewSlot` | `lilyco-core/src/view.rs:497` | 静态槽位：`#[view(id,title,kind)]` → `App::views()` → `run_registry --schema` 的 `views` |
| `struct ThemeSpec` / `enum ThemePreset` / `struct AdapterCapabilities` | `lilyco-core/src/theme.rs:14,104,284` | 10 年期 UI 主题与多框架适配器契约（10 套主流框架预设 + CSS/JSON 导出 + 协议主版本兼容与优雅降级） |
| `Context` | `lilyco-core/src/context.rs` | handler 上报进度：`emit:71` `tick:81` `log:92` `done:100` `is_cancelled:76` `view:131` `view_progress:139` |
| `trait HostBridge` | `lilyco-core/src/context.rs` | handler 反向调用宿主的唯一接口：`ctx.sample()`（MCP sampling/createMessage）/ `ctx.roots()`；CLI/TUI/GUI 不接桥，返回带指引错误 |
| `enum AppError` | `lilyco-core/src/error.rs` | InvalidArg/InvalidInput/Runtime/Cancelled |

## 3. 四个后端

### lilyco-cli（`lilyco-cli/src/`，按职责分文件；对外只有 `CliRenderer` / `run` / `run_registry` / `build_registry_command`）
| 符号 | 位置 | 说明 |
|---|---|---|
| `struct CliRenderer` | `renderer.rs:27` | schema → clap 的渲染入口；`handle_builtin_flags():46`、`output_format():90`、`extract_args():103` |
| `run::<A>()` | `single.rs:17` | 单命令一行启动（clap 校验 → extract → executor → drain） |
| `run_registry` / `run_registry_from` | `registry.rs:28,33` | **多命令**：Registry → clap 子命令树（支持显式参数列表）；根级 `--schema` 打印清单 |
| `build_registry_command()` | `registry.rs:64` | 构造根 Command，沿用统一的 Clap 帮助样式 |
| `resolve_registry_command()` | `registry.rs:93` | 规范名/别名 → 注册表条目（含 schema.name 兜底） |
| `drain_events()` | `registry.rs:125` | 单/多命令共用进度消费。人类可读进度写 stderr，结构化结果和流写 stdout |
| `build_command()` / `add_builtin_flags()` | `command.rs:23` / `command.rs:152` | `ArgKind` 决定值类型提示，内置选项按自动化、输出与界面分组 |

### lilyco-tui（状态机，三文件）
| 符号 | 位置 | 说明 |
|---|---|---|
| `enum AppState` | `lilyco-tui/src/renderer.rs:43` | CommandSelect → Form → Confirm → Running → Done/Error |
| `struct TuiApp` | `lilyco-tui/src/app.rs:13` | `new():33` 单命令；`new_multi():49` 多命令（mininterface picker 模式） |
| `handle_event()` | `lilyco-tui/src/app.rs:85` | 按状态分发；帮助窗口隔离焦点，Alt+↑/↓ 可跳过路径补全 |
| `struct FormRenderer` | `lilyco-tui/src/renderer.rs:14` | `validation_errors():131`（提交前校验）、`cli_preview():98` |
| `struct FormField` | `lilyco-tui/src/widgets.rs` | 按终端列宽分配标签和值，聚焦长路径时展示编辑端 |
| `render_form()` / `render_command_select()` | `lilyco-tui/src/renderer.rs:151` / `lilyco-tui/src/renderer.rs:214` | 在可用视口内保留当前字段或选中命令，底部呈现上下文快捷键 |
| `render_running()` | `lilyco-tui/src/renderer.rs:312` | 由 Running 状态调用，展示进度、最新日志与取消键 |
| `path_complete()` | `lilyco-tui/src/app.rs` | Path 字段 Tab 目录补全（readline 风格循环候选；`split_dir_prefix` 兼容 `/` 与 `\`） |

### lilyco-gui（`lilyco-gui/src/`，按职责分文件；对外只有 `GuiRenderer` / `RunnerFn` / `TOKEN_HEADER`）
| 符号 | 位置 | 说明 |
|---|---|---|
| `serve_app::<A>()` | `lib.rs:165` | 单命令：**内部建单命令注册表**再走 `serve_registry`（只有那条路登记取消句柄，见 DESIGN.md §8.6）；`serve(schema, runner):81` 是给自定义 runner 的逃生口（页面据此不画「取消」） |
| `serve_registry()` | `lib.rs:97` | **多命令**；`GET /?cmd=xxx` 渲染对应表单 + 下拉切换 |
| `serve_state()` | `lilyco-gui/src/lib.rs` | `/` 加载 shadcn/ui，`/classic` 加载经典控制台，路由共用回环令牌中间件 |
| `UiCatalog` / `UiRunRequest` | `lilyco-core/src/web_ui.rs` | 可见命令、参数、槽位、执行请求。由可选 `ui-schema` 导出 JSON Schema，前端用生成的 TypeScript 类型 |
| `catalog()` | `lilyco-gui/src/shadcn.rs:24` | `/ui/catalog` 提供类型化数据，含可选的 CLI 可执行文件与调用方式。适配器按 `ArgKind` 转换控件输入，按 `Component` 和 SSE 更新视图 |
| Web 表单、输出与文件交互 | `lilyco-gui/web/src/{main,fields,files,output}.tsx` | shadcn 工作区宽屏并排，窄屏可跳转输出。上传绑定对应路径，列表更新基于最新草稿 |
| shadcn/ui 组件来源与构建 | `lilyco-gui/web/README.md` | 官方 Registry 组件及锁、前端主题、契约生成与离线嵌入流程 |
| `security_mw()` | `security.rs:67` | 回环 Host 校验 + Origin 校验 + 随机 Token（防 DNS rebinding/CSRF），涵盖 `POST /cancel/{id}` |
| `AppState` | `state.rs:17` | schema / registry / sessions / cancels / token 一处真相（`pub(crate)`，测试夹具在同文件 `fixture`） |
| `pick_command()` | `render.rs:36` | `?cmd=` → 可见命令（别名命中；隐藏回退第一个可见） |
| `render::index()` | `render.rs:122` | 经典控制台的 HTML 装配与响应头 |
| `render_field()` | `render.rs:224` | **一个 `ArgKind` 一个组件函数**（`widget_flag/text/number/enum/path/list` + `field_shell`/`dropzone`/`pick_button`/`list_row`/`command_nav`），转义在 `Field` 里算一次 |
| 前端组件装配 | `assets/index.html` | 末尾 `BOOT` 装配语言、快捷键与文件控件。键盘提交和拖放走同一个表单 |
| 设计令牌与组件目录 | `lilyco-gui/DESIGN.md` | 命令工作区布局、DOM 契约、i18n 与键盘操作 |
| 双语资源 | `lilyco-gui/src/i18n.rs` / `assets/i18n/ui.toml` / `web/src/i18n.ts` | 两种 Web 渲染器共用语言偏好、框架词条和由 `App::ui_messages()` 提供的命令与参数文案 |
| Agent 自描述 | `ai_web.rs:27` / `:56` / `:88` | LLM 指南、Agent Card、OpenAPI（令牌与 SSE 契约） |
| `run_handler()` | `run.rs:88` | `/run`：多命令按 `req.cmd` 显式分发（未知/隐藏 → 400，**绝不静默换命令**）；单命令 runner 模式直接 spawn 调用方的 `RunnerFn` |
| `run_progress()` | `run.rs:45` | 唯一会登记取消句柄的执行循环：handler → `executor::spawn` → SSE 事件转发 → 终态清理 `cancels`（`/cancel` 只找得到这条路上的会话） |
| `upload_handler()` | `files.rs:98` | 拖拽上传 → base64 → 服务端临时副本（净化文件名 + 双重体积上限） |
| `pick_handler()` | `files.rs:201` | 本机原生选择器 → 回填**原始路径**（`pick` 特性；`flash_picker_when_ready:160` 治前台锁定） |

### lilyco-mcp（`lilyco-mcp/src/`；对外只有 `McpServer` + 协议常量）
| 符号 | 位置 | 说明 |
|---|---|---|
| `handle_line()` | `server.rs:35` | 纯函数：一行请求 → 一行响应（通知返回 None） |
| `handle_line_with_sink()` | `server.rs:44` | 流式版：进度通知逐行回调 |
| `tools_call()` | `server.rs:238` | 先 `validate_args`（错误 → INVALID_PARAMS），带 `_meta.progressToken` 时 spawn 流式执行 → `notifications/progress` |
| `serve()` / `serve_stdio()` | `server.rs:101` / `server.rs:208` | 双向 JSON-RPC 分流：客户端请求→dispatch（tools/call 进 worker 线程），客户端响应→pending 表路由给等待中的 handler |
| `protocol.rs` | `progress_notification():14`、`initialize_response():37` | JSON-RPC 报文装配与协议常量（线格式只在这一层） |
| `McpBridge` | `bridge.rs:37` | HostBridge 实现：`sampling/createMessage` / `roots/list` 反向请求；`srv-N` 字符串 id 防冲突；initialize 探测客户端能力门控 |

## 4. facade `lilyco`（唯一组合根，`lilyco/src/lib.rs`）

| 符号 | 行 | 说明 |
|---|---|---|
| `detect_backend()` | 123 | 显式 `--mcp/--gui/--tui` 优先于 `LILYCO_UI`。仅无参数的交互式终端自动进入 TUI |
| `run::<A>()` / `run_with()` | 165 / 170 | 单命令四端自动选择 |
| `serve_mcp()` | 189 | 注册表 → MCP 服务器 |
| `run_cli_registry()` | 206 | 注册表 → clap 子命令 |
| `run_tui_registry()` | 220 | 注册表 → TUI 命令选择页（起不来回退 CLI 多命令） |
| `run_web_registry()` | 243 | 注册表 → Web 控制台（`?cmd=` 下拉 + `/run` 显式分发） |
| `run_registry()` | 282 | **多命令一行启动**：按 `detect_registry_backend()` 自动分发 |
| `run_registry_with()` | 300 | 显式指定后端；**按调用面注入 SafetyPolicy**（MCP→`DenyElevated`，其余→`Interactive`） |
| `run_registry_with_policy()` | 315 | 调用方自带策略（逃生舱，业务通常不要用） |
| `detect_registry_backend()` | 392 | 多命令形态探测：**自动探测出的 TUI 降级为 CLI**（裸跑不该被拽进交互界面；进 TUI 须显式 `--tui`） |
| `build_multi_tui()` | 488 | 纯函数：注册表 →（选择页 TuiApp + 名字→handler 表）。与事件循环分离以便无 TTY 单测（隐藏命令过滤、空注册表报错） |
| `into_registry_with_policy()` | 359 | 重建注册表时保留命令元数据。旧 handler 上已有的策略仍随 handler 生效 |
| `run_tui_event_loop()` | 515 | 单/多命令共享的 TUI 循环（非阻塞 drain + 可取消） |

## 5. 多命令语义对照（四端对齐）

| 语义 | CLI | TUI | Web GUI | MCP |
|---|---|---|---|---|
| 可见命令 | 子命令 + help | 选择页列表 | `?cmd=` + 下拉 | tools/list |
| 别名 | clap alias + registry 解析 | —（列表不显示） | `registry.get` 命中 | `registry.get` 命中 |
| 隐藏命令 | 可调用，help 不显示 | 不可见 | 不可导航；`/run` 显式调用 → 400 | get 可命中，tools/list 不显示 |
| 参数校验 | clap（解析层） | `FormField::validate`（提交前，同语义） | `validate_args`（服务端 400）+ HTML5 | `validate_args`（INVALID_PARAMS） |
| 进度 | stdout / json-stream | 非阻塞 drain | SSE | `notifications/progress`（带 token 时） |

## 6. 不变量（改代码前必读）

1. **executor 是唯一执行宿主**：任何"线程 + channel + 进度消费"的新实现都是重复 —— 加后端 = 渲染层 + 一处 `spawn` 调用。
2. **`CommandSchema::validate_args` 是唯一校验实现**：TUI `FormField::validate` 是其表单侧映射；新增校验规则先改 core，再同步 TUI。
3. **事件流协议**：`rx` 恒以 `Done`/`Error` 结尾（`executor::spawn` 合成兜底）——消费者无需自己兜底。
4. **依赖单向**：core 不依赖后端，后端互不依赖，facade 是唯一知道所有后端的 crate。
5. **clap 只接受 `'static str`**：运行时字符串用 `leak_str`（`lilyco-cli/src/command.rs:13`，Box::leak，进程内无累积问题）。
6. **导航可回退、执行不可回退**：`index` 的 `?cmd` 未知时回退第一个可见命令；`/run` 显式指定未知命令必须 400。
7. **宏展开引用 `::lilyco::__core::`（facade doc-hidden 再导出）**：用户只需依赖 `lilyco` 一个 crate；直接使用 `lilyco-macros` 的项目须同时依赖 `lilyco`（macros 的 dev-deps 即此契约）。
8. **安全策略按「调用面」注入，且在 `register` 之前**：`Registry::with_policy` 在注册时就把 handler 包进门，所以顺序不能反；`run_registry_with` 按后端选策略 —— **MCP = `DenyElevated`（自动化面，fail-closed）**，**CLI/TUI/Web = `Interactive`（人类在环，放行 T1）**。这是四端"同一份 handler、不同信任级别"的唯一实现点。`into_registry_with_policy` 只能用于**尚未注册**的注册表；对已注册的调它会在 handler 上叠第二道门（旧门先拒），表现为"换了策略还是被拒"。
9. **MCP `serve()` 在 stdin EOF 后必须 join 在途 worker**：`tools/call` 丢到 worker 线程执行（为支持 handler 反向 `sampling/createMessage`），短连接客户端（脚本 / `echo … | server`）写完即关 stdin —— 不等 worker 就会丢响应。回归测试：`serve_waits_for_inflight_tools_call_on_eof`。

## 7. 扩展点（怎么加东西）

- **加一个命令**：写 struct + `#[derive(App)]`（可选 `#[app(name/about/run/safety/crate)]`，字段 doc comment 即描述）→ `registry.register(RegisteredCommand::from_app::<T>())`。四端自动获得。
  `crate = "lilyco_core"` 是给**不依赖 facade** 的嵌入方用的：默认展开成 `::lilyco::__core::…`，
  两条路径下的条目一一对应（facade 里就是 `pub use lilyco_core as __core`）。
- **加一个域二进制**：**最快路径 `lilyco new <name>`**（`lilyco-dev` crate，模板内嵌自 `scripts/domain-template/`，crate `lilyco-<name>` + bin `l<name>`）；手抄则照 `lilyco-files`（文件域）或 `lilyco-binfmt`（办公文件与容器结构域，`lbin`，12 条全 T0 只读）抄 `main.rs` 的 registry 装配 + 按调用面注入安全策略 → 根 `Cargo.toml` 的 `[workspace] members` 加一行 → 使用文档放 `docs/<域>.md`（`docs/lfiles.md`、`docs/binfmt.md`）。
  **完整清单（12 步，含 CI 枚举那一步）与可复制骨架见 [INTEGRATION.md](INTEGRATION.md) + `scripts/domain-template/`。**
  应用 crate 只依赖 `lilyco` 一个包：core 的类型（`Context` / `SafetyPolicy` / `SafetyTier` / `Registry`…）从 `lilyco::prelude::*` 拿，
  `derive(App)` 默认展开成 `::lilyco::__core::…` —— 不需要把 `lilyco-core` 写进 `[dependencies]`（`lbin` 已按这条清掉）。
- **加一个后端**：新 crate 只依赖 core，实现 `Renderer`；facade 加一行分发。参考 lilyco-mcp（最小样板 ≈ 350 行含测试）。
- **选后端 = 开 facade 特性**：`lilyco-core` 的 `ui-schema` 用于导出数据类型；
  域 crate 一律 `default = ["full"]` / `full = ["dep:lilyco", "lilyco/full"]` / `android = ["dep:lilyco"]`；
  facade `default = []` + `full = ["tui","web","ultra"]`（后端各自 `dep:` 门控）；
  `lilyco-gui` 的 `pick` 只管它自己的原生文件选择器。
  新增域 crate 照这套命名，别再造 `headless` / `no-tui` / `cli` 这类同义词（core 原来那三个空壳特性就是这么来的，已删）。
- **加校验规则**：`schema.rs::validate_kind` + `schema.rs` 测试 + TUI `FormField::validate` 映射 + README 限制清单。
- **加进度事件**：`Progress` 枚举加变体（serde 兼容）→ 各后端消费者各加一臂。

## 8. 常用命令

```bash
cargo test --workspace            # 全量测试（984 项，见 readme 的 Testing 一节，含两个前置条件）
cargo fmt --all && cargo clippy --workspace --all-targets
cargo run -p lilyco-example --example multi -- ping --name 世界   # 多命令冒烟
cargo run -p lilyco-example --example multi -- --schema           # 注册表清单
cargo run -p lilyco-example -- --mcp                              # MCP 服务器冒烟
cargo bench -p lilyco-example                      # schema 生成性能基准
cargo run -p lilyco-dev -- new todo                # 元 CLI：脚手架 lilyco-todo（bin ltodo）
cargo run -p lilyco-dev -- doc --project lilyco-letsgal              # 元 CLI：能力表 CAPABILITIES.md + capabilities.json
```

发版：改各 crate 版本（本文件 §1 同步更新）→ commit → `bash scripts/publish.sh`（按依赖顺序全链发布；脚本用临时 CARGO_HOME 绕开 rsproxy 镜像滞后，验证构建走 crates.io 真实索引）。

## 9. 测试地图

| 区域 | 位置 | 覆盖 |
|---|---|---|
| core 校验/协议/registry | `lilyco-core/src/{schema,lib,registry}.rs` `#[cfg(test)]` | validate_args 12 例、Progress serde、registry 别名/隐藏/JSON |
| CLI | `lilyco-cli/src/tests.rs` | 渲染/解析/内置标志/多命令构建与解析（31 例） |
| TUI | `lilyco-tui/src/lib.rs` 底部 | 渲染、状态机、校验拦截、多命令选择页、路径 Tab 补全 |
| GUI | `lilyco-gui/src/{security,state,render,run,files,util}.rs` 各自底部 | run_handler 400/200、pick_command、?cmd 导航、转义、base64、文件名净化、`/pick` 同闸；**取消链**：registry 跑起来会登记句柄且终态清掉、runner 模式 `/cancel` 必 404；**组件契约**：六种 ArgKind 各自出组件且 `data-component` 自报、`List` 的行控件跟着 item 类型、JS 里每个 `init*` 都在 `BOOT`、装配行在脚本末尾（TDZ）、内联 JS 过 `node --check`、页面无内联事件、每个 `data-*` 都有人读、拖拽区 id 对得上 JS 的拼法、`no-store`、单边区间也要写进提示、`app.css` 的颜色字面量全在 DESIGN.md §1 那张表里、资源体积守 DESIGN.md §9 的上限、reduced-motion 下进度与 loading 留了静态替代 |
| MCP | `lilyco-mcp/src/tests.rs` | initialize/tools/进度通知/双向 serve/校验拒绝（26 例） |
| 门面 | `lilyco/src/lib.rs` 底部 | 后端探测 |
| 办公文件（同一个域） | `lilyco-binfmt/src/{zipread,xmlscan,cfb,props,rtf,word,biff,ppt,numfmt,odsheet,opack}.rs` 与 `{office_info,office_text,office_meta,office_doc,office_sheet,office_slide,office_package,office_objects}.rs` 底部 + `tests/fixtures/office/` | 解压必须过部件自报的 CRC-32；容器识别看部件名/流名不看后缀；OPC 关系按源部件目录解析、断头关系要报；`[Content_Types].xml` 的 Default 与 Override 两种声明都要认；CFB 链成环/越界就地截断并留话；MS-OPS 的属性集个数在偏移 24 不是 28、`VT_LPSTR` 按文件自报的 CodePage 解、VT_I2 按无符号读；RTF 控制字按词边界结束、`\*` 群整群跳过、`\uN` 按 `\ucN` 丢等价回退字符且 `\'hh` 只算一个；`.doc` piece 的压缩位（fc 除二 + cp1252）；BIFF8 的 SST 跨 CONTINUE 要重读 grbit、RK 的 bit0=除 100 / bit1=整数、BOUNDSHEET 可见性在 grbit 最低两位、格子的主人看 lbPlyPos（Workbook 流内字节偏移）而不是数 BOF 的次序；PowerPoint 97 记录树的表头是 recVer(+0)/recType(+2)/长度(+4)（三个已知原子落在 +2、整条流按此铺满才敢这么读）、容器只靠「正文能再铺成一条完整记录流」来判、TextCharsAtom 的高字节全零时才允许按 Windows-1252 低字节读；RTF 的 `\info` 要读到下一个非元数据目标为止（真件里 `\info{}` 只包住标题那一对 `\upr`）、`\upr` 只跳第一群（真值在 `\*\ud` 那边）、属性名按群整块交账、`\printim` 全零不能报成 0000-00-00；`--limit 0` 回退缺省而 `--max-bytes 0` 是不设上限（两种 0 不能混）；xlsx 的 `s=` 是 `xl/styles.xml` 的 `cellXfs` **下标**不是格式号（不绕这一层 41631 只是一个数）、自定义格式号从 164 起（openpyxl 把日期/百分比/货币全写成 164-168，内置表查不到）、格式串判类型要先剥引号字面量与转义和 `[...]` 段（反斜杠转义与引号包起来的字面量都不算格式标记，比如那个带「年/月/日」字面量的日期格式）、时间标记之后的 `m` 是分钟、换算看 `date1904`，而 1900 系统第 60 号是那个不存在的 1900-02-29：文件写着 60 就照 60 报、文本 `12/23/2013`（格式号 0）不许被猜成日期；`docProps/thumbnail.jpeg` 也声明成 image/jpeg，先认缩略图再认媒体，否则「有几张图」被一张预览图顶掉；OOXML 缺 `word/footnotes.xml` 就是 0 个脚注（只有遗留 `.doc` 真看不见，才给 null）；`office-text` 的 `index` 是「在这份列表里第几条」，正文与批注共用一条数，不能出现一条 9 夹在 7 与 10 之间、或两页各自从 0 数起；RTF 的 `\uc0` 说的是一个回退字符都不跳，那个 `?` 是正文；伪根 `#doc` 底下的纯空白永远不是内容（元素之间才用「带换行才丢」保住 `<w:t> </w:t>` 那个空格）。ODS 的格子内不写序列数（`office:date-value` 直接是 ISO 串，没有 1900/1904 基准要猜），位置靠 `table:number-columns-repeated` 累加（行尾那片空白一填就是 16381、行首也会重复 2 格，照字面数就成了一万六千格），覆盖格 `covered-table-cell` 单列一个数不当内容，表可不可见在它引的自动样式的 `table:display` 里，而 `calcext:value-type` 是 `office:value-type` 的实验区副本（按局部名乱抓会抓到它）；LibreOffice 在 `meta.xml` 自报的 `cell-count` 是第三方保证，两份读者都与它对账；`#doc` 的直接孩子是根元素：`root.child("body")` 永远找不到 `w:body`（它在 `w:document` 里一层），而计数走 `descendants` 又照样对 —— 于是假话只留在 notes 里，CI 跑完第三轮才露出来；ODF 文字的结构账在 `office:body > office:text`，脚注与尾注共用一个 `text:note`、靠 `note:class` 分家，批注是 `text:annotation`、图在 `draw:image` 的 `xlink:href`、标题层级在 `text:outline-level`，`meta:document-statistic` 是生产者自己写的段落/页/字数（拿来当第三方对账，而不是当自己的结论）；`office-text` 的页眉页脚那条分支是靠 `notes-hf.docx` 第一次走到真件的：python-docx 默认让第二节沿用第一节，必须 `is_linked_to_previous = False` 才会写出第二个 `header*.xml` —— 少了这一步，那条分支等于从来没被测过；ODP 的备注只认 `presentation:notes` 里 `presentation:class="notes"` 那个框，同级的 page-number 框里是样字 `<编号>`（整页 `descendants("p")` 一把抓就会把它读成正文），页名在 `draw:name`（`attr("name")` 取不到，得按局部名）、尺寸四跳 `draw:master-page-name` → styles.xml 的 `style:master-page` → `style:page-layout-name` → `style:page-layout-properties`，而 `presentation-page-layout-name` 只是名字（这份文件里一个版式定义都没有）；ODF 的批注是嵌在正文段里面的 `text:annotation`（作者/时间在 `meta:creator`、`meta:date` 这些**子元素**上，不是属性），整段 itertext 一把抓会把它接进正文，而 LibreOffice 自报的 paragraph-count 正是因为它把批注里那段也数了（10 = 9 正文 + 1 批注）；16 份 fixture 全由独立生产者写，期望值来自 `scripts/acceptance/office_reader.py`，CI 里 `office_probe.py` 与它逐字段对账 |
| 域二进制 | `lilyco-binfmt/src/{main,read,entries,regions,symbols}.rs` 底部 | 注册表形状/全 T0/工具导出/参数拒绝；魔数判别（Java class ≠ 通用二进制）、tar 八位校验和、PNG 真算 CRC、ELF 头部只到 `e_ehsize`、Mach-O 端序与定长 16 字节节名、零填充节不画成数据 |
| 域二进制（chatgpt） | `lilyco-chatgpt/src/{main,api,auth,authinfo,browser,chat,jwt,list,login,models,refresh,search,serve,show,store,usage,util,whoami,ask}.rs` 底部 | 注册表形状/7 条 T0 + `ask`·`login`·`serve` T1/MCP 面拒绝三条提权命令/about 写清副作用与返回形状；**没勾 `--allow-codex` 时 `ask` 的代码路径里不出现 `codex/responses`（单测守着）**；chat 路径的会话 id 认法（URL 里的 `local-chatgpt%3A…` REST 不认，靠发前快照 + 发后轮询 `/conversations` 并核对最后一条用户消息）、快照读不到就不发；点击发送而非合成 Enter、`Input.insertText` 后等一拍再读按钮 `disabled`；活动分支从 `current_node` 沿 `parent` 回溯且兄弟分支不入结果、工具节点（`recipient != "all"`）与思考摘要默认丢弃、`--include-reasoning` 才收；`limit 0`/空串回退默认值；SSE 事件解析（`output_text.delta` / `completed` / `failed` / 未知事件忽略）；429 `usage_limit_reached` 翻成人话且带恢复时间与 ISO；`usage` 两套额度分开报（codex `/codex/usage` + chat `/wham/usage` 与 `/conversation/init` 的 `limits_progress`）；两种时间戳（列表 ISO 字符串 vs 消息浮点 epoch）都归一；凭证优先级 env → `LCHATGPT_AUTH_FILE` → 仓库 → `~/.codex/auth.json`，token 打码；JWT `scp` 判定 `Send`/`ReadOnly`/`Unknown`（`api.connectors.invoke` 才允许发消息）、`load_for_send` 拒绝只读凭证、`refresh_token` 轮换后写回仓库、`login --from browser` 不降级（能发的凭证被只读的覆盖要 `--force`）、CDP 端点显式指定时不回落；`serve` 只暴露 `lchatgpt:chat`（`--allow-codex` 才多 `lchatgpt:codex`，否则 400 并指路该开关）、模型路由前缀可省、多轮被压平成带角色前缀的一段、codex 路保留逐轮并把 system 折进 instructions、增量 diff 只交新增尾段（多字节安全）、usage 字段名换成 OpenAI 那套；`compose` 的比对折叠空白但不放过多字少字（ProseMirror 会把空行变多），清空输入框要反复验到真的空；env 相关测试走 `main.rs` 的 `testenv` 护栏（串行 + 还原） |
| 端到端 | `lilyco-example/tests/integration.rs` + `examples/multi.rs` | 图片压缩全链路 + 多命令演示 |
| 性能基准 | `lilyco-example/benches/schema.rs` | schema 生成 / 导出 / 校验 / Registry 装配（`cargo bench -p lilyco-example`） |
| aic 域 | `lilyco-aic/src/{pxlslib,unityfs,serialized,atlas,util}.rs` + `{poses,frame,findpose,tex,mpcc,sprites,render,animate,repack,pack}.rs` 底部 | 注册表形状/10 条=8×T0+repack/pack 两条 T1（MCP 拒/CLI 放）/工具导出/缺参拒绝；pxls 磁盘大端（u16 字符串、u32 子流），`serialize` 是 `parse` 的严格逆（PACK/IMGS/POSE 结构化重写、IMGV/PTCL/IMGD/尾部透传，全表逐字节回环测试）；UnityFS version≥7 读完 flags 16 字节对齐、LZMA1 无 size 头补 alone 头（可早停取元数据前缀）、节点表 offset/size u64 切 resS，`repack` 重压分块+重建头/块表/节点表；签名定位容错（真 parse 验候选+长度前缀收边，防把后续 AssetBundle 复制一份）；Unity `TextureFormat` 全表（12=DXT5/BC3、29=DXT5Crunched，曾错位致 71 张图集解成噪点，`format_size_ok` 与编号表测试钉住），块格式先验长度守卫；图集按 asset 名后缀 `_<i>` 配对（`noel_t` 对象表倒序，resolve-once + ordinal 直读，读像素级回归测试）；SerializedFile v22 48B 头（版本串从 @48 起）、typetree blob 节点首 i16 是 version 非 level、CommonString 114 条、MetaFlag 0x4000 对齐、TypelessData 无 Array 包装（3 节点）而 string/vector 有（4 节点）、string len 不含 null，解析时记 `file_size_at`/`obj_table_at` 供写回修补（正文差与对齐后对象差分开记，skew 恒 ±3 内）；noel 真值 232B 对象全字段断言；MPCC 深层 ACC 调色板（`XX.mobpxl.MobPCCContainer`，在 unsafeAssem.dll）：byte占位×2 + string name/chr_name + byte ARMX + ARMX×{pstring key, byte 操作数, ACC×操作}，类型 1=HSV(i16 h, u8 s, u8 v, u8 flags) / 2=TONECURVE(byte 通道数, 每通道 byte 点数 + 点×2B)；三处游戏怪癖必须照抄——s/v 读入后 >=127 再减 256、操作类型 0 丢弃但字节照吃、操作数<=0 的部件直接 continue（`dropped` 如实报出，此时回环必然不成立）；部件表存 Vec 保序（游戏写回走 X.objKeys 字典插入序）；每份文件**逐字节**回环自证 roundtrip_ok（只比 len() 会让同长度不同内容静默通过）|

### 桌面软件图标链路

apps/desktop-common/desktop_icons.py:11 prepare_icon：仅在 Actions 从固定哈希原创 PNG 生成透明 16/24/32/48/64/128/256 ICO 与 PNG；:55 verify_embedded_icon 逐帧核对 EXE/NSIS 的实际 PE 资源。asset-studio 与 vision-studio 的 packaging/build-app.py 传 --icon 并打包 resources，windows-installer.py 配置安装/卸载图标并写独立收据。两个 tests/test_desktop_icons.py 验证透明尺寸、哈希拒绝与仅CI约束；ui/index.html 使用 /assets/app-icon.png，资源随实际 UI 构建保留。

原生窗口用 pywebview.start(icon=...) 的官方路径；启动回调 record_window_icon 将实际 WinForms Icon 保存为原始 ICO 字节并与包内 ICO 比对，验收必须收到 native_icon_applied=true。没有依赖跨线程写 UI 的 shown 回调。

素材工坊真实引擎测试 `apps/asset-studio/tests/test_studio.py:35` 的 `run_job` 用 monotonic 30秒有界轮询并检查HTTP200；shared Windows runner 的24帧512×512 Spine渲染可能超过旧5秒。原始帧差异、GIF透明像素、导出与失败状态断言均保留；生产jobs的120秒限时和取消行为不变。

### 发行边界：现有 office Tauri 桌面端

`lilyco-tauri/src/lib.rs` 注册 `office_commands`、`office_run` 和 `office_messages`。前端通过 WebView 拖放事件或原生对话框获得文件路径，通过 TOML 语言包更新界面。语言默认跟随 WebView 的语言偏好，可显式覆盖。宽窗口并排显示操作区与结果。
`tauri.conf.json` 明确使用原生 `binaries/lbin` sidecar，不是截图或空网页。
办公字段仍由相同 lilyco-binfmt 原CLI读取；安装文件存在不等于全部GUI验收完成。
Desktop workflow仅把 `v[0-9]*` 框架版本标签传给Tauri release上传。
`vision-studio-*` 虽然以v开头，但不属于框架版本，不能再把office bundle挂进Vision Release。
已上传的文件保留，防止破坏他人下载；Source/GUI/icon/install验收另行核实。

Desktop 的 workflow 同时复用 Asset Studio 已验证的完整 `workspace-contract`：真实 `lbin` sidecar、`cargo fmt --all -- --check`、`cargo clippy --locked --workspace --all-targets` 与 `cargo test --locked --workspace`；不以仅覆盖部分 crate 的通用 CI 代替 AGENTS 全 workspace 门。

### Office 0.3.1 · 原始 Lyco 图标及包内验证

`lilyco-tauri/desktop_icons.py` 是 Actions 专用资源配方：原始 PNG 与完整角色许可固定摘要；官方 Tauri CLI 2.12.1 生成 ICNS，共享 `apps/desktop-common/desktop_icons.py` 生成已审查的七尺寸 ICO。前端 favicon/标题图使用同一原图并署名，独立角色许可由 bundle.resources 装入；软件代码许可不变。Office crate/lock/config 版本单独升至 0.3.1，其他 crate 版本不改。
Desktop 的 workspace-contract 与三平台 bundle 在编译前都准备图标；Windows 实际 EXE/NSIS PE 原始图像帧、macOS .app ICNS、Debian PNG 载荷逐字节验证，再保存 icon-build/icon-package-evidence。MSI 目前仅证实生成，并非实际安装/卸载验收；这一步不证明整个 Office 高级 UI 和销售交付已经完成。不使用编译前“文件存在”冒充包内验证，不本机执行转换/测试/构建。

Office 图标包暂只保存 Actions 产物，不在 tauri-action 构建阶段公开上传（平台资源检查发生在其后）。后续仍须真实原生界面、安装/卸载与许可审查，再经独立审核发行；不能先公开再验证。

Office Windows图标门还调用 native-icon.ps1 启动实际冻结程序，按进程归属读取 WM_GETICON / 窗口类 HICON，只回读不设置图标。比较 ICO/同尺寸 PNG 的实际不透明像素与 alpha 轮廓，有限允许 GDI 单级舍入；WM_CLOSE 必须真正退出，保存截图摘要。完整 GUI 与安装器验收仍独立待做。

Windows HWND 出现不代表图标初始化完成，native-icon.ps1 在有限15秒内读取实际BIG/SMALL/SMALL2和class大小图标；仍以原生像素/alpha及正常关闭证明，不设置图标、不用从EXE提取图片冒充窗口图标。966的实际Windows测试曾在只读取BIG/class-big时报无图标，Linux/macOS已通过，须用修复后的新head重新验收。Python宿主的125秒总预算覆盖窗口60秒、图标15秒、关闭30秒及PowerShell初始化；三个阶段仍分别有限截止。

563 的 Windows 真实窗口图标像素和 WM_CLOSE 已通过，后续 NSIS PE 图标核对发现安装器仍使用默认图标，故完整门仍 FAIL。`tauri.conf.json` 明确将 `bundle.windows.nsis.installerIcon/uninstallerIcon` 指向相同七尺寸 ICO，原始 EXE/NSIS PE 多帧核对保持不变；源码回归删除 installerIcon 时必须拒绝，不能把“应用 EXE 有图标”冒充“安装器有图标”。Linux/macOS 563 包载荷已通过，不替代修复后的 Windows 当前产物。

### 商业复用套件与开源 Crate 替换（2026-11）

- **`lilyco-license`**（`lilyco-license/src/lib.rs`）：从 `lilyco-web2cli` 抽离的独立企业授权套件（`PluginDescriptor` / `EnterpriseLicense` / `issue_license` / `verify_license` / `check_plugin_entitlement`），基于 `ring::hmac`（HMAC-SHA256 恒定时间验签），供 `lilyco-web2cli`、`apps/lain42-portal` 等商业应用共享。
- **根 `[workspace.dependencies]`**（`Cargo.toml`）：统一锁定 `axum 0.8` / `tokio` / `serde` / `serde_json` / `thiserror 2` / `ring` / `base64` / `scraper` / `cookie` / `url`。
- **手写轮子替换**：`lilyco-web2cli/src/dom.rs` 已改用 `scraper`（455 行 → 200 行，支持完整 CSS3 选择器）；`lilyco-web2cli/src/session.rs` 的 `Set-Cookie` 解析已改用 `cookie` crate。选型目录见 `docs/CRATE_CATALOG.md`。
- **Web2CLI 响应判定**。`discover_url` 使用 `SessionManager::request_for_discovery` 校验上游 HTTP 状态。`--render` 通过 `CdpClient::navigate_document` 按 frame 和 loader 关联主文档响应。默认 feature `cloudflare` 在 `cloudflare.rs` 集中识别挑战与拦截，并用于过滤 HAR 中的挑战流量。回归覆盖位于 `lilyco-web2cli/tests/e2e.rs` 和 `lilyco-web2cli/src/discover.rs`。
- **Web2CLI 人工交接**。`InspectCmd` 的 `--ego-lite handoff|resume` 调用二进制模块 `ego_lite`，通过 ego-lite 任务空间交接控制权，显式恢复后将符合条件的 HTML 交给 `discover_from_html`。
