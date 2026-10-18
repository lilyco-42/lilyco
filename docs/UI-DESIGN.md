# UI 设计与渲染后端决策（tiny_lilyco / Tiny Agent）

> 目标：**shadcn 式高级感**（克制、灰白黑为主、细描边、低饱和），默认主题 **Tokyo Night**，
> 并能在 Windows / Linux / macOS 上跑起来，且与 **UniDesktop/SDK (UDA)** 直接互通。

---

## 1. 渲染后端决策：GPUI + UDA

| 维度 | **GPUI + gpui-kit + UDA** | Tauri v2 + UDA | Electron + UDA |
| :--- | :--- | :--- | :--- |
| 语言一致性 | ✅ 全 Rust，单工具链 | ⚠️ Rust 壳 + TS/React | ❌ Rust + Node + Chromium |
| 调 UDA | ✅ 同进程直接调用，零序列化 | ⚠️ 需 IPC 命令转发 | ❌ 需 native addon 或 sidecar |
| 启动/包体 | ✅ 原生 GPU 渲染，无 webview 运行时 | ⚠️ 系统 webview，包体中等 | ❌ 自带 Chromium，量级最大 |
| shadcn 还原度 | ✅ `gpui-kit` 已提供 75+ 成品组件与主题系统 | ✅ 原生 React/Tailwind，最省力 | ✅ 同左 |
| Windows 支持 | ✅ 已确认（`cfg(target_os="windows")` 依赖 + `windows-manifest` feature，Zed 官方提供 Windows 构建） | ✅ 成熟 | ✅ 成熟 |
| 与 `no_std` 内核的边界 | ✅ 内核零依赖，GPUI 只是 std 外壳 | ✅ 同 | ⚠️ 同，但多一层 Node |
| CI 成本 | ⚠️ 需 GPU/显示相关系统库，构建较重 | ✅ 较轻 | ⚠️ 需 Node + 打包链 |

**结论：GPUI + gpui-kit + UDA 为主线**，理由排序：

1. **一条语言边界**：agent 内核（`tiny_lilyco`）、工具、UDA 调用、UI 全在 Rust 内，异常与类型不会跨 IPC 丢失。
2. **UDA 是 Rust crate**，跨 IPC 调用它等于把"能力探测 + 优雅退化"重新做一遍，得不偿失。
3. **包体与启动**符合 Catime 式的极简取向；Electron 直接排除。
4. shadcn 的"高级感"本质是**令牌 + 规则**（灰阶、1px 描边、8px 圆角、克制动效），而非 React 本身；`gpui-kit` 已经把组件层补齐。

**保留的退路**：`tiny_lilyco::ui` 的令牌与 `Theme::emit_css()` 让"同一套设计"能直接投到
webview。若某天需要 Web 分发或复杂 Markdown/表格渲染，**Tauri v2 + 同一套 CSS 变量**即可切换，
不需要改设计系统。

---

## 2. 设计语言规则（shadcn 高级感的可执行定义）

1. **灰阶承担 90% 的视觉，颜色只做信号**。主色仅用于：主按钮、焦点环、选中态、状态点。
2. **描边而非阴影**。1px `--border`；仅浮层（popover/dialog）用低透明度阴影。
3. **低饱和 + 高对比文本**。正文对比度目标 ≥ 7:1，次要文本 ≥ 4.5:1。
4. **圆角按 `apps/*` 既有尺度**：徽章 4px（sm）、控件 8px（md）、卡片 12px（lg）、面板 16px（xl）、头像全圆。
5. **留白优先**：容器内边距 16/24，列表行高 ≥ 36，绝不挤。
6. **动效只做状态反馈**：120ms 悬停、180ms 展开、240ms 面板；不做装饰性动画。
7. **焦点永远可见**：2px ring + 2px offset，键盘可达。
8. **暗色不是反色**：暗色主题的"抬高"靠 `background < card < popover` 的层级，不靠阴影。

---

## 3. 令牌（唯一来源：`tiny_lilyco::ui`）

### 3.1 语义颜色（28 个角色，命名对齐 shadcn）

`background / foreground / card / card-foreground / popover / popover-foreground /
primary / primary-foreground / secondary / secondary-foreground / muted / muted-foreground /
accent / accent-foreground / destructive / destructive-foreground / border / input / ring /
sidebar / sidebar-foreground / sidebar-accent / sidebar-border / success / warning / info /
selection / code-bg`

### 3.2 三套主题

| id | 定位 | background | foreground | primary | border |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `tokyo-night` **默认** | 夜间主力 | `#1a1b26` | `#c0caf5` | `#7aa2f7` | `#292e42` |
| `neutral-light` | 白天 / 打印感 | `#ffffff` | `#09090b` | `#18181b` | `#e4e4e7` |
| `neutral-dark` | 纯石墨 | `#09090b` | `#fafafa` | `#fafafa` | `#27272a` |

Tokyo Night 完整映射（与上游配色一致）：

| 角色 | 值 | 角色 | 值 |
| :--- | :--- | :--- | :--- |
| background | `#1a1b26` | foreground | `#c0caf5` |
| card | `#1f2335` | popover | `#292e42` |
| primary | `#7aa2f7` | primary-foreground | `#1a1b26` |
| secondary / accent / border | `#292e42` | muted | `#24283b` |
| muted-foreground | `#565f89` | destructive | `#f7768e` |
| sidebar | `#16161e` | sidebar-foreground | `#a9b1d6` |
| success | `#9ece6a` | warning | `#e0af68` |
| info | `#7dcfff` | selection | `#33467c` |
| code-bg | `#16161e` | ring | `#7aa2f7` |

