# 列表

> `Vec<T>` 参数的动态行：服务端先画好两行，页面只负责增删；行的控件由 item 类型决定。

---

## 结构

```html
<div class="field">
  <label for="field-tag">标签</label>
  <div class="list-rows" id="list-tag" data-component="list"
       data-list="tag" data-item="Text">
    <div class="list-row">
      <input type="text" class="mono" data-list-item="tag" data-item-kind="Text"
             placeholder="标签" aria-label="标签">
      <button type="button" class="btn-icon row-del">－</button>
    </div>
    <!-- 第二行同上 -->
  </div>
  <button type="button" class="btn-icon list-add"
          data-component="list-add" data-list-add="tag">＋ 添加一项</button>
</div>
```

注意 `data-item`（容器级，收值时读一次）与 `data-item-kind`（每行，复制/清行时读）的分工。

---

## item 类型决定行内控件

| item 类型 | 行内控件 |
|---|---|
| `Text` | `<input type="text">` |
| `Number` | `<input type="number">`，带 `min` / `max` / `step="any"` |
| `Enum` | `<select>`，`<option>` 来自枚举变体 |
| `Path` | 等宽文本框（**不**每行再挂一套拖拽区，否则表单会被撑爆） |
| `Flag` | 一行 `<label class="flag-row">` + 复选框 |
| `List`（嵌套） | 不支持，退化为文本，交给服务端校验 |

映射只在服务端 `list_row` 里存在一份。JS 复制行时照 `data-item-kind` 重置值，
**不许**在 JS 里再写一份「类型 → 控件」表 —— 那会变成第二份真相，删空后行会退化成文本框。

---

## 状态

| 态 | 表现 |
|---|---|
| hover | 删除按钮转 `--danger`，控件描边压深 |
| focus | `.btn-icon` 有自己的焦点环；新加的那一行自动 `focus()` |
| disabled | 样式就位（`opacity .55`），当前没有只读列表参数的触发方 |
| empty | 删到最后一行时按模板补一行 —— 永远至少留一行可填 |
| loading / error / success | n/a —— 行的增删是本地操作，对错由服务端在运行时说 |

---

## 令牌

`--s-2`（行间距）、`--danger`（删除 hover）、`--hairline`（`.btn-icon` 描边）、`--r-control`。

---

## 无障碍

- 每行控件有 `aria-label`，取参数说明；
- 新增行后 `focus()` 跟上，键盘用户不会「加了一行但焦点还在按钮上」；
- 删除按钮是 `<button>`，不要做成无焦点的图标；
- 行内控件 `flex: 1 1 auto; min-width: 0` —— 否则一条长路径会把整行撑破。

---

## 实现细节（两个坑）

1. **删空要补一行**：以前删空后走的是一段手搓的兜底建行代码，于是 `Number` / `Enum`
   行删完变成文本框。现在新增与补行都克隆 `ROW_TMPL`（服务端画的第一行）。
2. **收值按 item 类型**：`Number` 行要发数字。服务端 `validate_kind` 的 `Number` 分支只认
   `as_f64`，发字符串会被「需要数字」打回。

> 已知限制：`#[derive(App)]` 取 List 值时目前一律按字符串收集，所以只有 `Vec<String>`
> 能端到端跑通；`Vec<u32>` / `Vec<Enum>` 会编译不过。这是宏侧问题，不是页面的问题 ——
> 页面已经能按 schema 把数字行、下拉、复选框画对了。

---

## 别做

- 不要在前端另写一份 item → 控件映射；
- 不要用 `tagName` 猜类型（读 `data-item-kind`）；
- 不要允许删到 0 行；
- 不要给列表行加卡片式容器（嵌套层次会抢内容）。

---

## 源码

- 结构：`render.rs::widget_list` / `list_row`
- 行为：`index.html` 的 `initList` / `addListRow` / `removeListRow` / `listValues`
- 样式：`app.css` 的 `.list-rows` / `.list-row` / `.row-del` / `.list-add`
- 设计表：`DESIGN.md` §10.1 的 `list` 行