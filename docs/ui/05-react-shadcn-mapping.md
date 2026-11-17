# 05 · React + shadcn 映射

> 适用场景：页面不在 `--gui` 里，而是独立前端（`kuake-studio` / `lyblive` 这类 bun + TS + shadcn 项目）。
> 目标不是让两套代码合并，而是让**视觉语言连续**：同一个产品，换一面墙，气质不变。

---

## 1. 先决定用哪套

| 维度 | 内置 `--gui`（lilyco-gui） | 独立 React + shadcn |
|---|---|---|
| 交付 | 单页内嵌进二进制 | 独立前端产物 + 本地 HTTP 后端 |
| 依赖 | 零外部请求（CSP `default-src 'none'`） | React / Tailwind / Radix / 图标库 |
| 表单来源 | schema 自动派生 | 手写，或按 schema 生成（如 `schema-form.tsx`） |
| 适合 | 一个命令一张表单、进度 + 日志 + JSON | 多页面、文件列表、对话框、复杂交互 |
| 不适合 | 路由、持久化视图、富文本、图表 | 需要零依赖 / 要嵌进单个 `.exe` 的场景 |

判断标准：**页面主体是「一次命令的输入输出」→ 用内置端；是「一个持续使用的应用」→ 用 React。**

两者可以并存：同一个 Rust 后端既 `serve_app`（内置页）也提供 HTTP API 给自建前端。
但**不要**试图把 shadcn 的 React 产物塞进 `lilyco-gui`：CSP 不允许外链脚本，
引 React 等于打破零依赖这条硬约束。

---

## 2. 令牌映射

### 2.1 语义角色

| lilyco（十六进制） | shadcn（oklch 变量） | 说明 |
|---|---|---|
| `--canvas` `#f6f7f9` / `#0d1014` | `--background` | 页面底 |
| `--surface` `#ffffff` / `#151a20` | `--card` | 卡片、输入框底 |
| `--surface-2` `#f1f3f5` / `#1b2128` | `--muted`、`--secondary` | 次级面板、chip、代码底 |
| `--surface-3` `#e7eaee` / `#232b34` | 无直接对应 | 进度槽、按下态，可在 Tailwind 里自建 |
| `--hairline` `#e0e3e8` / `#28313b` | `--border` | 装饰性分隔 |
| `--control` `#7d8794` / `#5f6d7c` | `--input` | 控件描边（shadcn 的 `--border` 通常更浅，别混用） |
| `--ink` `#16191d` / `#e7eaee` | `--foreground` | 主文本 |
| `--ink-muted` `#5f6b7a` / `#9aa6b4` | `--muted-foreground` | 次文本 |
| `--accent` `#2563eb` / `#6ea1ff` | `--primary` | **注意语义差**：lilyco 的 accent 是主色 |
| `--on-accent` | `--primary-foreground` | — |
| `--ok` / `--warn` / `--danger` | `--success` / `--warning` / `--destructive` | shadcn 默认只有 destructive，另两个自定义 |
| `--ring` | `--ring` | 焦点环；lilyco 用 `outline`，shadcn 用 ring |
| `--r-control`（8px） | `--radius` | shadcn 默认 `0.625rem`(10px) 或 `0.65rem`，lilyco 锁 8px |

### 2.2 换算思路（不是逐值等价）

lilyco 写十六进制，shadcn 常写 oklch。两者**不要求数值相等**，要求：

1. 同一角色的**明暗顺序**一致（canvas < surface < surface-2）；
2. 同一角色的**对比度档位**一致（正文 ≥7:1、次要 ≥4.5:1、控件边 ≥3:1）；
3. 强调色只有一种，且用途集合一致（六处，见 [04 §1.1](04-tokens.md)）。

换算步骤：

```text
1. 取 lilyco 的十六进制 → 转 oklch（浏览器 DevTools 或 colorjs.io）
2. 按上表落到 shadcn 变量名
3. 用同一套对比度脚本复核（不要凭肉眼）
4. 暗色不是反相：canvas/surface/surface-2 的色阶要重新挑，不能直接反转
```

`kuake-studio` 的 `frontend/src/index.css` 就是这套做法的一个活例：
它的 `--background / --foreground / --card / --primary …` 已经对齐 shadcn 命名，
暗色为主（长时间看文件列表更省眼），品牌蓝偏青以避免和 shadcn 默认紫撞气质。

---

## 3. 组件映射

| lilyco 组件 | shadcn/ui | 迁移要点 |
|---|---|---|
| `.btn.primary`（运行） | `Button`（default） | 高度对齐 40px；不要用 `size="lg"` |
| `.btn.ghost`（复制 CLI） | `Button variant="outline"` | lilyco 的 ghost 有描边，不是 shadcn 的无边框 ghost |
| `.btn.danger`（取消） | `Button variant="outline"` + destructive 色 | 取消是次动作，不要用实底 destructive |
| `.btn-icon`（选择文件 / 添加） | `Button variant="outline" size="sm"` | 28px 高；文字按钮优先于纯图标 |
| `input[type=text]` | `Input` | 40px 高、`line-height: 1.4`、label 必须绑定 |
| `input[type=number]` + `.field-hint` | `Input type="number"` + 说明文字 | 区间既进属性也要可见；用 `aria-describedby` |
| `select` | `Select`（Radix） | 原生 select 更轻；只在需要分组/搜索时换 Radix |
| `.flag-row` + checkbox | `Checkbox` + `Label` | 整行可点，保持原生语义 |
| `.dropzone` | 自建 + `Input type=file` / 拖拽事件 | **不要**整块套 `role="button"`：里面有多个可点控件 |
| `.list-rows` + `.list-add` | 自建数组渲染 + `Button` | 删除到空要补一行；行的控件按 item 类型 |
| `.progress` | `Progress` | 8px 高、胶囊；不确定态要有静态替代 |
| `.terminal` | `ScrollArea` + `<pre>` | 固定深色，不跟随主题；`tabindex=0` 让键盘能滚 |
| `#result` | `Card` + `<pre>` | JSON 用纯文本渲染，不走 `innerHTML` |
| `.file-chip` | `Badge` 包成 `<button>` | 可聚焦、可回车清除，`aria-label` 写清清除的是哪个文件 |
| `.card` | `Card` | 全页唯一阴影；暗色下 `shadow-none` |
| `.topbar` | 自建 header（`sticky`） | 只放品牌、命令切换、主题按钮 |
| 主题切换 | `next-themes` 或自建 class 切换 | lilyco 用 `data-theme`，shadcn 用 `.dark`；二者选一，别混 |

