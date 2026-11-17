# 视图组件协议

> 后端只声明「这是什么组件、带什么数据」，前端决定「怎么画」。
> 同一份 JSON，内置 GUI 渲染成令牌化 HTML，React + shadcn/ui 渲染成 `<Progress>` / `<Table>`，
> 其他 UI 也能接。协议层不依赖任何前端框架。

---

## 1. 两个入口

```rust
// 静态：命令一出现就有槽位（页面可先画壳）
#[derive(App)]
#[app(name = "download", about = "下载文件", run = "run_dl")]
#[view(id = "job", title = "下载进度", kind = "progress")]
struct Download { /* … */ }

// 运行时：按 id 填充/更新
fn run_dl(app: &Download, ctx: &Context) -> Result<serde_json::Value, AppError> {
    ctx.view("job", Component::Progress {
        label: Some("下载中".into()),
        current: Some(3),
        total: Some(10),
    });
    // …
    Ok(result)
}
```

| 入口 | 作用 | 出现在哪 |
|---|---|---|
| `#[view(id, title, kind)]` | 静态槽位（可重复） | `run_registry` 的 `--schema` → `views` 数组 |
| `ctx.view(id, Component)` | 运行时声明/更新 | `Progress::View` 事件 → `--json-stream` / SSE / MCP |

同一个 `id` 的后续调用视为**更新**（进度条推进、表格刷新），前端应原位替换而不是叠加。

---

## 2. 事件形状

```json
{"type":"view","id":"job","component":{"kind":"progress","label":"下载中","current":3,"total":10}}
```

组件本身是一个**带 `kind` 标签的 JSON 对象**，可递归嵌套：

```json
{"kind":"card","title":"概览","children":[
  {"kind":"key_value","items":[{"key":"版本","value":"0.3.0"}]},
  {"kind":"table","columns":["名","值"],"rows":[["a","1"]]}
]}
```

约定：

- `kind` 是 `snake_case`，与 `lilyco-core/src/view.rs` 的变体一一对应；
- 缺省字段在反序列化时有默认值（如 `badge.tone` 默认 `neutral`、`skeleton.lines` 默认 3）；
- 文本一律按纯文本处理（前端必须先转义），协议不承载 HTML。

---

## 3. 内置渲染器契约

`lilyco-gui` 的实现只做四件事：

1. `#views` 容器 + `applyEvent` 的 `case "view"`；
2. `renderComponent(c)` 按 `kind` 分派，输出 `<div class="v v-<kind>" data-view-kind="<kind>">`；
3. **静态槽位先画骨架**：服务端把 `views` 渲染成
   `<section class="view" data-view-id="…" data-view-title="…" data-view-kind-hint="…">` +
   `skeleton` 占位，运行时按同一个 `id` 原位替换（`clearViews()` 只把内容退回骨架，不删槽位）；
4. 浮层（`dialog`/`sheet`/`drawer`/`alert_dialog`）带 `data-close` 按钮，点了调原生 `dialog.close()`；
   `<details>` 类浮层本来就能折叠。

替换渲染器（换成 shadcn）时**只需重写 `renderComponent`**：

```js
// 伪代码：React 侧
function renderComponent(c) {
  switch (c.kind) {
    case "progress": return <Progress value={c.current} max={c.total} />;
    case "badge":    return <Badge variant={toneMap[c.tone]}>{c.text}</Badge>;
    // …
  }
}
```

协议与 Rust 侧无需任何改动。

---

## 4. 51 个 shadcn 组件的映射

「协议承载」= 后端可声明并渲染；「ArgKind」= 表单参数已由现有 `ArgKind` 表达；
「前端」= 纯交互/布局，由前端按自己的状态机实现，协议只给数据/结构。

