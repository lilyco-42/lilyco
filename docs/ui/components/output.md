# 输出区

> 运行之后出现的三块内容：进度条、日志终端、结果 JSON。
> 它们的共同职责是回答「跑没跑完、跑到哪、结果是什么」。

---

## 结构

```html
<section id="out" hidden data-component="output">
  <div class="out-head">
    <h2>输出</h2>
    <div class="progress" id="progress" data-indet="1"
         role="progressbar" aria-label="运行进度"
         aria-valuemin="0" aria-valuemax="100">
      <div class="progress-bar" id="progress-bar"></div>
    </div>
  </div>

  <div id="log" class="terminal" role="log" aria-live="polite" tabindex="0"
       data-component="log"></div>

  <div class="result-wrap" id="result-wrap" hidden data-component="result">
    <div class="result-head">
      <span>结果</span>
      <button type="button" id="copy-result" class="btn-icon">复制</button>
    </div>
    <pre id="result" class="mono" role="region" aria-label="运行结果" tabindex="0"></pre>
  </div>
</section>
```

`#out` 初始 `hidden`，第一次运行才露出。**每次运行开头必须清空**：日志在滚、结果区
还写着上一次的 JSON，看着像这次刚出结果 —— 这是真实踩过的坑。

---

## 1. 进度条（`progress`）

| 态 | 表现 |
|---|---|
| empty | 宽度 0% —— 没开始就什么都不该有；收尾也**不给** 0% |
| indeterminate | `data-indet="1"`，40% 宽的条来回跑 |
| determinate | `data-indet="0"` + `aria-valuenow` + 内联宽度百分比 |
| done | `data-state="done"`，整条 `--ok` |
| error | `data-state="error"`，整条 `--danger`，宽度仍写 100% |
| reduce 动效下 | 不确定态改「铺满 + `opacity .5`」，不再左右跑 |
| hover / focus / disabled | n/a —— 非交互元素 |

规格：8px 高、胶囊、槽 `--surface-3`、填充 `--accent`、宽度过渡 200ms。

**无障碍**：`role="progressbar"` + `aria-valuemin/max`；**没有比例时不写 `aria-valuenow`**
（别骗读屏）；确定态才写四舍五入后的整数。

---

## 2. 日志终端（`log`）

| 态 | 表现 |
|---|---|
| empty | `等待输出…`（`#8b949e`，终端固定色板） |
| running | 持续追加，自动滚到底 |
| error 行 | `.line.err` → `#ff7b72` |
| warn 行 | `.line.warn` → `#e3b341` |
| telemetry 行 | `.line.tele` → `#56d364` |
| full | 上限 500 行，超出从头部删 |
| focus | 内描边（`outline-offset: -2px`） |

**终端不随主题变**：固定 `#0d1117` 底 / `#e6edf3` 字（16.02:1）。
理由是「日志是机器的输出」，浅色主题下不该像一块贴歪的纸。

**无障碍**：`role="log"` + `aria-live="polite"`；`tabindex="0"` 让键盘能滚（WCAG 2.1.1）。

**别做**：不要把日志改成跟随主题的浅色面板；不要用 `innerHTML` 拼日志（用 `textContent`）。

---

## 3. 结果区（`result`）

| 态 | 表现 |
|---|---|
| empty | 整块 `hidden`，每次运行开头清空 |
| success | 露出 `<pre>`，`JSON.stringify(value, null, 2)` |
| focus | `<pre>` 自己的内描边 |
| 宽屏 | 限高内滚（`min(560px, 70vh)`） |
| hover / disabled / loading / error | n/a —— 失败没有结果，也不在跑 |

视觉：`--surface-2` 底 + `--ink` 字 + 等宽 13px，**跟随主题**（与终端相反）。

**无障碍**：`role="region"` + `aria-label="运行结果"` + `tabindex="0"`；
横向也会溢出，所以和终端同一条规矩：键盘能滚。

**别做**：不要把结果渲染成富文本或表格 —— 它是 `serde_json::Value`，纯文本最诚实。

---

## 事件到界面的映射

| SSE 事件 | 界面动作 |
|---|---|
| `started` | 日志一行「运行中…」 |
| `tick` | 进度条按 `percent`；有 `message` 就追加日志 |
| `log` | 日志行（`warn` / `error` 上色） |
| `telemetry` | 日志 `key = value`，tele 色 |
| `done` | 进度 100% + 转绿；结果区展开；解除忙碌 |
| `error` | 进度 100% + 转红；日志红字；解除忙碌 |

收尾必须落到 `done` 或 `error` 之一 —— 这是 `Progress` 事件流的不变量，executor 会兜底合成。

---

## 令牌

`--surface-3`（槽）、`--accent`、`--ok`、`--danger`、`--hairline`、`--surface-2`、
`--ink`、`--ink-muted`、`--mono`、`--r-pill`、`--r-panel`、`--s-3/s-4/s-5`；
终端固定色板 `#0d1117` / `#e6edf3` / `#ff7b72` / `#e3b341` / `#56d364` / `#8b949e` / `#30363d`。

---

## 别做

- 不要给 `#out` 再叠一层 `aria-live`（日志与状态行已经是 live region，同一句会念两遍）；
- 不要用 0% 表示「失败」；
- 不要在不确定态写 `aria-valuenow`；
- 不要让日志超过 500 行还不裁剪（长任务的 tick 能刷出上万行）。

---

## 源码

- 结构：`assets/index.html` 第 32–44 行
- 行为：`initProgress` / `setProgress` / `setProgressState` / `initLog` / `logLine` /
  `initResult` / `clearResult` / `showResult` / `applyEvent`
- 样式：`app.css` 的 `.out-head` / `.progress` / `.terminal` / `#result`（约 408–507 行）
- 设计表：`DESIGN.md` §10.3 的 `output` / `progress` / `log` / `result` 行