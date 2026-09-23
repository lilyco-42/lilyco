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

### 多命令四端入口（`lfiles` 已跑通，可直接照抄）

```rust
// 每个 app crate 的 main.rs 就是这个形状（lilyco-files/src/main.rs 是样板）
pub fn build_registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);   // ← 必须在 register 之前
    for c in [RegisteredCommand::from_app::<find::Find>(),
              RegisteredCommand::from_app::<rename::Rename>(),
              RegisteredCommand::from_app::<dedup::Dedup>(),
              RegisteredCommand::from_app::<stats::Stats>()] {
        reg.register(c).expect("命令名冲突");
    }
    reg
}

pub fn policy_for(backend: lilyco::Backend) -> Arc<dyn SafetyPolicy> {
    match backend {
        lilyco::Backend::Mcp => Arc::new(DenyElevated),  // 自动化面 fail-closed
        _ => Arc::new(Interactive),                      // 人类在环，放行 T1
    }
}

fn main() {
    let backend = lilyco::detect_registry_backend();
    let reg = build_registry_with_policy(policy_for(backend));
    lilyco::run_registry_with("lfiles", reg, backend);   // 四端一行分发
}
```

> **已落地**（`D:/Code/lilyco`，PR #14 后）：
> - `lilyco::run_cli_registry` / `run_tui_registry` / `run_web_registry` / `serve_mcp` ✅ 四个薄包装齐了
> - `run_registry()` / `run_registry_with()` / `detect_registry_backend()` ✅ 一行自动分发
> - **`run_web_registry` 缺口已补**（facade 原先是唯一缺口，需手写 tokio 块绕过）
> - **安全策略按调用面注入已补**：原先 `Registry::with_policy` 在整个框架里**从无调用点** →
>   所有 app 都吃默认 `DenyElevated` → CLI 上手敲 T1 也被拒。现在 `run_registry_with`
>   按后端选策略（MCP→`DenyElevated`，其余→`Interactive`）。详见 `CODEGRAPH.md` §6 不变量 8。
> - `detect_registry_backend()` 的约定：**多命令形态下自动探测出的 TUI 降级为 CLI**
>   （裸跑一个多命令 app 不该被拽进交互界面），进 TUI 要显式 `--tui`。

## 3. MVP 场景划分（普通人 80% 的电脑操作）

| 二进制 | 覆盖场景 | 典型命令 | 复用现状 |
|---|---|---|---|
| **`lfiles`** | 整理/查找/去重/压缩 —— 最高频 | 批量重命名、按类型归类、递归查找、去重、大小统计、zip/tar、sha256 | 新（部分可借 `lilyco-grep` 的查找能力） |
| **`lmedia`** | 视频/音频 | 转码、剪辑、抽帧、转 GIF、抽音轨、媒体信息 | **已有 `lffmpeg`**，并进来 |
| **`limage`** | 照片/PS 类 | 抠图、老照片修复、超分、美颜、格式转换、缩放、加水印 | **已有 `lilyco-brush`** + `lilyco-vision` |
| **`ldoc`** | 文档 | PDF 合并/拆分/提取文本、OCR、Office↔PDF、批量转换 | 部分可用 `lilyco-vision`(OCR) |
| **`lsys`** | 系统/硬件 | 进程、磁盘、内存、网络、环境变量、服务、shell 执行 | 新（`lilyco-plc` 可参考） |
| **`lbin`** | 看文件的结构（逆向/取证/答疑） | 识别魔数并摊开头字段、列压缩包成员、按区上色、读目标文件节表与符号表 | **已有 `lilyco-binfmt`**，4 条命令全 T0 只读 |

**MVP 先做这 5 个**：覆盖「整理文件 → 处理照片 → 剪视频 → 读文档 → 看系统」，
其中两个已有底子，边际成本最低。`lbin` 不在名额里——它全 T0 只读、已在仓库，随时可发。
次批：`ltext`（grep/替换/编码）、`lnet`（下载/抓取/API）、`lchat`（文本生成）。

