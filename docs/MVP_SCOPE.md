# MVP 范围建议：四端齐开（CLI + WebUI + MCP [+ TUI]），按「域」聚合

> 2026-09-19 提案（WorkBuddy 侧整理）。
> 目标：**覆盖普通人用电脑的高频场景**，全部 lilyco 框架化。
> **四端都开**，不分主次 —— 这是 lilyco 存在的意义，砍掉任何一端都是在砍自己的卖点。

## 0. 先纠正一个理解偏差

上一版本写的是「MVP 只编 cli + mcp，Web/TUI/GUI 全砍省资源」——**这是反的，已废弃**。

要的是 **CLI + WebUI + MCP 三端齐开**（TUI 顺带，成本为零，见 §3）。

**为什么四端齐开才是对的**：

| 端 | 谁在用 | 不可替代的理由 |
|---|---|---|
| **CLI** | 脚本、CI、老手、管道 | AI 生成的命令可直接给人复核；`--json-stream` 是机器可读的稳定契约 |
| **WebUI** | **普通人**（这次要覆盖的主体） | 不认识命令行的人占绝大多数；表单 + SSE 实时进度 + 浏览器自动打开 |
| **MCP** | lyco_agent / 任何 agent | agent 只认 MCP；`tools/list` 的 schema 就是模型的工具定义 |
| **TUI** | 终端里的交互场景 | 顺手产出，不额外加域逻辑 |

**关键事实：这四端不共享代码逻辑，只共享 `CommandSchema`。** 所以「四端齐开」的增量成本
**不是四倍工作量**，而是在 Cargo 特性里多挂几个依赖：

```toml
# 每个 app crate 都是这个形状（lffmpeg / lbrush / lgrep 已是）
[features]
default = ["full"]
full    = ["dep:lilyco", "lilyco/full"]   # CLI + TUI + Web + MCP
android = ["dep:lilyco"]                  # 无 crossterm 平台：只剩 CLI + MCP
```

`lilyco` facade 默认 `features = ["tui", "web"]`，即**四端默认全开**。
只有 Android/Termux 这种 crossterm 编不过的目标才降级 `--no-default-features`（CLI + MCP）。

## 1. 四端共用一条执行链（这才是框架化的价值）

```
        ┌── CLI (clap)  ──┐
        ├── TUI (ratatui)─┤
schema ─┤                  ├─→ core::executor ─→ handler ─→ ctx 进度事件 ─→ 各端渲染
        ├── Web (axum/SSE)┤
        └── MCP (stdio) ──┘
```

- **唯一执行宿主**：`core::executor`（AGENTS.md 硬规则 ①）。四端都不自己跑逻辑。
- **唯一校验**：`validate_args`（硬规则 ②）。CLI 有 clap 兜底，但 MCP / Web 直传 JSON，
  安全完全靠它 —— 所以**新校验规则一律先加到 core + 测试**。
- **进度事件统一**：`Progress::{Started, Tick, Log, Telemetry, Done, Error}`。写一遍 `ctx.emit`，
  CLI 打点、TUI 画进度条、Web 走 SSE 推到浏览器、MCP 发 `notifications/progress`。
  → **业务代码里不该出现任何 `if 是哪种前端`**。

## 2. 关键省钱点：**按「域」聚合，不是一条命令一个二进制**

MCP server 是常驻进程，进程数 ≈ 内存 + 启动开销。
`CommandSchema` 原生支持 `subcommands` + `Registry` 多命令，所以**一个二进制可以挂一整个域的命令**，
CLI 是子命令、MCP 一次 `tools/list` 全返回、Web 用 `?cmd=` 下拉切换，三端同源。

→ **每个「域」= 一个二进制 × 四端**，而不是每个命令一个进程。

### 多命令三端入口（已核实 API 存在，无需改 core）

```rust
let mut reg = Registry::new();
reg.register(RegisteredCommand::from_app::<FindFiles>())?;
reg.register(RegisteredCommand::from_app::<Dedup>())?;
// … 同域更多命令

// CLI 子命令
lilyco::run_cli_registry("lfiles", reg.clone());
// Web 控制台（?cmd= 下拉）
lilyco::run_web_registry("lfiles", reg.clone());   // ← 见下方「待补」
// MCP stdio
lilyco::serve_mcp(reg.clone());
// TUI 命令选择页 → 表单
lilyco::run_tui_registry("lfiles", reg);
```

> ⚠️ **已核实的 API 现状**（`D:/Code/lilyco` 本地读码）：
> - `lilyco::run_cli_registry` ✅ 有
> - `lilyco::run_tui_registry` ✅ 有
> - `lilyco::serve_mcp(registry)` ✅ 有
> - `lilyco_gui::GuiRenderer::serve_registry(registry)` ✅ 有，但 **facade 里没有 `run_web_registry` 包装**
>   → 目前只能手写 tokio 块绕过 facade。**这是唯一需要补的 facade 缺口**（约 10 行）：
>
> ```rust
> /// 以 Web 控制台形态运行整个注册表（多命令，?cmd= 切换）
> #[cfg(feature = "web")]
> pub fn run_web_registry(app_name: &str, registry: Registry) {
>     let port = std::env::var("LILYCO_PORT").ok()
>         .and_then(|p| p.parse().ok()).unwrap_or(8080);
>     let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
>     rt.block_on(async {
>         eprintln!("{app_name} Web UI: http://localhost:{port}");
>         lilyco_gui::GuiRenderer::new(port).serve_registry(registry).await;
>     });
> }
> ```
> 同时 `detect_backend` 要加 `--web` / `--gui` 在多命令形态的分发（单命令 `run::<A>()` 已有）。