| # | shadcn 组件 | 本协议 | 说明 |
|---|---|---|---|
| 1 | Accordion | `accordion` | items: id/title/body/open，原生 `<details>` |
| 2 | Alert | `alert` | title/description/tone |
| 3 | Alert Dialog | `alert_dialog` | id/title/description/open/confirm/cancel |
| 4 | Aspect Ratio | `aspect_ratio` | ratio + children |
| 5 | Avatar | `avatar` | name（首字母兜底），src 仅作提示 |
| 6 | Badge | `badge` | text + tone（neutral/info/success/warn/danger） |
| 7 | Breadcrumb | `breadcrumb` | items: 字符串数组 |
| 8 | Button | `button` | id/label/action/variant/disabled |
| 9 | Calendar | `calendar` | month/selected/marked |
| 10 | Card | `card` | title/description/children |
| 11 | Carousel | `carousel` | index + items（前端负责滑动） |
| 12 | Chart | `chart` | chart_kind(bar/line)/labels/series |
| 13 | Checkbox | `checkbox` | name/label/checked |
| 14 | Collapsible | `collapsible` | id/title/open + children |
| 15 | Combobox | `combobox` | name/value/options（原生 datalist） |
| 16 | Command | `command` | query + items(label/action/hint) |
| 17 | Context Menu | `context_menu` | items（前端绑定右键手势） |
| 18 | Data Table | `table` | caption/columns/rows/sortable |
| 19 | Date Picker | `date_picker` | name/label/value |
| 20 | Dialog | `dialog` | id/title/description/open + children |
| 21 | Drawer | `drawer` | id/title/open + children |
| 22 | Dropdown Menu | `dropdown_menu` | trigger + items |
| 23 | Form | `form` | children + submit（表单参数优先 ArgKind） |
| 24 | Hover Card | `hover_card` | trigger/title/body |
| 25 | Input | `input` | name/label/value/placeholder |
| 26 | Input OTP | `input_otp` | name/value/length |
| 27 | Label | `label` | text/html_for |
| 28 | Menubar | `menubar` | menus（label + items） |
| 29 | Navigation Menu | `navigation_menu` | items + active |
| 30 | Pagination | `pagination` | page/pages |
| 31 | Popover | `popover` | trigger/open + children |
| 32 | Progress | `progress` | label/current/total（total=null 为不确定） |
| 33 | Radio Group | `radio_group` | name/options/selected |
| 34 | Resizable | `resizable` | ratio + start/end |
| 35 | Scroll Area | `scroll_area` | max_height + children |
| 36 | Select | `select` | name/value/options |
| 37 | Separator | `separator` | label? |
| 38 | Sheet | `sheet` | side(top/right/bottom/left)/open |
| 39 | Sidebar | `sidebar` | items + active |
| 40 | Skeleton | `skeleton` | lines |
| 41 | Slider | `slider` | name/min/max/value |
| 42 | Sonner (Toast) | `toast` | text + tone |
| 43 | Switch | `switch` | name/label/checked |
| 44 | Table | `table` | 同 Data Table |
| 45 | Tabs | `tabs` | items + active |
| 46 | Textarea | `textarea` | name/value/rows |
| 47 | Toggle | `toggle` | id/label/pressed |
| 48 | Toggle Group | `toggle_group` | options + selected |
| 49 | Tooltip | `tooltip` | text + children（原生 title） |
| 50 | Command（菜单形态） | `command` | 同上，hint 给快捷键 |
| 51 | Carousel（页面形态） | `carousel` | 同上 |

协议内另有四个 shadcn 没有、但控制台需要的语义组件：

| 组件 | 用途 |
|---|---|
| `key_value` | 键值对（版本、路径、字节数） |
| `log` | 日志行（level + text），复用终端色板 |
| `result` | 运行结果（ok + title + JSON） |
| `section` | 无卡片边框的分区 |

### 4.1 逐组件四项核对

主表给出 shadcn 对应物；这里补齐另外三项（JSON 最小形状 · GUI DOM · 无障碍要点）。
`kind` 与 `lilyco-core/src/view.rs` 的变体一一对应。

