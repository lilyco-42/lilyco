# 04 · 令牌参考

> 一页查完所有可用的颜色、间距、圆角、字号。
> 数值抄录自 `lilyco-gui/assets/app.css`；**冲突时以 `app.css` 与
> `lilyco-gui/DESIGN.md` 为准**，本文只做查阅与解释。

---

## 1. 语义颜色（明 / 暗）

| 令牌 | 浅色 | 深色 | 用在哪 | 对比度（明 / 暗） |
|---|---|---|---|---|
| `--canvas` | `#f6f7f9` | `#0d1014` | 页面底色 | — |
| `--surface` | `#ffffff` | `#151a20` | 卡片、输入框底 | — |
| `--surface-2` | `#f1f3f5` | `#1b2128` | 次级面板、chip、CLI 预览底、结果区 | — |
| `--surface-3` | `#e7eaee` | `#232b34` | 进度条槽、按下态 | — |
| `--hairline` | `#e0e3e8` | `#28313b` | 卡片/面板装饰边 | ≈1.3（**故意不过 3:1**） |
| `--control` | `#7d8794` | `#5f6d7c` | 输入框、按钮、下拉、拖拽虚线 | 3.64 / 3.30（≥3:1） |
| `--ink` | `#16191d` | `#e7eaee` | 主文本 | 17.63 / 14.50 |
| `--ink-muted` | `#5f6b7a` | `#9aa6b4` | 次文本、说明、页脚 | 5.43 / 7.07 |
| `--accent` | `#2563eb` | `#6ea1ff` | 强调（六处用途，见下） | 5.17 / 6.83 |
| `--on-accent` | `#ffffff` | `#0b1220` | 强调底上的字 | 5.17 / 7.32 |
| `--ok` | `#0e7a55` | `#3ddc97` | 就绪 / 成功 | 5.34 / 未实测 |
| `--warn` | `#8a5a06` | `#f0b23a` | 进行中 / 提醒 | 5.92 / 未实测 |
| `--danger` | `#b3261e` | `#ff7b72` | 错误、必填星号、危险 hover | 6.54 / 6.94 |
| `--accent-soft` | `rgba(37,99,235,.10)` | `rgba(110,161,255,.16)` | 焦点环扩散、hover 底色、选中背景 | — |
| `--shadow-card` | `0 1px 2px` + `0 4px 16px`（≤.06） | `none` | 全页唯一层次阴影 | — |

对比度全部是 2026-09-23 在**跑起来的最终页面**上用 `getComputedStyle` 取色后按 WCAG 公式算的，
不是手算静态色值。改令牌后要重算，别沿用这张表。

### 1.1 `--accent` 的六个合法用途

1. 品牌标记 `◆`
2. 当前命令（标题、下拉选中）
3. 焦点环（`outline` / 扩散环）
4. 主按钮底色
5. 进度条确定态填充
6. 链接、CLI 预览的 `$` 前缀

出现第七个用途就是强调失效。

### 1.2 `--control` 与 `--hairline` 不能互换

- 控件边界（输入框、按钮、下拉、拖拽区）用 `--control`，必须 ≥3:1；
- 容器装饰边界（卡片、面板）用 `--hairline`，故意很浅。

把 hairline 调深不会「更清楚」，只会让卡片边抢内容。

---

## 2. 终端固定色板

终端是「机器的输出」，**不随主题变**，避免浅色主题下像一块贴歪的纸。
这一组是全页唯一允许的十六进制字面量（不走主题令牌）：

| 用途 | 值 |
|---|---|
| 终端底 | `#0d1117` |
| 终端字 | `#e6edf3`（对底 16.02:1） |
| 错误行 | `#ff7b72` |
| 警告行 | `#e3b341` |
| 遥测行 | `#56d364` |
| 空态占位 | `#8b949e` |
| 滚动条滑块 | `#30363d` |

结果区**不是**终端：它用 `--surface-2` + `--ink`，跟随主题。

---

## 3. 间距

| 令牌 | 值 | 常见用途 |
|---|---|---|
| `--s-1` | 4px | 图标与文字、chip 内边距 |
| `--s-2` | 8px | 字段与标签、按钮间距、行间距 |
| `--s-3` | 12px | 控件内边距、面板内边距 |
| `--s-4` | 16px | 版心留白、卡片内边距（窄屏） |
| `--s-5` | 24px | 卡片内边距、区块间距 |
| `--s-6` | 32px | 宽屏卡片内边距 |
| `--s-7` | 48px | 页底留白 |

出现 10 / 14 / 18 / 20px 均为漂移。

---

## 4. 圆角与高度