## 3. MVP 场景划分（普通人 80% 的电脑操作）

| 二进制 | 覆盖场景 | 典型命令 | 复用现状 |
|---|---|---|---|
| **`lfiles`** | 整理/查找/去重/压缩 —— 最高频 | 批量重命名、按类型归类、递归查找、去重、大小统计、zip/tar、sha256 | 新（部分可借 `lilyco-grep` 的查找能力） |
| **`lmedia`** | 视频/音频 | 转码、剪辑、抽帧、转 GIF、抽音轨、媒体信息 | **已有 `lffmpeg`**，并进来 |
| **`limage`** | 照片/PS 类 | 抠图、老照片修复、超分、美颜、格式转换、缩放、加水印 | **已有 `lilyco-brush`** + `lilyco-vision` |
| **`ldoc`** | 文档 | PDF 合并/拆分/提取文本、OCR、Office↔PDF、批量转换 | 部分可用 `lilyco-vision`(OCR) |
| **`lsys`** | 系统/硬件 | 进程、磁盘、内存、网络、环境变量、服务、shell 执行 | 新（`lilyco-plc` 可参考） |

**MVP 先做这 5 个**：覆盖「整理文件 → 处理照片 → 剪视频 → 读文档 → 看系统」，
其中两个已有底子，边际成本最低。
次批：`ltext`（grep/替换/编码）、`lnet`（下载/抓取/API）、`lchat`（文本生成）。

> **为什么 TUI 顺带做**：`lilyco::run_tui_registry` 已存在，多命令选择页也已实现；
> 对每个新 app 来说 TUI 的增量代码 = 0 行。**不做的唯一理由是编不过**（Android/CI），
> 那不是"砍"，是 `--no-default-features` 的降级路径。

## 4. 四端各自的验收标准（都要过）

| 端 | 验收 |
|---|---|
| **CLI** | `lfiles find --pattern '*.jpg' --json` 出结构化 JSON；`--schema` / `--openai-tool` 有输出 |
| **WebUI** | `lfiles --gui` → 自动开浏览器 → `?cmd=` 下拉能切到任意命令 → 提交后有 SSE 进度 → 结果显示 |
| **MCP** | `lfiles --mcp` → `initialize` / `tools/list` 返回带 safety tag 的 schema → `tools/call` 缺参被 `validate_args` 拦下 |
| **TUI** | 交互终端直接 `lfiles` → 命令选择页 → 表单 → 执行有进度条 → `q`/Esc 返回选择页 |

**四端同一份 handler，结果 JSON 必须逐字一致**（这是框架化的验收点）。

## 5. MVP 必须带上、不能省的两件事

1. **安全分级**：删除/覆盖/上传/外呼 一律 `SafetyTier::Confirm`(T1) 以上；
   `tools/list` 已把 tier 写进 description，agent 能看到门槛。**别为省事全标 T0。**
2. **参数不得拼 shell**：MCP / Web 直传参数没有 clap 兜底，安全完全依赖 `validate_args` + 执行层转义。
   → 先做**参数白名单 + 数组式 exec（不经 shell）**。反面教材：`mcp-imagemagick-rce`（CWE-78 命令注入）。

> 注：Web 端 `serve_state` 已带 `security_mw` 中间件且**只绑 127.0.0.1**，这是对的，别改成 0.0.0.0。

## 6. 分发与 agent 侧对齐

- 分发：`cargo binstall`（`lffmpeg` 已验证，见其 `[package.metadata.binstall]`），
  CI 的 release 工作流按 `{name}-{target}{ext}` 命名资产。**四端编在同一份资产里**，不做多版本分发。
- 与 agent 侧对齐：模型只吃 `to_openai_tool()` 形状（= MCP `tools/list` 的等价物），
  **agent 里做一次 MCP→OpenAI 的形状转换即可**，模型不需要知道 MCP 细节。
  已实测：留出工具 schema 泛化 81.2% → **新 CLI 零重训**（见 `lyco_agent/docs/toolcall-contract-2026-09-19.md`）。
- 若要省 prompt 预算：**按任务只挂载相关域的 MCP server**（剪视频只挂 `lmedia`），
  比在 core 里做 Tool-RAG 简单得多 —— **先不要做 RAG**。

## 7. 一句话行动清单

1. 补 facade 的 `run_web_registry`（约 10 行）+ 多命令形态的 `--gui` 分发。
2. 起 `lfiles` 作为第一个「一域一二进制 × 四端」样板，把 `find` / `rename` / `dedup` 三条命令挂上。
3. 四端逐个过 §4 验收表，确保结果 JSON 逐字一致。
4. 样板跑通后，`lmedia` / `limage` 就是把已有 app 并进来，`ldoc` / `lsys` 照抄。
