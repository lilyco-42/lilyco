# Lilyco Manager MVP

独立于主 Cargo workspace 的本地浏览器管理器，首期选择 Axum + SeaORM 2 (SQLx SQLite)。仅回环监听、随机端口，浏览器启动后通过 URL fragment 将用户提供的随机会话 token 传到页面；每次 API 调用携带 x-lilyco-token。SQLite 保存只读工具执行历史。

## 运行
设置强随机环境变量 `LILYCO_MANAGER_TOKEN` 后启动 lilyco-manager。Linux 可自动使用 /dev/urandom；Windows/Linux 默认生成强随机 token，可通过环境变量覆盖。测试或服务模式可设置 `LILYCO_MANAGER_NO_BROWSER=1`。

## 当前范围
已实现两个内置只读工具、发现、调用、历史查看和自动打开浏览器。尚未集成 Lilyco Registry、MCP、通用工具执行，也未实现 Turso / wtx 后端。后续先通过 facade 读出 CommandSchema，再重用 core executor 做真实工具调度，不允许 Web 层直接执行任意外部命令。

## 安全
只监听 127.0.0.1，默认不暴露外网；token 不写入持久化数据库。不要将打印的完整 URL 或 token 外传。浏览器本地页面有意不加载第三方脚本。


## 新版前端

Vue 3 + TypeScript + Vite + Tailwind CSS 4；发布时嵌入 frontend/dist/index.html。开发约束见 AGENTS.md、docs/TECHNICAL_SPEC.md 和 docs/UI_SPEC.md。

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
