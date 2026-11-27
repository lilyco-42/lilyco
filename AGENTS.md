# AGENTS.md — AI Agent 指南

Lilyco：一个 struct 派生 CLI / TUI / Web / MCP 四端。Rust workspace，edition 2021。

## 先读这个

**[docs/CODEGRAPH.md](docs/CODEGRAPH.md)** 是代码图谱（唯一导航入口）：
workspace 依赖图、全部关键符号（带 `文件:行号`）、四端调用链、多命令语义对照、
不变量、扩展点、测试地图。**改任何图谱内符号必须同步更新它。**

架构决策与取舍记录：[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 硬性规则

1. `core::executor` 是唯一执行宿主 —— 禁止在后端重写"线程 + 进度消费"循环。
2. `CommandSchema::validate_args` 是唯一参数校验实现 —— 新规则先加 core（带测试），TUI 侧在 `FormField::validate` 映射。
3. 依赖严格单向：core 不依赖后端；后端互不依赖；`lilyco` facade 是唯一组合根。
   - facade `default = []`（headless）：TUI/Web/Ultra 平台重依赖一律显式 `features = [...]`（守则 3）
   - 应用 crate 只依赖 `lilyco`，不直接依赖 `lilyco-core`（core 类型走 `lilyco::__core` 或 prelude）
   - git 依赖必须锁 rev/tag（守则 2，CI hygiene job 强制）
4. `Progress` 事件流恒以 `Done`/`Error` 结尾（executor 合成兜底）。
5. 任何 PR：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets`、`cargo test --workspace` 全绿；新增能力必须带测试。
6. 多命令语义四端对齐：可见/别名/隐藏 —— 改语义时对照 `docs/CODEGRAPH.md` §5 的矩阵，四端一起改。
7. Web / 移动端 / WebView 交付硬性规范（详见 `docs/ui/06-ten-year-ui-architecture.md` §4.1）：
   - 严禁依赖隐式横向滚动导航（尤其禁止父级 `overflow: hidden` 吞掉触摸滚动）；移动端一律用网格/换行或底部 TabBar，确保全部入口一屏可见；
   - 移动端隐藏桌面侧栏时，侧栏内的核心表单/授权/操作项必须在主视图保留可见入口；
   - 移动端输入框字号 `≥ 16px`、触控热区 `≥ 44px`、留出 `env(safe-area-inset-bottom)` 安全区；
   - Android/桌面 WebView 壳必须实现 `onReceivedError` 中文重试/兜底页、`onShowFileChooser` 文件上传与禁用误触缩放；
   - 任何 Web UI 改动必须通过 `320px / 390px / 768px / 1440px` 真实浏览器零横向溢出（`scrollWidth <= innerWidth`）验收。
8. **禁止重复造轮子（`https://lib.rs` 优先）**：开工前必查 `docs/CRATE_CATALOG.md` 与 `https://lib.rs`。成熟 MIT/Apache-2.0 crate 覆盖 ≥ 80% 需求时必须直接复用（HTML 解析用 `scraper`、Cookie 用 `cookie`/`cookie_store`、模板用 `askama`/`maud`、多后端 SQL 用 `sqlx`/`sea-orm`/`turso`、MCP 优先 `rmcp`、OpenAPI 用 `utoipa`）；公共基础 crate 版本统一锁在根 `Cargo.toml` 的 `[workspace.dependencies]`。

## 界面参考

各模块的参考项目、适用场景与许可证见 [Web GUI](lilyco-gui/AGENTS.md)、[CLI](lilyco-cli/AGENTS.md)、[TUI](lilyco-tui/AGENTS.md)、[Tauri 桌面](lilyco-tauri/AGENTS.md)、[原生界面](lilyco-native-ui/AGENTS.md)。

## 常用命令

```bash
cargo test --workspace
cargo run -p lilyco-example --example multi -- ping --name 世界   # 多命令冒烟
cargo bench -p lilyco-example                     # schema 性能基准
cargo run -p lilyco-dev -- new <name>             # 元 CLI：脚手架新域 crate（内嵌模板）
cargo run -p lilyco-dev -- doc [--bin <bin>]      # 元 CLI：生成能力表 CAPABILITIES.md + capabilities.json
```

## 约定

- commit：Conventional Commits（中文描述，如 `feat(lilyco-mcp): ...`）
- 文档语言：README 架构叙述用中文；符号/API 描述保持英文原文
- 版本：各 crate 独立版本号；改动的 crate patch +1，并同步 CODEGRAPH §1 版本表
- 发布：`bash scripts/publish.sh`（依赖序全链；需先 `cargo login --registry crates-io`）