```text
card          {"kind":"card","title","description","children"}          div.v-card > .v-title/.v-desc        无 role（标题为文本）
section       {"kind":"section","title","children"}                     section.v-section                    无 role
separator     {"kind":"separator","label"}                              div.v-sep[role=separator]             role=separator
scroll_area   {"kind":"scroll_area","max_height","children"}            div.v-scroll[tabindex=0]              可键盘滚动
aspect_ratio  {"kind":"aspect_ratio","ratio","children"}                div.v-aspect                         无 role
collapsible   {"kind":"collapsible","id","title","open","children"}     details.v-collapse > summary          Enter/Space 展开
accordion     {"kind":"accordion","items":[id,title,body,open]}         div.v-accordion > details             Enter/Space
tabs          {"kind":"tabs","items":[id,label,children],"active"}      .v-tablist[role=tablist] .v-tab[role=tab]  aria-selected
breadcrumb    {"kind":"breadcrumb","items":[str]}                       nav.v-crumb[aria-label]               末项 aria-current=page
pagination    {"kind":"pagination","page","pages"}                      nav.v-page[aria-label]                文本
sidebar       {"kind":"sidebar","items":[id,label],"active"}            nav.v-sidebar[aria-label]             .v-nav.active
resizable     {"kind":"resizable","ratio","start","end"}                div.v-resize (grid)                   无 role
carousel      {"kind":"carousel","index","items":[title,children]}      div.v-carousel                        无 role
progress      {"kind":"progress","label","current","total"}             div.v-progress[role=progressbar]       aria-valuenow（有比例才写）
badge         {"kind":"badge","text","tone"}                            div.v-badge.<tone>                    文本
alert         {"kind":"alert","title","description","tone"}             div.v-alert.<tone> > .v-alert-title    文本
toast         {"kind":"toast","text","tone"}                            div.v-toast.<tone>                    文本
skeleton      {"kind":"skeleton","lines"}                               div.v-skeleton > .v-sk-line           装饰，无文本
avatar        {"kind":"avatar","name","src"}                            div.v-avatar[role=img]                 aria-label=名称
key_value     {"kind":"key_value","items":[key,value]}                  div.v-kv > .v-kv-row                  dl 语义近似
table         {"kind":"table","caption","columns","rows","sortable"}    div.v-table > table                   原生表语义
chart         {"kind":"chart","chart_kind","labels","series"}           div.v-chart[role=img]                  aria-label=图表
log           {"kind":"log","lines":[level,text]}                       div.v-log > .line                     行文本
result        {"kind":"result","ok","title","value"}                    div.v-result.<ok?success:danger> > pre  文本
button        {"kind":"button","id","label","action","variant","disabled"} button.btn.v-btn[data-action]     原生 button
toggle        {"kind":"toggle","id","label","pressed"}                   button.btn[aria-pressed]              Enter/Space
toggle_group  {"kind":"toggle_group","id","options","selected"}         div.v-group[role=group] > .v-chip     文本/前端补
tooltip       {"kind":"tooltip","text","children"}                      span.v-tip[title]                     浏览器播报
hover_card    {"kind":"hover_card","trigger","title","body"}            details.v-hover > summary             Enter/Space
dialog        {"kind":"dialog","id","title","description","open","children"} dialog.v-dialog + [data-close]   Esc / 关闭按钮
sheet         {"kind":"sheet","id","title","side","open","children"}    dialog.v-dialog.v-sheet.<side>        Esc / 关闭按钮
drawer        {"kind":"drawer","id","title","open","children"}          dialog.v-dialog                        Esc / 关闭按钮
alert_dialog  {"kind":"alert_dialog","id","title","description","open","confirm","cancel"} dialog + chips + close  Esc / 关闭
popover       {"kind":"popover","id","trigger","open","children"}       details.v-pop > summary               Enter/Space
dropdown_menu {"kind":"dropdown_menu","id","trigger","items"}           details.v-pop > .v-menu               Enter/Space
context_menu  {"kind":"context_menu","items":[id,label,action,danger]}  div.v-menu                            前端绑定右键
menubar       {"kind":"menubar","menus":[label,items]}                  nav.v-menubar                         展示态
command       {"kind":"command","query","items":[label,action,hint]}    div.v-command > .v-command-item       前端补搜索键
navigation_menu {"kind":"navigation_menu","items":[id,label],"active"}  nav.v-navmenu > .v-nav                展示态
input         {"kind":"input","name","label","value","placeholder"}     label.v-row > input[type=text]        隐式 label
textarea      {"kind":"textarea","name","label","value","rows"}         label.v-row > textarea                隐式 label
select        {"kind":"select","name","label","value","options"}        label.v-row > select                  隐式 label
checkbox      {"kind":"checkbox","name","label","checked"}              label.v-row > input[checkbox]         隐式 label
radio_group   {"kind":"radio_group","name","label","options","selected"} div.v-group[role=radiogroup] > label  radio 语义
switch        {"kind":"switch","name","label","checked"}                label.v-row > input[role=switch]       原生
slider        {"kind":"slider","name","label","min","max","value"}      label.v-row > input[type=range]       原生
combobox      {"kind":"combobox","name","label","value","options"}      label.v-row > input[list] + datalist 原生
input_otp     {"kind":"input_otp","name","label","value","length"}      div.v-otp > input[aria-label]         每位 aria-label
calendar      {"kind":"calendar","month","selected","marked"}           div.v-cal[aria-label]                 展示态
date_picker   {"kind":"date_picker","name","label","value"}             label.v-row > input[type=date]        原生
form          {"kind":"form","children","submit"}                       div.v-form                            子控件可聚焦
label         {"kind":"label","text","html_for"}                        div.v-label[for]                      文本
```

