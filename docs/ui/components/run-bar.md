# 动作区

> 运行、取消、复制 CLI，以及它们下面那行实时命令预览。

---

## 结构

```html
<div class="actions" data-component="run-bar">
  <button type="submit" id="run-btn" class="btn primary">
    <span class="btn-label">▶ 运行</span>
  </button>
  <button type="button" id="cancel-btn" class="btn danger" hidden>■ 取消</button>
  <button type="button" id="copy-cli" class="btn ghost">复制 CLI</button>
</div>
<div class="cli-preview mono" id="preview" data-component="cli-preview" aria-live="off"></div>
```

三颗按钮的角色不同，视觉权重必须区分：

| 按钮 | 类 | 视觉 | 语义 |
|---|---|---|---|
| 运行 | `.btn.primary` | 实底 accent | 主动作，`type="submit"` |
| 取消 | `.btn.danger` | 描边，hover 才转红 | 次动作；只在服务端能取消时出现 |
| 复制 CLI | `.btn.ghost` | 描边 | 辅助动作，不该抢主按钮 |

`cli-preview` 是**实时**的：每次表单 `input` / `change` 都重算，让人能直接抄走等价命令。

---

## 状态

### 运行按钮

| 态 | 表现 |
|---|---|
| hover | 底色压深（accent 88% 混 ink） |
| focus | 2px outline + 2px offset |
| active | `transform: scale(.98)` |
| loading | `.loading` + `aria-busy="true"` + `disabled` + `cursor: progress`；转圈，标签隐藏 |
| reduce 动效下 | 圈隐藏，标签恢复并追加 `…` —— 不动也要说清在忙 |
| disabled | `opacity .55` + `cursor: not-allowed`，且 hover 不再变色 |
| empty / error / success | n/a —— 按钮没有这些态 |

**`disabled` 同时是「一次只跑一个」的闸门**：`setBusy(true)` 在任何 `await` 之前同步发生，
所以第二次提交必被挡。别用 `if(es)` 代替 —— `es` 要等 `/run` 返回才赋值，实测连点三次
`requestSubmit()` 真的会发出三个请求。

### 取消按钮

| 态 | 表现 |
|---|---|
| 初始 | `hidden`；`CANCELABLE` 为假时永远不出现 |
| 提交后、sid 未回 | 可见但 `disabled`（此时点了确实什么都没发生） |
| sid 到手 | 解禁，可点 |
| 请求中 | 自禁 |
| 成功 | 日志一行 warning「已发送取消请求」并保持禁用（空点击比禁用更糟） |
| 失败 | 日志红字 + 重新解禁，让你能再点一次 |

### 复制 CLI

| 态 | 表现 |
|---|---|
| 平时 | 描边按钮 |
| success | `.copied`，描边与字转 `--ok`，文字变「已复制」 |
| error | `.copy-failed`，转 `--danger`，文字变「复制失败」 |
| 反馈期间 | `data-copying="1"` 挡住连点，1200ms 后复位 |

剪贴板被拒不能静默 —— 静默的复制等于没复制。`writeText` 同步抛（非安全上下文里
`navigator.clipboard` 是 undefined）也要接住，否则会永远卡在 `data-copying`。

### CLI 预览

| 态 | 表现 |
|---|---|
| empty | `display: none` —— 不留孤零零的 `$ ` |
| 有内容 | `--surface-2` 底、`--ink-muted` 字、`$ ` 前缀 accent |

---

## 令牌

`--accent`、`--on-accent`、`--control`、`--danger`、`--ok`、`--hairline`、
`--surface-2`、`--ink-muted`、`--r-control`、`--s-2/s-3/s-5`。

---

## 响应式（组件级）

| 容器宽度 | 行为 |
|---|---|
| ≥721px | 不换行，按钮按内容宽 |
| ≤560px | 允许换行，按钮 `flex: 1 1 140px` 并居中 |
| ≤420px | 竖排，每个占满；**必须把 `flex` 收回 `0 0 auto`** |

最后一档是最容易写出事故的地方：`flex: 1 1 140px` 在 `flex-direction: column` 下
会把 140px 解释成**高度**，实测按钮被顶到 140px 高。

---

## 无障碍

- 运行按钮保留 `type="submit"` 语义，回车提交与点击一致；
- loading 时 `aria-busy="true"` 给读屏，`disabled` 防重复触发；
- 复制反馈改按钮文字 + 颜色，不只改颜色；
- 预览区 `aria-live="off"` —— 它随每次输入变化，不该把读屏刷屏。

---

## 别做

- 不要加第四颗同权重按钮（导出、分享、保存…）—— 动作区只服务「跑一次」；
- 不要在取消按钮上承诺服务端做不到的事；
- 不要用 `aria-live` 播报预览；
- 不要把「复制 CLI」做成图标按钮（它是次要但常用的动作，文字更清楚）。

---

## 源码

- 结构：`assets/index.html` 第 24–30 行
- 行为：`initRunBar` / `setBusy` / `run` / `cancel` / `initCliPreview` / `copyText`
- 样式：`app.css` 的 `.btn` 一族（约 250–322 行）、`.actions`、`.cli-preview`
- 设计表：`DESIGN.md` §10.3 的 `run-bar` / `cli-preview` 行