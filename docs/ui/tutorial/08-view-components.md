# 第 8 章 · 声明组件，而不是画组件

> **本章你会学到**
> 1. 为什么后端只该说「这是进度条」，而不是「画一个 8px 蓝条」；
> 2. `ctx.view(id, Component::…)` 的运行时声明；
> 3. `#[view(id, title, kind)]` 的静态槽位；
> 4. 内置 GUI 怎么渲染，换 shadcn/ui 时要改哪一处；
> 5. 52 类组件怎么验收。

---

## 8.1 问题

第 3 章你已经会发 `tick` 了。但它只能表达「进度」，表达不了「一张表」「一个状态徽章」「一个带确认的弹窗」。
于是每个项目都会开始自己拼 HTML —— 后端开始知道前端长什么样，换 UI 就得改业务代码。

这一章要做的，是把「**是什么**」和「**长什么样**」分开：

```text
后端：Component::Progress { current: 3, total: 10 }
        ↓  JSON（协议）
前端 A（内置 GUI）：<div class="v-progress">…</div>
前端 B（shadcn）：<Progress value={3} max={10} />
前端 C（TUI）：一行 "progress 3/10"
```

后端只依赖 `lilyco-core::view::Component`，不 import 任何前端代码。

---

## 8.2 运行时声明

```rust
use lilyco::prelude::*;

fn run_views(app: &Views, ctx: &Context) -> Result<serde_json::Value, AppError> {
    ctx.view(
        "job",
        Component::Progress {
            label: Some("处理中".into()),
            current: Some(0),
            total: Some(app.steps),
        },
    );

    for i in 1..=app.steps {
        ctx.view(
            "job",
            Component::Progress {
                label: Some(format!("第 {i} 步")),
                current: Some(i),
                total: Some(app.steps),
            },
        );
    }

    ctx.view(
        "summary",
        Component::Table {
            caption: Some("结果".into()),
            columns: vec!["项".into(), "值".into()],
            rows: vec![vec!["steps".into(), app.steps.to_string()]],
            sortable: false,
        },
    );

    let r = serde_json::json!({ "steps": app.steps });
    ctx.done(r.clone(), 0);
    Ok(r)
}
```

同一个 `id` 的多次调用是**更新**：进度条推进、表格刷新，前端原位替换。

`--json-stream` 会逐行输出（实测）：

```json
{"type":"view","id":"job","component":{"kind":"progress","label":"处理中","current":0,"total":3}}
{"type":"view","id":"job","component":{"kind":"progress","label":"第 1 步","current":1,"total":3}}
{"type":"view","id":"summary","component":{"kind":"table","caption":"结果","columns":["项","值"],"rows":[["steps","3"]],"sortable":false}}
```

---

## 8.3 静态槽位

运行时声明有个问题：页面在第一次 `view` 事件之前是空的。
静态槽位让命令一出现就告诉页面「我有哪几块区域」：

```rust
#[derive(App)]
#[app(name = "views", about = "演示后端声明组件", run = "run_views")]
#[view(id = "job", title = "任务进度", kind = "progress")]
#[view(id = "summary", title = "结果表", kind = "table")]
struct Views {
    /// 步骤数
    #[arg(default = 5)]
    steps: u64,
}
```

`#[view]` 可以写多个。它会出现在注册表的契约里（实测）：

```bash
cargo run -- --schema | grep -A4 '"views"'
```

```json
"views": [
  { "id": "job", "title": "任务进度", "kind": "progress" },
  { "id": "summary", "title": "结果表", "kind": "table" }
]
```

> 单命令的 `--schema` 输出的是命令 schema，不包含 `views`；
> 多命令（`run_registry`）的 `--schema` 输出注册条目，才带 `views`。

前端可以先按 `kind` 画骨架（`Skeleton`），事件到了再替换成真组件。

---

## 8.4 前端怎么接

内置 GUI 的契约只有四条：

1. 页面有 `#views` 容器；
2. `applyEvent` 里 `case "view": showView(p.id, p.component)`；
3. `renderComponent(c)` 按 `c.kind` 分派，根节点带 `data-view-kind`；
4. 静态槽位由服务端渲染成 `skeleton` 占位，运行时按同一个 `id` 原位替换
   （实测：页面初始有 `data-view-id="job"` 的骨架，运行后 `skeletons: 0`）。

浮层带 `data-close` 按钮：`<dialog>` 调原生 `close()`，`<details>` 自行折叠。
实测 `<dialog>` 关闭前 `open: true`，点关闭后 `open: false`。

于是验收不需要看截图，直接查 DOM：

```js
[...document.querySelectorAll('[data-view-kind]')].map(x => x.dataset.viewKind)
```