### 无障碍契约（与内置渲染器一致）

| 组件 | 角色 / ARIA | 键盘 |
|---|---|---|
| progress | `role=progressbar` + `aria-valuemin/max`；有比例才写 `aria-valuenow` | 非交互 |
| avatar | `role=img` + `aria-label=名称` | 非交互 |
| tabs | `role=tablist` + `role=tab` + `aria-selected` | 方向键切换由前端补（当前展示态） |
| breadcrumb | `nav aria-label`；末项 `aria-current="page"` | 纯文本时不可聚焦 |
| pagination / sidebar / navigation_menu | `nav aria-label`；当前项 `.active` | 展示态；接路由后改链接 |
| chart | `role=img` + `aria-label` | 非交互 |
| scroll_area | `tabindex="0"` 可键盘滚动 | 方向键 / PageUp/Down |
| dialog / sheet / drawer / alert_dialog | 原生 `<dialog>`；`data-close` 按钮可聚焦 | Esc 关闭（原生） |
| popover / dropdown_menu / hover_card / collapsible / accordion | 原生 `<details>` / `<summary>` | Enter / Space 展开 |
| tooltip | `title` 属性 | 聚焦或悬停时由浏览器播报 |
| input / textarea / select / checkbox / radio_group / switch / slider / combobox / date_picker | 包在 `<label>` 内（隐式关联） | 原生控件键盘 |
| input_otp | 每位 `aria-label="第 n 位"` | 原生 |
| calendar | `aria-label=月份` | 非交互（展示标记日） |
| form | 容器 | 子控件各自可聚焦 |
| label | 纯文本 | 非交互 |
| button / toggle / toggle_group | 原生 `<button>`；toggle 带 `aria-pressed` | Enter / Space |

未实现（如实标注）：焦点陷阱、roving tabindex、`<details>` 的 Esc 关闭（浏览器原生行为）。
**换 shadcn 时**：这些行为由 Radix 提供，但仍需复核 `aria-*` 与键盘路径。

### 哪些不走协议

| 类别 | 由谁承担 | 原因 |
|---|---|---|
| 表单参数（Input/Select/Checkbox/Radio/Switch/Slider/Textarea/Combobox/Form/Label/Calendar/DatePicker 作为**参数**） | `ArgKind` | 参数需要 CLI/MCP 的校验与取值，视图组件不参与 `from_args` |
| 浮层的**开关状态** | 前端 | 协议给 `open` 初值，交互由 `<dialog>`/`<details>` 或前端状态机负责 |
| 拖拽 / 手势 / 键盘导航 | 前端 | 协议不描述事件，只描述结构与数据 |

---

## 5. 安全与体积

- 所有文本经 `esc()` 转义，`innerHTML` 只用于拼装**已转义**的片段；
- 浮层用原生 `<dialog>` / `<details>`，不需要任何 JS 运行时；
- 渲染器仍然零外部依赖（CSP `default-src 'none'`），没有 webfont / 图标库 / CDN；
- 协议新增会让 `index.html` / `app.css` 增长，预算与理由记在 `lilyco-gui/DESIGN.md §9`。

---

## 6. 验收方式

```bash
# 后端声明是否进入契约
cargo run -- --schema | grep -A4 '"views"'

# 事件流
cargo run -- --json-stream        # 逐行 view 事件

# 页面渲染（无头浏览器也可）
LILYCO_PORT=18994 cargo run -- --gui
# 打开后点「运行」，统计：
# [...document.querySelectorAll('[data-view-kind]')].map(x=>x.dataset.viewKind)
```

本仓库的实测结果记录在 [教程的验证记录](tutorial/VERIFICATION.md)。