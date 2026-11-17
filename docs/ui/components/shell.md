# 页面骨架

> 顶栏、卡片、页脚。这三块是每一页都有的骨架，也是最不该被「设计发挥」的部分。

---

## 1. 顶栏（`topbar`）

```html
<header class="topbar" data-component="topbar">
  <div class="brand">
    <span class="brand-mark" aria-hidden="true">◆</span> lilyco
    <span class="brand-sub">Web 控制台</span>
  </div>
  <!-- 多命令才有这一项 -->
  <select id="cmd-nav" data-component="command-nav" aria-label="切换命令">
    <option value="count" selected>count</option>
    <option value="help">help</option>
  </select>
  <button type="button" id="theme-toggle" class="icon-btn" aria-label="切换深浅主题">◐</button>
</header>
```

| 项 | 规则 |
|---|---|
| 常驻 | `position: sticky` + `backdrop-filter: blur(10px)`（全页唯一模糊） |
| 背景 | `color-mix(in srgb, var(--surface) 82%, transparent)` |
| 品牌 | 只有 `◆` 一个装饰字符，蓝；副标题在 ≤420px 隐藏 |
| 命令下拉 | 可见命令 >1 才出现；窄容器（≤420px）整行换到第二行 |
| 主题按钮 | 34×34，`order: 1` 永远钉在行尾；`aria-pressed` 反映当前是否深色 |

**状态**

| 态 | 表现 |
|---|---|
| hover | 图标按钮描边转 accent + accent-soft 底 |
| focus | 2px outline + 2px offset |
| pressed（深色开启） | 描边与字色转 `--accent`，`aria-pressed="true"` |
| busy（切换命令中） | 下拉 `disabled` + `aria-busy="true"` + `cursor: progress` |
| disabled / loading / empty / error | n/a —— 常驻骨架 |

**别做**：不要往顶栏加导航菜单、搜索框、用户头像。它不是应用栏，是状态栏。

---

## 2. 卡片（`card`）

```html
<main class="page">
  <section class="card">
    <h1 class="card-title">count</h1>
    <p class="card-about">统计文本文件的行数、字节数或词数</p>
    <form id="form" novalidate> … </form>
    <section id="out" hidden> … </section>
  </section>
  <footer class="foot">lilyco · 本地回环会话 · 令牌随进程轮换</footer>
</main>
```

| 项 | 规则 |
|---|---|
| 宽度 | `max-width: 860px`；≥1200px 放宽到 1040px |
| 内边距 | 24px；≤420px 收到 16px；≥721px 增到 32px |
| 边 | 1px `--hairline` |
| 圆角 | 12px（`--r-panel`） |
| 阴影 | 全页唯一层次阴影；深色下 `none` |
| 进场 | `rise` 300ms（`opacity` + `translateY(6px)`） |

**标题**：22px / 650 / 1.25 / -0.2px 字距，≤420px 降到 20px。
**说明**：14px / **1.7 行高** / `max-width: 72ch`，颜色 `--ink-muted`。

行高 1.7 是为中英混排准备的：命令说明常常是长英文，CJK 兜底需要更松的行距。

**别做**：卡片里再套卡片。需要分区时用 `hairline` 分隔或 `--s-5` 留白。

---

## 3. 页脚（`foot`）

```html
<footer class="foot">lilyco · 本地回环会话 · 令牌随进程轮换</footer>
```

12px / `--ink-muted` / 居中 / 与卡片同宽 / 上方 24px 留白。

这一行是**安全声明**，不是装饰：页面只监听回环，令牌每次启动随机。
如果哪天这个前提变了，这行字必须跟着改 —— 它承诺的是事实，不是文案。

---

## 令牌

`--surface`、`--surface-2`、`--hairline`、`--ink`、`--ink-muted`、`--accent`、
`--r-panel`、`--r-control`、`--s-2/s-4/s-5/s-6/s-7`、`--shadow-card`。

---

## 无障碍

- 顶栏是 `<header>`，主区是 `<main>`，页脚是 `<footer>`；
- 命令下拉有 `aria-label="切换命令"`；
- 主题按钮有 `aria-label` 与 `aria-pressed`，不能只靠颜色表达当前主题；
- 品牌标记 `◆` 是 `aria-hidden` —— 它纯装饰，读屏不该念「黑色菱形」。

---

## 源码

- 结构：`assets/index.html` 第 11–47 行
- 样式：`app.css` 的 `.topbar` / `.brand` / `.page` / `.card` / `.foot`（约 100–176、496–504 行）
- 设计表：`DESIGN.md` §10.3 的 `topbar` 行