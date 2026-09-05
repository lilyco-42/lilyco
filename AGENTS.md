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
4. `Progress` 事件流恒以 `Done`/`Error` 结尾（executor 合成兜底）。
5. 任何 PR：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets`、`cargo test --workspace` 全绿；新增能力必须带测试。
6. 多命令语义四端对齐：可见/别名/隐藏 —— 改语义时对照 `docs/CODEGRAPH.md` §5 的矩阵，四端一起改。

## 常用命令

```bash
cargo test --workspace
cargo run -p lilyco-example --example multi -- ping --name 世界   # 多命令冒烟
cargo bench -p lilyco-example                     # schema 性能基准
```

## 约定

- commit：Conventional Commits（中文描述，如 `feat(lilyco-mcp): ...`）
- 文档语言：README 架构叙述用中文；符号/API 描述保持英文原文
- 版本：各 crate 独立版本号；改动的 crate patch +1，并同步 CODEGRAPH §1 版本表