### 3.3 尺度

* **间距**：4 / 8 / 12 / 16 / 24 / 32 / 48（4px 网格）
* **圆角**：4 / 8 / 12 / 16 / full（sm / md / lg / xl，控件 8、卡片 12）
* **字阶**：11 / 12 / 13 / 14 / 16 / 20 / 24，行高 16 / 18 / 20 / 22 / 24 / 28 / 32
* **字重**：400 / 500 / 600
* **描边**：1px
* **焦点环**：2px + 2px offset
* **动效**：120 / 180 / 240 ms

---

## 4. Agent 界面骨架

```
┌───────────────┬──────────────────────────────────────────────┐
│  Sidebar      │  Header: 会话标题 · 模型 · 主题切换 · UDA 状态 │
│  (240px)      ├──────────────────────────────────────────────┤
│  + 新会话     │  Trace（OODA 流）                             │
│  ▸ 会话列表   │   step 3 │ act │ edit_file   → diff 卡片       │
│  ▸ 项目       │   step 4 │ verify │ cargo test → exit 0 徽章  │
│               ├──────────────────────────────────────────────┤
│  设置 / 主题  │  Composer：输入框 + 附件(图片) + 运行         │
└───────────────┴──────────────────────────────────────────────┘
```

* **Trace 是主角**：每次工具调用一行，左侧状态点（success/warning/destructive），
  点开是 `Observation`（text / diff / console / image / exit）。
* **状态由数据推导，不由调用方传入**：`TraceRow::status()` 只看
  `RowBody`——`exit 0` 才是 Ok，`exit != 0` 是 Fail，`stderr` 是 Warn。
  三种前端渲染同一行，不会各自解释一遍"什么算失败"。
* **颜色也从令牌来**：`TraceRow::status_colors(&palette)` 直接返回
  `(背景, 前景)`，GPUI 用 `Color`，webview 用 `emit_css()` 的同名变量。
* **Diff 卡片**：等宽字体，`+`/`-` 行用 success/destructive 的 20% 透明底，不使用饱和色块。
* **图片预览**：`image_inspect` 嗅探 → 缩放到 1568 边 → 内联缩略图 + `png 1024x768` 元信息。
* **UDA 状态**：Header 右侧一个徽章，显示 `theme: dark`、可用的桌面能力数，
  降级时显示 `Partial(reason)` 的 tooltip。

---

## 5. 组件映射（shadcn → gpui-kit）

| shadcn | gpui-kit | 本项目用途 |
| :--- | :--- | :--- |
| Button / IconButton | Button | 运行、停止、撤销、重启 |
| Input / Textarea | Input / Textarea | Composer、搜索 |
| Card | Card | Diff 卡片、工具结果 |
| Badge | Badge | exit code、状态、能力等级 |
| Tabs | Tab / TabBar | Trace / Diff / Preview 切换 |
| Dialog | Modal | 确认危险命令 |
| Sidebar | Sidebar / List | 会话列表 |
| ScrollArea | Scrollable | Trace 流 |
| Tooltip | Tooltip | UDA 降级原因 |
| Toast | Notification | 后台任务完成 |

> 版本以 `gpui-kit` 当前 release 为准；`tiny_lilyco::ui` 只提供令牌，不绑定组件 API。

---

## 6. 最小 demo 验收标准

1. 一个窗口，左侧栏 + Trace + Composer 三段式，Tokyo Night 配色正确。
2. Header 能切换三套主题，颜色立即生效（令牌驱动，非硬编码）。
3. 启动时调用 UDA 探测：显示当前主题（dark/light）与"可用能力"计数；
   探测失败必须优雅降级为 `unknown`，不 panic。
4. Trace 能渲染一条 `edit_file` 的 diff 卡片与一条 `exit 0` 徽章。
5. 图片预览能显示一张 PNG 的缩略图与 `png WxH`。
6. 全程无 `unsafe`，`tiny_lilyco` 内核保持 `no_std`。

---

## 7. 与既有 `apps/*` 的一致性

仓库里 `apps/asset-studio` 与 `apps/vision-studio` 已经使用 **shadcn + Tailwind v4 + OKLCH 变量**
（见 `apps/asset-studio/ui/src/studio-theme.css`）。本设计系统与它们的关系：

| 项 | 既有 apps | `tiny_lilyco::ui` | 处理 |
| :--- | :--- | :--- | :--- |
| 变量名 | `--background` `--card` `--border` … | 同名同义（28 个角色） | ✅ 完全对齐 |
| 颜色空间 | OKLCH | sRGB `#rrggbb` | ⚠️ 并存：CSS 变量接受任意颜色语法，Tailwind 映射不挑空间；Rust/GPUI 侧必须是 u32 |
| 主题切换 | `.dark` 类 | `Theme` 枚举 + `--theme-kind` | ⚠️ 两个壳各自切换，变量名一致 |
| 圆角 | `md=.5rem` `lg=.75rem` `xl=1rem` | `md=8` `lg=12` `xl=16` | ✅ 已对齐 |
| 额外令牌 | `--checker`（透明棋盘） | 无 | 属于素材编辑器专用，agent 壳不需要 |

**原则**：`tiny_lilyco::ui` 只做"共享词汇表"，不接管既有 apps 的 Tailwind 构建。
未来若把 agent 壳做成 webview，直接消费 `Theme::emit_css()` 的输出即可与 apps 视觉统一。
