# 输入控件

> 四种最基础的参数控件：复选框、文本框、数字框、下拉。
> 它们都由参数类型自动派生，通常不需要你写任何 HTML。

---

## 1. 复选框（`Flag`）

**什么时候出现**：字段类型是 `bool`。

**结构**

```html
<div class="field field-flag">
  <label class="flag-row" data-component="flag">
    <input type="checkbox" id="field-quiet" checked>
    <span>只输出总数，不打印明细</span>
  </label>
</div>
```

**状态**

| 态 | 表现 |
|---|---|
| hover | 原生 |
| focus | `input:focus-visible` 环 |
| checked | 原生勾选，`accent-color: var(--accent)` |
| disabled | `opacity .55`（样式已就位，当前没有只读参数的触发方） |
| loading / empty / error | n/a —— 开关没有中间态 |

**令牌**：`--accent`（`accent-color`）、`--s-2`、`--s-1`（行距）。

**无障碍**：整行是 `<label>`，文字即可点区域；不要用 `div` + JS 重造开关。

**别做**：不要把它做成 iOS 风格开关 —— 这是「是/否」参数，原生复选框更省认知。

**源码**：`render.rs::widget_flag`、`app.css` 的 `.flag-row`。

---

## 2. 文本框（`Text`）

**什么时候出现**：`String`，以及没有特殊类型的自定义类型。

**结构**

```html
<div class="field">
  <label for="field-file">要统计的文件路径<span class="req-mark" aria-hidden="true">*</span></label>
  <input type="text" id="field-file" data-component="text"
         placeholder="要统计的文件路径" required value="">
</div>
```

**状态**

| 态 | 表现 |
|---|---|
| hover | 描边压深到 `--ink-muted` |
| focus | 描边转 `--accent` + 3px 扩散环 |
| disabled | `opacity .55` + `cursor: not-allowed` |
| error | `:user-invalid` 描边转 `--danger`（只在用户动过之后） |
| empty | 只有占位文案，没有别的状态 |
| loading / success | n/a —— 一行输入没有中间态，成没成由运行结果说 |

**令牌**：`--control`、`--ink`、`--danger`、`--accent-soft`、`--r-control`、`--s-2/s-3`。

**无障碍**：
- `label for` 与 `id` 必须配对；
- 必填用原生 `required`，红星只是视觉提示且 `aria-hidden`；
- 视口 ≤720px 时字号提到 16px，防 iOS 聚焦缩放。

**别做**：不要用 placeholder 替代标签（输入后就消失）；不要给输入框加内阴影。

**源码**：`render.rs::widget_text`、`app.css` 的 `input[type="text"]`。

---

## 3. 数字框（`Number`）

**什么时候出现**：整数 / 浮点类型（`u8`、`u32`、`f64` …）。

**结构**

```html
<div class="field">
  <label for="field-limit">最多统计多少行</label>
  <input type="number" id="field-limit" data-component="number"
         value="0" min="0" max="1000000" step="any"
         aria-describedby="hint-limit">
  <span class="field-hint" id="hint-limit">取值 0 – 1000000</span>
</div>
```

**状态**

| 态 | 表现 |
|---|---|
| hover / focus / disabled / empty / loading / success | 与文本框同名同法 |
| error（越界） | `:out-of-range` 与 `:user-invalid` 描边转红 |
| 区间提示 | 单边也要说：「取值 >= 0」/「取值 <= 100」，无区间则不画提示行 |

**令牌**：同文本框 + `--ink-muted`（提示行）。

**无障碍**：
- `min` / `max` 进属性 **并且** 渲染成可见 `.field-hint`；
- `aria-describedby` 必须指向提示节点，否则读屏听不到约束；
- 不写 `default` 时框里填的是 `min`（有意为之：给一个合法起点），
  这意味着「用户没动它也会把 min 交上去」，与 CLI 省略即缺省不同。

**别做**：不要只在属性里写区间；不要用 `step="1"` 卡死小数参数。

**源码**：`render.rs::widget_number` / `number_attrs`、`app.css` 的 `:out-of-range`。

---

## 4. 下拉（`Enum`）

**什么时候出现**：`#[derive(ValueEnum)]` 的枚举。

**结构**

```html
<div class="field">
  <label for="field-mode">统计口径</label>
  <select id="field-mode" data-component="enum">
    <option value="lines" selected>lines</option>
    <option value="bytes">bytes</option>
    <option value="words">words</option>
  </select>
</div>
```

枚举值由 `ValueEnum` 派生规则决定：变体名转 `snake_case`（`Jpeg` → `jpeg`，`UpperCase` → `upper_case`）。

**状态**

| 态 | 表现 |
|---|---|
| hover | 描边压深 |
| focus | accent 描边 + 扩散环 |
| disabled | `opacity .55` + `cursor: not-allowed` |
| busy（切命令时） | `aria-busy="true"` + `cursor: progress` —— 是「正在办」不是「不许办」 |
| empty / error / loading / success | n/a —— 值域非空，选得出的就是合法值 |

**令牌**：`--control`、`--ink`、`--r-control`、40px 高。

**无障碍**：原生 select 自带键盘操作；40px 高与其他控件对齐。
多命令页顶栏的命令下拉另有 `aria-label="切换命令"`。

**别做**：值域超过 ~10 项或需要搜索时，先考虑拆命令，而不是换 Radix 组件（内置端零依赖）。

**源码**：`render.rs::widget_enum` / `options_html`、`app.css` 的 `select`。

---

## 四种控件的共同外壳

除 Flag 外，都由 `field_shell` 包一层：

```html
<div class="field">
  <label for="field-<arg>">标签<span class="req-mark" aria-hidden="true">*</span></label>
  <!-- 控件 -->
</div>
```

- 标签文案来自 `#[arg(about = "...")]` 或 `///` 文档注释；
- 必填星号由 `required` 派生；
- 字段之间 `margin-bottom: var(--s-4)`；
- 标签与控件之间 `var(--s-2)`。

改外壳会同时影响四种控件 —— 所以它是 `field_shell` 一个函数，不是四处重复。