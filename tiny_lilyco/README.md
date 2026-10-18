# tiny_lilyco — 极简 AI 编程工具内核

`no_std`、零堆分配、可跑在 MCU 与浏览器 WASM 里的 **agent 内核**。
同一个内核驱动 CLI / 桌面壳 / Web / MCP 四端，渲染后端（GPUI-kit、egui、imgui、
Tauri、Webview、EUI-NEO）全部通过 trait 解耦，任意替换。

## 第一性原理

AI 编程工具不是"代码生成器"，而是**带环境感知与撤销保护的闭环反馈引擎**：

```
观察 Observe → 判断 Orient → 决策 Decide → 执行 Act → 验证 Verify →（回到观察）
```

由此推出四个不可替代的底层抽象：

| 抽象 | 为什么必须有 | 本项目落点 |
| :--- | :--- | :--- |
| **事务保护** | LLM 必然出错；只有可逆，才敢让它连续试错 | `adapters::vcs_guard`（`jj` 优先，`git` 兜底） |
| **虚拟桌面 / 应用内浏览器** | 终端日志看不到 DOM、样式、渲染失败 | `adapters::browser_control`（CDP 帧构造器） |
| **精确文件读写** | 全量覆写既贵又容易丢代码 | `adapters::fs_engine`（行窗口 + 唯一锚点替换） |
| **多模态图像感知** | 设计稿、截图、SVG 都是编程输入 | `adapters::image_inspect`（免解码嗅探 + 缩放规划） |

## 模块地图

```
src/
├── agent/
│   ├── tool.rs        Tool trait / ToolSpec / ToolError / ObsKind
│   ├── registry.rs    定容工具注册表（16 个槽，重复名拒绝）
│   └── ooda.rs        OODA 状态机 —— 强制"改文件前必须打 checkpoint"
├── adapters/
│   ├── fs_engine.rs        行窗口、路径越权校验、唯一锚点替换、行号渲染
│   ├── vcs_guard.rs        jj / git 检查点、撤销、日志、丢弃（纯规划）
│   ├── browser_control.rs  CDP 帧：navigate / evaluate / screenshot / DOM
│   ├── image_inspect.rs    PNG/JPEG/GIF/WebP/BMP/SVG 嗅探 + 尺寸 + 缩放
│   ├── host.rs             （feature = "std"）唯一能 spawn 进程的地方
│   ├── uda.rs              UniDesktop/SDK 桌面能力桥
│   ├── mcp_uda.rs          Agent 工具 ↔ CLI 统一 schema
│   └── gui.rs              GuiBackend trait（任意渲染后端）
└── core/                    schema / progress / executor
```

## 关键设计：为什么"检查点"在状态机里，而不是在工具里

`Machine::begin_act()` 会对 `mutating = true` 的工具返回
`ActPermit::CheckpointFirst`，并且在 `checkpointed()` 之前拒绝 `finish_act()`。
于是"先快照、再修改"是**类型系统强制的**，而不是调用方的自觉 —— 这是
"AI 改坏了能一键回滚"的工程保证，也对应 `jj undo` / `git reset --hard` 的语义。

## 嵌套预算

全仓库强制 **函数体内嵌套 ≤ 4 层、缩进 ≤ 20 列**，由
`scripts/check_nesting.py` 在 CI 中把关。

## 验证方式

**本地不编译、不运行**。所有构建与测试由 GitHub Actions 远端完成
（`.github/workflows/tiny_lilyco.yml`）：

* `thumbv7em-none-eabi`（裸机 ARM Cortex-M4）与 `wasm32-unknown-unknown` 交叉检查
* `cargo clippy -- -D warnings`（`no_std` 与 `std` 两套）
* `cargo test`（`no_std` 核心 + host 桥）
* 嵌套预算检查
