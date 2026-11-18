# Ultra 积木编程

> Scratch 式积木：事件 / 动作 / 条件 / 循环，垂直咬合、可嵌套，导出 JSON 与可读 JS。
> 与 `/designer`（Axure 原型）、`/graph`（节点图）并列，三者共用 ultra 的零依赖页面风格。

```bash
LILYCO_UI=ultra LILYCO_PORT=8090 cargo run -p <你的载体>
# 打开 http://127.0.0.1:8090/blocks
```

## 1. 三种编排工具的分工

| 页面 | 形态 | 适合 |
|---|---|---|
| `/designer` | 画布 + 绝对定位 UI 元素 | Axure 式界面原型，导出 React |
| `/graph` | 节点 + 端口连线（LiteGraph） | 数据流/命令图，看依赖与分层执行 |
| `/blocks` | 积木垂直咬合（本页） | Scratch 式逻辑：事件触发、顺序动作、条件、循环 |

## 2. 积木（7 个，四类）

| 类别 | 积木 | 参数 |
|---|---|---|
| 事件 | 当 开始 | — |
| 事件 | 当 点击 | `target` |
| 动作 | 运行命令 | `cmd`（count / echo / compress） |
| 动作 | 设置变量 | `name`、`value` |
| 动作 | 显示文本 | `text` |
| 条件 | 如果 … 则 | `name`、`op`（> < == !=）、`value`，容器 |
| 循环 | 重复 N 次 | `times`，容器 |

## 3. 操作

| 操作 | 行为 |
|---|---|
| 从左侧积木库拖到脚本区 | 追加到末尾 |
| 拖到某个积木的上/下半区 | 插入到该积木前/后 |
| 拖到「如果/重复」的凹槽里 | 成为子积木（嵌套） |
| 积木内输入框 | 实时改参数，右侧 JS 同步 |
| 清空 | 重置脚本 |
| 导出 JSON | `blocks.json`（树结构，可再载入） |
| 导出 JS | `blocks.js`（可读脚本） |

## 4. 生成示例

积木：

```text
当 ▶ 开始
重复 5 次
  运行命令 count
```

生成 JS：

```js
// 当开始
for (let i = 0; i < 5; i++) {
  await run("count");
}
```

导出 JSON：

```json
{
  "children": [
    { "type": "when_start", "props": {}, "children": [] },
    { "type": "repeat", "props": { "times": 5 },
      "children": [{ "type": "run_command", "props": { "cmd": "count" }, "children": [] }] }
  ]
}
```

## 5. 实测（CDP，7 项全 PASS）

```text
积木页无外链请求/失败          外部=0  失败=0
积木库 7 个（事件2/动作3/条件1/循环1）
拖入「当开始」               blockCount=1
拖入「重复」                 blockCount=2
积木可嵌套（重复→运行命令）   loop.children=["run_command"]
生成 JS 含循环次数与命令调用  for (let i = 0; i < 5; i++) { await run("count");
导出 JSON 结构正确           ["when_start","repeat"]
```

Rust 侧：`cargo test -p lilyco-ultra-ui` 含 `page_has_scratch_bits` / `page_is_self_contained`（无外部 http/脚本引用）。

## 6. 已知限制

- 积木页暂不做持久化（导出 JSON 即存档）；需要时按 `designer.json` 的同款接 `/api/blocks/save|load`。
- headless 下键盘事件不可达（与 `/graph`、`/designer` 同）：快捷键需真实浏览器验证。
- 生成 JS 是「可读脚本」，尚未接真实 `run()` 执行器；接入点在 `toJs()` 与 `run_command` 积木。