---

## 4. 排版与间距

| 项目 | lilyco | Tailwind 对应 |
|---|---|---|
| 正文 | 15px / 1.6 | `text-[15px] leading-relaxed` |
| 命令说明 | 14px / 1.7 | `text-sm leading-[1.7]` |
| 字段标签 | 13px / 600 | `text-[13px] font-semibold` |
| 元信息 | 12.5px | `text-xs` + 自定义 |
| 小标题 | 11.5px / 大写 / 0.8px 字距 | `text-[11.5px] uppercase tracking-[0.8px]` |
| 间距 | 4/8/12/16/24/32/48 | `gap-1/2/3/4/6/8/12` |
| 圆角 | 8 / 12 / 999 | `rounded-lg` 需校准到 8px；`rounded-xl` 12px；`rounded-full` |

Tailwind 默认 `rounded-lg` 是 `var(--radius)`，shadcn 模板常设为 10px。
如果坚持 lilyco 的三档，就把它设成 `0.5rem`，否则两套页面圆角会对不上。

---

## 5. 主题、双语、可缩放

这三件事在 lyco 的项目里是硬需求，两套实现都要满足。

### 5.1 主题

| | lilyco-gui | React + shadcn |
|---|---|---|
| 切换 | `html[data-theme]` + `localStorage["lilyco-theme"]` | `.dark` class（`next-themes` 或自建） |
| 默认 | 跟随 `prefers-color-scheme` | 同上 |
| 变量位置 | `:root` / `[data-theme="dark"]` | `:root` / `.dark` |
| 原生控件 | 必须声明 `color-scheme` | 同样要声明，否则下拉箭头在暗色下是白的 |

### 5.2 双语

- 文案走字典（`i18n-dict.ts` 这类），不在组件里硬编码；
- 中英混排时正文行高按 CJK 1.7，UI 标签可紧到 1.4；
- 按钮宽度按**较长语言**预留，不要用 `nowrap` 硬撑导致溢出。

### 5.3 可缩放（窄容器场景必做）

OBS Dock 可能只有 360px 宽，默认字号在窄条里读不动。两条要求：

1. **字号用 rem**，整体缩放只改 `html { font-size }`；
2. **提供显眼缩放控件并持久化**（如顶栏 `− 100% +`、设置页 75%–250% 七档，存 `localStorage`）。

React 侧参考实现要点：

```ts
const apply = (pct: number) => {
  document.documentElement.style.fontSize = `${16 * pct / 100}px`;
  localStorage.setItem("ui-scale", String(pct));
};
```

容器宽度用 `@container`（Tailwind v4 支持 `@container` 与 `@sm:` 变体），
不要只写视口断点 —— 原因和 [03 §6](03-design-language.md) 一样：视口可能是 1920px，面板只有 360px。

---

## 6. 无障碍对照

两套都要满足，React 侧别以为用了 Radix 就自动过关。

| 要求 | lilyco 做法 | React 侧检查点 |
|---|---|---|
| 焦点可见 | `outline` 2px + offset 2px | 不要全局 `outline: none`；`focus-visible:ring-2` |
| 标签绑定 | `<label for>` | `Label htmlFor` + `id` |
| 状态可读 | `role=status` / `role=log` + `aria-live` | 同 |
| 区间提示 | `aria-describedby` 指向 `.field-hint` | 同，别只写 placeholder |
| 可滚动区 | `tabindex=0` + 内描边 | `ScrollArea` 需自己补键盘可达 |
| 进度 | 无比例不写 `aria-valuenow` | 同 |
| 复制反馈 | 成功/失败都改文字与颜色 | Toast 不算反馈的唯一手段，按钮本身也要变 |

---

## 7. 视觉连续性检查表

做完一面新墙，用这张表对照内置页：

- [ ] 强调色只有一种，且只出现在那六个位置；
- [ ] 卡片是唯一有阴影的东西，暗色下没有阴影；
- [ ] 没有 `10px` / `16px` 圆角与间距；
- [ ] 控件 40px 高，图标按钮 34px，紧凑按钮 28px；
- [ ] 正文对比度 ≥7:1，次要 ≥4.5:1，控件边 ≥3:1；
- [ ] 320px 宽容器里没有横向溢出，按钮组不塌；
- [ ] 关掉动效后仍能看出「在跑 / 成功 / 失败」；
- [ ] Tab 走一遍，焦点环始终可见；
- [ ] 字号可整体缩放并持久化；
- [ ] 没有外链字体、图标或 CDN 资源（内置端必须；React 端允许但会破坏离线一致性）。

---

想先跑通内置端再对齐：从 [第 1 章](tutorial/01-hello-schema.md) 开始。