> **为什么 TUI 顺带做**：`lilyco::run_tui_registry` 已存在，多命令选择页也已实现；
> 对每个新 app 来说 TUI 的增量代码 = 0 行。**不做的唯一理由是编不过**（Android/CI），
> 那不是"砍"，是 `--no-default-features` 的降级路径。

## 4. 四端各自的验收标准（**`lfiles` 已全过**）

| 端 | 验收 | 实测结果（2026-09-19） |
|---|---|---|
| **CLI** | `lfiles find --pattern '*.jpg' --json` 出结构化 JSON；`--schema` / `--openai-tool` 有输出 | ✅ `--help` 列 4 子命令；`find`/`dedup`/`stats` 数值全对；`rename` dry-run 不动盘、`--apply` 真改、冲突中止（`目标已存在…未做任何改动`）、幂等三态（跳过 / `--allow-reapply` 叠加）全通过 |
| **WebUI** | `lfiles --gui` → `?cmd=` 下拉能切到任意命令 → 提交后有 SSE 进度 → 结果显示 | ✅ 首页 132KB；下拉含 4 命令且 `?cmd=` 四命令渲染字节数各异；裸 `POST /run` → **401**（CSRF 令牌中间件生效）；带令牌 → `started→started→tick→done` SSE 全流；**`find` 结果与 CLI `--json` 逐字一致**；`rename`(T1) 在 Web(Interactive) 面放行 |
| **MCP** | `initialize` / `tools/list` 返回带 safety tag 的 schema → `tools/call` 缺参被 `validate_args` 拦下 | ✅ `initialize` 正常；`tools/list` 4 工具（`rename` 带 `[safety: T1]`）；`tools/call find`(T0) 放行返回真实数据；`tools/call rename`(T1) **被安全门拒绝**；缺 `root` → `-32602 参数错误: 缺少必填参数: root` |
| **TUI** | 交互终端 `lfiles --tui` → 命令选择页 → 表单 → 执行有进度条 → `q`/Esc 返回选择页 | ✅ 真 PTY（winpty）实测：选择页渲染 4 命令 + `▶` 光标可移动；Enter 进表单渲染字段（`root (*)` / `min-size` / `ext`）+ CLI 预览 `$ dedup --root qq` + 实时校验（`⚠ 必填参数 root 未填写` → `⚠ 路径不存在`）；Esc/q 干净退出；裸跑自动降级 CLI |

**四端同一份 handler，结果 JSON 必须逐字一致** —— 已在 CLI↔Web 之间用同一 `root` 参数实测比对通过（忽略 `duration_ms`）。

> **2026-09-23 修正一处过时数字**：上表 WebUI 那行的「首页 132KB」是 Layui 内嵌时代的量。
> 控制台换成自研设计系统后重量：`lbin identify` 的 `GET /` = 44,605 B、`lbrush` = 44,051 B
> （内联 CSS + JS，零外链，明暗两套 + 320–1920 全档），其余各行结论不变。

> **`lbin` 的第二轮实测（2026-09-22，`scripts/acceptance/binfmt_probe.py`）：两个真实文件各 27/27 通过**（一个 ELF 目标、一个真实 PE 动态库）。
> CLI 基准 ↔ Web（`--gui` + CSRF + SSE）↔ MCP（stdio `tools/call`）三处结果 JSON 逐字一致（四条命令都比，含 `symbols`）；
> TUI 在 winpty 真 PTY 下选择页列出四条命令、`↓` 移动高亮、`Enter` 渲染表单与 CLI 预览、`Esc`/`q` 干净退出；
> 裸跑（不带 `--tui`）仍降级 CLI。
> 这一轮抓到两处**只在非 CLI 端现形**的缺陷：`limit` / `max-bytes` 在 Web/MCP 省略时传进来是 0，
> 当时被当成「只列一条」/「只读一千字节」，于是四端给出长短不一的同一张表、报错还怪文件——
> 现在 0 一律按文档缺省处理（`docs/binfmt.md` 的注意事项里有明文）。