本仓库实测（无头 Chromium 真点「运行」）：

```json
{"slots":["v00","v01",…"v51"],
 "kinds":["card","section","separator","scroll_area","aspect_ratio","collapsible",
          "accordion","tabs","breadcrumb","pagination","sidebar","resizable","carousel",
          "progress","badge","alert","toast","skeleton","avatar","key_value","table",
          "chart","log","result","button","toggle","toggle_group","tooltip","hover_card",
          "dialog","sheet","drawer","alert_dialog","popover","dropdown_menu","context_menu",
          "menubar","command","navigation_menu","input","textarea","select","checkbox",
          "radio_group","switch","slider","combobox","input_otp","calendar","date_picker",
          "form","label"]}
```

---

## 8.5 换一套 UI

后端不改，只替换 `renderComponent`：

```jsx
// React + shadcn/ui（示意）
function renderComponent(c) {
  switch (c.kind) {
    case "progress":
      return <Progress value={c.current} max={c.total ?? 100} />;
    case "badge":
      return <Badge variant={toneMap[c.tone]}>{c.text}</Badge>;
    case "table":
      return <Table columns={c.columns} rows={c.rows} />;
    case "dialog":
      return <Dialog open={c.open}><DialogContent>{c.children.map(renderComponent)}</DialogContent></Dialog>;
    // …
  }
}
```

完整的 51 条对照（哪个由协议承载、哪个由 `ArgKind` 承担、哪个由前端本地实现）见
[视图组件协议](../view-protocol.md) §4。

---

## 8.6 组件清单

协议共 52 个变体，按用途分四组：

| 组 | 组件 |
|---|---|
| 核心 | card / section / separator / progress / badge / alert / toast / skeleton / key_value / table / log / result / button |
| 布局导航 | scroll_area / aspect_ratio / collapsible / accordion / tabs / breadcrumb / pagination / sidebar / resizable / carousel / avatar / chart |
| 浮层交互 | dialog / sheet / drawer / alert_dialog / popover / dropdown_menu / context_menu / menubar / command / navigation_menu / tooltip / hover_card / toggle / toggle_group |
| 表单控件 | input / textarea / select / checkbox / radio_group / switch / slider / combobox / input_otp / calendar / date_picker / form / label |

表单**参数**仍然优先用 `ArgKind`（第 2 章）：它能被 CLI 解析、被 MCP 校验、进入 `from_args`。
`Component` 里的输入控件用于「视图内部的表单」，不参与命令参数。

---

## 8.7 现在你应该看到

| 检查 | 应该看到 |
|---|---|
| `cargo build` | 通过；`#[view]` 不报未知属性 |
| `--schema`（多命令） | `views` 数组里每个槽位有 id/title/kind |
| `--json-stream` | 逐行 `{"type":"view",…}` |
| `--gui` + 运行 | 表单下方出现视图区；进度条推进、表格/徽章/日志各自成形 |
| 换主题 | 视图区跟着 `--surface` / `--ink` / `--accent` 走（日志与结果沿用固定色板） |
| 320px 宽 | 卡片、表格、按钮组按容器宽度换行，无横向溢出 |

---

## 小结

- 后端只声明 `Component`，不写 HTML/CSS。
- 运行时 `ctx.view(id, …)` 更新；静态 `#[view(id,title,kind)]` 预留槽位。
- 协议是带 `kind` 的可嵌套 JSON，`snake_case`。
- 前端只实现一个 `renderComponent`；换 UI 不动 Rust。
- 表单参数仍走 `ArgKind`；`Component` 管展示与视图内控件。

---

## 练习

1. **加一个槽位**：给第 4 章的 `count` 加 `#[view(id="summary", title="统计", kind="table")]`，
   在函数里发一张两行表，确认 `--schema` 与页面都出现。
2. **换一种渲染**：把 `progress` 的渲染改成文字 `3 / 10`（不画条），跑一次验收脚本，
   确认 `data-view-kind` 仍是 `progress` —— 协议没变，只是外观变了。
3. **验证更新语义**：对同一个 `id` 连发 5 次 progress，确认页面上只有一条进度条在动，而不是 5 条。

---

## 结课

到这里，你走完了：一个命令 → 参数 → 事件 → 多命令 → 设计系统 → agent 面 → 交付 → 组件协议。

继续查阅：

- [视图组件协议](../view-protocol.md)（51 条映射与协议细节）
- [附录 A · 注解速查](../appendix/A-attributes.md)（`#[view]` 与 `Component`）
- [设计语言](../03-design-language.md) / [组件参考](../components/README.md)
- [验证记录](VERIFICATION.md)（本章所有数字的来源）