# 路径与拖拽

> `PathBuf` 参数的完整形态：一个文本框 + 一个拖拽区，三条取文件的路最后都回填到同一个 input。

---

## 结构

```html
<div class="field">
  <label for="field-file">要统计的文件路径<span class="req-mark" aria-hidden="true">*</span></label>
  <input type="text" id="field-file" class="mono" data-component="path"
         placeholder="要统计的文件路径" required value=""
         spellcheck="false" aria-describedby="up-file">
  <div class="dropzone" data-component="dropzone" data-target="file"
       data-max-upload="209715200">
    <input type="file" class="visually-hidden" tabindex="-1" aria-hidden="true">
    <span class="dz-icon" aria-hidden="true">⇪</span>
    <span class="dz-hint">拖到这里，或点「选择文件」—— 上传后自动回填服务端路径</span>
    <span class="dz-status" id="up-file" role="status" aria-live="polite"></span>
    <button type="button" class="btn-icon dz-browse" data-component="browse">选择文件</button>
    <!-- 自定义构建开了 pick 特性时才有： -->
    <button type="button" class="btn-icon dz-pick" data-component="pick"
            data-pick="file" aria-label="用系统选择器挑本机文件">本机</button>
    <button type="button" class="file-chip" id="chip-file" hidden></button>
  </div>
</div>
```

---

## 三条取文件的路

| 路径 | 走哪里 | 拿到什么 | 限制 |
|---|---|---|---|
| 拖拽 / 「选择文件」 | `POST /upload` | 服务端**副本**路径 | 受 `data-max-upload` 限制（默认 200 MB） |
| 「本机」 | `POST /pick` | 磁盘上的**原始**路径 | 不复制、不限大小（服务端弹系统对话框） |
| 手填 | 无请求 | 用户自己的字符串 | 无 |

三条路都只往同一个 `#field-<arg>` 回填，所以命令侧永远只看 `--file`。

**为什么要有「本机」**：浏览器上传要把整个文件读成 base64 再发，200 MB 的文件在页面里
既吃内存又慢。能直接引用本机路径时，就不该上传副本。

---

## 状态

| 态 | 表现 |
|---|---|
| empty | 虚线 `--control` + 提示语 `.dz-hint` |
| hover | 虚线压深一档，文字转 `--ink` |
| drag-over | 虚线转实线、`--accent` 边框 + `--accent-soft` 底 |
| busy（上传中 / 等系统框） | `.dz-status.busy` 一行 `--warn` 文案 |
| success（`.uploaded`） | 实线 accent 边 + chip 出现 + 提示语让位（`.dz-hint` 隐藏） |
| error | `.dz-status.err` + 整个区 `:has(.dz-status.err)` 转 `--danger` |
| focus | 区内每个控件各自 `:focus-visible`，容器本身不接收焦点 |

状态行永远独占一行（`flex: 1 1 100%`）：上传中的话术最长，挤在行里读不全。

---

## 令牌

`--accent`（`--accent-soft`）、`--control`、`--warn`、`--ok`、`--danger`、`--ink-muted`、
`--r-panel`（容器 12px）、`--r-pill`（chip）、`--s-2/s-3/s-4`。

---

## 无障碍

- 拖拽区是**容器不是按钮**：里面有三个各自能点的控件。外层不许挂 `role="button"` 或 `tabindex`
  —— 那会把一次点击变成两个动作，读屏也数不清有几个控件。
- 键盘可达由「选择文件」这个真按钮负责；chip 是 `<button type="button">`，可 Tab、可回车清除。
- 状态行 `role="status" aria-live="polite"`，并用 `aria-describedby` 挂在路径框上，
  上传结果不必聚焦也能读到。
- chip 的 `aria-label` 必须写清「清除已上传的 <文件名>」，不能只有文件名。
- 超限报错要说**字节**：「209,715,201 B」而不是「200.0 MB」，否则会出现
  「200 MB 超过 200 MB」这种自相矛盾的话。

---

## 别做

- 不要给 `.dropzone` 加 `role="button"`；
- 不要在 JS 里复制一份上传上限（上限只有一个来源：服务端注入的 `data-max-upload`）；
- 不要在点击白区时触发两次文件框（点击分流要排除 `button / input / .dz-status`）；
- 不要用 `dragleave` 无条件移除 `drag-over`（扫过子元素会一直闪，要判 `relatedTarget`）；
- 不要把「本机」按钮画在没开 `pick` 特性的构建里 —— 点了只会得到一句报错。

---

## 源码

- 结构：`render.rs::widget_path` / `dropzone` / `pick_button`
- 行为：`index.html` 的 `initDropzone` / `initPickButton` / `uploadFile` / `clearUpload`
- 样式：`app.css` 的 `.dropzone` 一族（约 325–388 行）
- 设计表：`DESIGN.md` §10.2