| 令牌 / 类 | 值 | 用在哪 |
|---|---|---|
| `--r-control` | 8px | 输入框、按钮、下拉、CLI 预览 |
| `--r-panel` | 12px | 卡片、终端、结果区、拖拽区 |
| `--r-pill` | 999px | chip、进度条 |
| 滚动条滑块 | 4px | 非界面元素，唯一例外 |
| `.btn` / `input` / `select` | 40px 高 | 控件统一高度 |
| `.icon-btn` | 34×34px | 顶栏主题按钮 |
| `.btn-icon` | 28px 高 | 选择文件 / 添加一项 / 复制 |

---

## 5. 字体与字号

| 令牌 | 值 |
|---|---|
| `--ui` | `system-ui, -apple-system, "Segoe UI", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif` |
| `--mono` | `ui-monospace, "SF Mono", "Cascadia Code", Consolas, "Noto Sans Mono CJK SC", monospace` |

| 角色 | px | weight | line-height | tracking |
|---|---|---|---|---|
| title（命令名） | 22 | 650 | 1.25 | -0.2px |
| about（命令说明） | 14 | 400 | 1.7 | 0 |
| label（字段标签） | 13 | 600 | 1.4 | 0 |
| body（正文基准） | 15 | 400 | 1.6 | 0 |
| meta（状态、chip、页脚） | 12.5 | 400 | 1.5 | 0 |
| mono-data（预览、结果、日志） | 13 | 400 | 1.55 | 0 |
| overline（「输出」「结果」） | 11.5 | 600 | 1.4 | 0.8px，大写 |

视口 ≤720px 时输入框字号提到 16px（防 iOS 聚焦缩放）。

---

## 6. 断点

| 层 | 条件 | 变化 |
|---|---|---|
| 视口 | ≤420px | 页面留白收窄、卡片内边距 16、副标题隐藏、标题 20px |
| 视口 | ≤720px | 输入框字号 16px |
| 视口 | ≥721px | 卡片内边距 32 |
| 视口 | ≥1200px | 卡片最大宽 1040、结果区限高 |
| 容器 | ≤560px | 动作行可换行、按钮 140 基准居中 |
| 容器 | ≤420px | 动作行竖排占满、输出头换行、命令下拉换行 |
| 容器 | ≥721px | 动作行不换行、chip 靠右限 45% 宽 |

容器指 `.card` `.field` `.actions` `.out-head` `.topbar`（`container-type: inline-size`）。

---

## 7. 动效

| 用途 | 时长 | 属性 |
|---|---|---|
| hover / active | 150ms | `border-color`、`background`、`transform: scale(.98)` |
| 状态切换 | 200ms | `background`、`color`、进度条 `width` |
| 进场 | 300ms | `opacity` + `translateY(6px)` |

`@keyframes` 只有 `rise` / `spin` / `indet` 三个。
`prefers-reduced-motion: reduce` 下全部停掉，且必须留静态状态信号（见 [03 §7](03-design-language.md)）。

---

## 8. 改令牌的正确顺序

```text
1. lilyco-gui/DESIGN.md   ← 先改表（数值、理由、实测记录）
2. assets/app.css         ← 再改 :root 与 [data-theme="dark"]
3. cargo test -p lilyco-gui
4. 浏览器实测（明/暗 × 320–1920）
```

第 3 步会跑这几条与令牌直接相关的闸门：

- `every_css_color_is_in_the_design_table`：CSS 里每个色值必须能在 DESIGN.md 找到；
- `assets_stay_within_the_documented_budget`：资源字节上限；
- `reduced_motion_keeps_a_non_animated_state_signal`：reduce 下的静态替代别被删。

第 4 步是必须的：`cargo test` 看不见对比度，也看不见 360px 容器里的挤压。

---

## 9. 与 shadcn 令牌的对应

两套命名不同但角色可对齐。详细映射与换算见 [05 · React 映射](05-react-shadcn-mapping.md)。

| lilyco | shadcn/ui | 注意 |
|---|---|---|
| `--canvas` | `--background` | — |
| `--surface` | `--card` / `--popover` | shadcn 把浮层与卡片分开，lilyco 只有一层 |
| `--surface-2` | `--muted` / `--secondary` | — |
| `--ink` | `--foreground` | — |
| `--ink-muted` | `--muted-foreground` | — |
| `--accent` | `--primary` | lilyco 的 accent 是**主色**，不是 shadcn 的 hover 底色 |
| `--on-accent` | `--primary-foreground` | — |
| `--ok` / `--warn` / `--danger` | `--success` / `--warning` / `--destructive` | shadcn 默认没有 success/warning，需自定义 |
| `--control` | `--input` / `--border` | shadcn 的 border 通常比 input 浅，别直接等价 |
| `--hairline` | `--border` | 用途一致：装饰性分隔 |

---

改令牌的完整流程（改表 → 改 CSS → 跑测试 → 浏览器量）：见
[第 5 章 · 让页面像产品](tutorial/05-make-it-product.md)。