> TUI 的「选择页→表单→进度→回选择页」状态机另有 6 个 facade 单测（`build_multi_tui`
> 的隐藏命令过滤 / 空注册表报错 / 高亮驱动）+ 29 个 `lilyco-tui` 单测覆盖；
> 真 TTY 下验证的是"确实渲染出来了、确实能进能退"。

## 5. MVP 必须带上、不能省的两件事

1. **安全分级**：删除/覆盖/上传/外呼 一律 `SafetyTier::Confirm`(T1) 以上；
   `tools/list` 已把 tier 写进 description，agent 能看到门槛。**别为省事全标 T0。**
   `lfiles` 的 `rename` 已按此定为 T1 —— 副产物是它**逼出了框架级缺陷**：
   `Registry::with_policy` 原先无调用点，导致 CLI 上敲 T1 也被拒（见 §2 已落地清单）。
2. **参数不得拼 shell**：MCP / Web 直传参数没有 clap 兜底，安全完全依赖 `validate_args` + 执行层转义。
   → 先做**参数白名单 + 数组式 exec（不经 shell）**。反面教材：`mcp-imagemagick-rce`（CWE-78 命令注入）。
   `lfiles` 全命令只用 `std::fs`，无 `Command::new`，天然免疫此类问题。

> 注：Web 端 `serve_state` 已带 `security_mw` 中间件且**只绑 127.0.0.1**，这是对的，别改成 0.0.0.0。
> 实测确认：中间件对 `POST /run` 同时校验回环 `Origin` + `X-Lilyco-Token`（令牌写在首页
> `<meta name="lilyco-token">`，前端 JS 读取后随 fetch 带上）。

## 6. 分发与 agent 侧对齐

- 分发：`cargo binstall` 的 metadata 已写在 `[package.metadata.binstall]`，但**尚未达到可用**：
  `{ name }` 是包名（`lilyco-ffmpeg`），CI 的 release 工作流却按二进制名（`lffmpeg-…`）命名资产，
  URL 对不上 → 现在只有源码安装这条路（修法见 `docs/INTEGRATION.md` §0）。**四端编在同一份资产里**，不做多版本分发。
- 与 agent 侧对齐：模型只吃 `to_openai_tool()` 形状（= MCP `tools/list` 的等价物），
  **agent 里做一次 MCP→OpenAI 的形状转换即可**，模型不需要知道 MCP 细节。
  已实测：留出工具 schema 泛化 81.2% → **新 CLI 零重训**（见 `lyco_agent/docs/toolcall-contract-2026-09-19.md`）。
- 若要省 prompt 预算：**按任务只挂载相关域的 MCP server**（剪视频只挂 `lmedia`），
  比在 core 里做 Tool-RAG 简单得多 —— **先不要做 RAG**。

## 7. 一句话行动清单

1. ~~补 facade 的 `run_web_registry`~~ ✅ 已完成（含 `run_registry` / `run_registry_with` /
   `detect_registry_backend`，以及**按调用面注入 SafetyPolicy**）。
2. ~~起 `lfiles` 作为第一个「一域一二进制 × 四端」样板~~ ✅ 已完成：`find` / `rename` /
   `dedup` / `stats` 四命令挂一表，**69 个单测**，`docs/lfiles.md` 为样板文档。
3. ~~四端逐个过 §4 验收表，确保结果 JSON 逐字一致~~ ✅ 已完成（见上表）。
4. 样板已跑通 → 下一步把 `lmedia`(已有 `lffmpeg`) / `limage`(已有 `lilyco-brush`+`lilyco-vision`)
   并进来，`ldoc` / `lsys` 照抄 `lilyco-files` 的结构（`main.rs` 的 registry 装配 + 每命令一文件）。
