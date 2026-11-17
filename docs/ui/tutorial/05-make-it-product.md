# 第 5 章 · 让页面像产品

> **本章你会学到**
> 1. 为什么改外观要「先改表、再改代码」；
> 2. 主题切换的三个入口与 `color-scheme` 的作用；
> 3. 窄容器里按钮组为什么会塌，以及正确写法；
> 4. 改样式后必须跑哪条命令、量哪几件事。

这一章不改 Rust，只改设计系统的取值与规则。

---

## 5.1 三层：令牌 → 组件 → 页面

```text
lilyco-gui/DESIGN.md      ← 数值与规则的唯一来源（表）
        ↓
assets/app.css :root      ← 令牌定义（浅色）与 [data-theme="dark"]（深色）
        ↓
组件规则                   ← 只引用 var(--x)，不写第二份颜色
        ↓
assets/index.html         ← 结构与行为；DOM 名是契约
```

改外观的正确顺序是**从上往下**。反过来（先在组件里写死一个 `#2563eb`）就会产生第二份来源，
测试会红，暗色主题会漏。

---

## 5.2 改一个令牌：完整流程

以「把品牌蓝换掉」为例（示意，不要直接把示例值提交）。

### 第一步：改表

`lilyco-gui/DESIGN.md` §1.1（浅色）与 §1.2（深色）：

```text
| accent | #2563eb |   →   | accent | #1d4ed8 |
| accent | #6ea1ff |   →   | accent | #5b8cff |
```

同时在「实测」那一行补上新的对比度（后面第三步会量）。理由也写一句：为什么换、换完解决了什么。

### 第二步：改代码

`assets/app.css` 的 `:root` 与 `[data-theme="dark"]`：

```css
:root { --accent: #1d4ed8; }
[data-theme="dark"] { --accent: #5b8cff; }
```

还要同步 `@media (prefers-color-scheme: dark)` 里那一份 —— 它复制了深色令牌，用于「没有手动设主题」的情况。

### 第三步：跑闸门

```bash
cargo test -p lilyco-gui
```

会盯住你的是：

| 测试 | 你违反了什么 |
|---|---|
| `every_css_color_is_in_the_design_table` | CSS 里的色值没写进 DESIGN.md |
| `assets_stay_within_the_documented_budget` | 资源体积超预算 |
| `reduced_motion_keeps_a_non_animated_state_signal` | 删掉了 reduce 下的静态状态 |

### 第四步：浏览器量

`cargo test` 看不见对比度，也看不见 360px 容器里的挤压。必须真的打开页面：

```bash
cargo run -- --gui
```

- 明 / 暗各一遍；
- 从 320px 拖到 1920px，或把页面塞进同域 `srcdoc` iframe 改宽度；
- 对正文、次要文本、控件描边三档各量一次对比度（目标 7:1 / 4.5:1 / 3:1）。

---

## 5.3 主题：三个入口，一个 `color-scheme`

| 优先级 | 来源 | 说明 |
|---|---|---|
| 1 | `html[data-theme="dark"\|"light"]` | 用户点过 `◐`，存 `localStorage["lilyco-theme"]` |
| 2 | `@media (prefers-color-scheme: dark)` | 没手动设过时跟随系统 |
| 3 | `:root` | 兜底浅色 |

两个容易忘的点：

1. **`color-scheme` 必须随主题声明**（`light` / `dark`）。不声明的话，深色主题下原生下拉箭头、
   滚动条、日期选择器仍然是白底 —— 像一块贴错地方的纸。
2. **深色不是反色**。`canvas < surface < surface-2` 的色阶要重新挑，强调色要提亮，
   卡片阴影要置 `none`。

---

## 5.4 窄容器：组件看自己，不看视口

页面会被塞进 OBS Dock、分屏、iframe。视口可能是 1920px，卡片只有 360px。

```css
.card, .field, .actions, .out-head, .topbar { container-type: inline-size; }

@container (max-width: 420px) {
  .actions { flex-direction: column; }
  .actions .btn { width: 100%; flex: 0 0 auto; }   /* 必须收回基准 */
}
```

两个真实坑：

1. **`flex` 子项要 `min-width: 0`**。默认 `auto` 时，一条 150 字符的 Windows 路径就能把整行撑破。
2. **竖排时基准会变成高度**。`flex: 1 1 140px` 在 `column` 下把 140px 当成高度，
   实测按钮被顶到 140px 高。竖排那一档必须改回 `flex: 0 0 auto`。

---

## 5.5 动效：关掉动画不等于关掉状态

| 类型 | 时长 | 属性 |
|---|---|---|
| 微交互 | 150ms | `border-color` / `background` / `transform: scale(.98)` |
| 状态切换 | 200ms | `background` / `color` / 进度条 `width` |
| 进场 | 300ms | `opacity` + `translateY(6px)` |

`prefers-reduced-motion: reduce` 下，两个「只靠动来说话」的组件要有静态替代：

- 不确定进度条：从「40% 来回跑」改成「铺满 + `opacity .5`」；
- 主按钮 loading：转圈隐藏、标签恢复并追加 `…`，同时保留 `aria-busy` / `disabled` / `cursor: progress`。

注意 `*` 选不到伪元素；要停 `::after` 的动画得点名它。

---

## 5.6 现在你应该看到

| 动作 | 应该看到 |
|---|---|
| `cargo test -p lilyco-gui` | 全部通过；任何一条红都指向一处漂移 |
| 改一个令牌后重跑 `--gui` | 页面变了，但组件结构没变 |
| 点 `◐` | 立刻切主题，刷新后仍记得（`localStorage`） |
| 清掉 `lilyco-theme` 再刷新 | 跟随系统深浅色 |
| 把窗口拖到 360px | 动作行竖排、按钮占满，无横向滚动条 |
| 系统开启「减少动态效果」 | 进度条不再跑动，但仍能看出「在跑」；loading 按钮显示 `运行 …` |

---

## 小结

- 改外观 = 先改 `DESIGN.md`，再改 `app.css`，再跑测试，最后浏览器量。
- 令牌只在一处定义；组件规则里不出现第二份颜色。
- 主题三个入口，别忘了 `color-scheme`。
- 组件级用 container query；`min-width: 0` 与竖排 `flex-basis` 是两个高频坑。
- 关掉动画必须留下静态状态。

---

## 练习

1. **改一档间距**：把 `--s-3` 从 12px 调到 16px，跑测试并列出哪些组件变了。
   提示：若测试没拦住，说明该值没有被写进表 —— 那正是要修的地方。
2. **验证 hairline 不该调深**：把 `--hairline` 调到接近 `--control` 的深浅，看卡片边界是否开始抢内容。
   提示：hairline 是装饰性分隔，不承担「认出组件」的 3:1 要求。
3. **窄容器实测**：把页面塞进 360px 宽的 iframe，检查按钮组与文件 chip 是否溢出。

---

## 下一章

[第 6 章 · 给 agent 用](06-agent-surface.md)：把同一个命令暴露成 MCP 工具，并真的调一次。