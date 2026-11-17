# 组件参考

> 每个组件一页：什么时候出现、DOM 长什么样、有哪些状态、用哪些令牌、无障碍怎么过。
> 分页原则是「一起改的东西放一页」，不是「一个 class 一页」。

---

## 组件清单

| 组件 | 页 | 由谁产生 |
|---|---|---|
| 复选框 / 文本框 / 数字框 / 下拉 | [输入控件](input-controls.md) | `render.rs::widget_flag/text/number/enum` |
| 文件路径 + 拖拽区 + chip | [路径与拖拽](path-dropzone.md) | `widget_path` + `dropzone` |
| 动态列表 | [列表](list.md) | `widget_list` |
| 顶栏 / 卡片 / 页脚 | [页面骨架](shell.md) | `assets/index.html` 骨架 |
| 运行 / 取消 / 复制 CLI / CLI 预览 | [动作区](run-bar.md) | 骨架 + `initRunBar` |
| 输出区 / 进度 / 终端 / 结果 | [输出区](output.md) | 骨架 + `initProgress/initLog/initResult` |

---

## 三条组件级约定

### 1. 一个 `ArgKind` 一个 widget

服务端 `render.rs` 的 `widget_*` 按参数类型选控件；页面侧一个组件一个 `initXxx`。
两边用 `data-component="<名字>"` 对账（只给测试与调试读，**不是**行为锚点）。

### 2. 状态一律用伪类，不新增颜色

```css
/* 对 */
input:hover      { border-color: var(--ink-muted); }
input:focus-visible { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }

/* 错：第二份颜色 */
input.is-active { border-color: #2563eb; }
```

「七态」是这套组件的通用检查表：hover / focus / disabled / loading / empty / error / success。
**不存在的态写 n/a 并说明为什么**，不要为了凑格子留一条永远命中不到的规则。

### 3. 按钮只能许诺服务端做得到的事

新增动作前先确认后端有对应能力。典型例子：取消按钮只有在服务端登记了会话句柄时才画，
否则点了只会得到一句 404 —— 页面给了不该给的能力，比没有这个按钮更糟。

---

## 加一个组件：六步

1. `render.rs`：写 `widget_xxx`，接进 `render_field` 的 `match`，并带上 `data-component="xxx"`。
2. `assets/index.html`：写 `initXxx`，**加进 `BOOT`**（漏了会被 `every_js_component_init_is_bootted` 拦下）。
3. `assets/app.css`：只用令牌与 `--s-*`；状态用伪类；不新增颜色字面量（终端固定色板除外）。
4. 要在 DOM 上带信息就加 `data-*` —— 必须同时有读者（JS 或 CSS），否则 `no_data_attribute_goes_unread` 会红。
5. 在 `lilyco-gui/DESIGN.md` §10 加一行：构成 / 哪些态真的存在 / 用到的令牌 / 无障碍要求。
6. 浏览器实测：`srcdoc` iframe 走 320 → 1920，明暗两套各一遍。

装配顺序还有一条硬要求：**启动循环必须留在脚本最后一行**。
组件函数读的是顶层 `const`，提前跑会撞 TDZ，一个组件抛错会带走后面所有组件。
现在每个 `init` 各自 `try`，失败会写进日志区 —— 但顺序仍然不能动。

---

## 每个组件的公共无障碍底线

- 标签必须与控件绑定（`label for` 或整行 `label`）；
- 焦点环必须可见（`outline: 2px` + `offset`，面板内元素用负 offset）；
- 可滚动区域加 `tabindex="0"` 与 `:focus-visible` 内描边（WCAG 2.1.1）；
- 状态变化用 `role="status"` / `role="log"` + `aria-live="polite"`，不要只改颜色；
- 不用 `<div role="button">` 包住多个可交互控件；
- 不用内联 `onclick=` / `onchange=`，行为只写在 `initXxx` 里。

---

这些组件是怎么被 schema 驱动出来的：见 [第 2 章 · 参数就是界面](../tutorial/02-args-are-ui.md)。