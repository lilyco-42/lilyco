# Ultra UI：图编程（节点图）可行性与架构

> 结论：**能做**，而且 lilyco 已经有 2/3 的积木。
> `Registry` 天然是「节点表」，`CommandSchema` 是「端口签名」，`Component` 是「节点输出」；
> 缺的是图模型、拓扑执行、画布——v0 已补上前两件的一半与一个只读画布。

---

## 1. 架构映射

| 图编程概念 | lilyco 现有件 | 状态 |
|---|---|---|
| 节点 | `RegisteredCommand`（一个命令 = 一个节点） | ✅ 已有 |
| 端口（输入） | `CommandSchema.args`（名字、类型、必填、区间） | ✅ 已有 |
| 端口（输出） | handler 返回的 `serde_json::Value` | ✅ 已有 |
| 节点视图 | `Component`（52 变体，后端声明） | ✅ 已有 |
| 连线 | `Edge { from, to, arg, pointer }` | ✅ v0 新增 |
| 拓扑执行 | `graph::plan()` 分层 + 未来接 `executor::spawn` | 🟡 计划有，执行未接 |
| 画布 | `/graph` 只读 SVG（分层自动布局） | 🟡 v0 只读 |
| 类型检查 | 现在只查「参数存在」，未查类型兼容 | ❌ 待做 |

v0 的模型（`lilyco-ultra-ui/src/graph.rs`）：

```json
{
  "nodes": [
    { "id": "count", "command": "count", "args": { "steps": 3 } },
    { "id": "echo",  "command": "echo",  "args": {} }
  ],
  "edges": [
    { "from": "count", "to": "echo", "arg": "text", "pointer": "/value" }
  ]
}
```

`pointer` 是 JSON Pointer：从上游结果里取值再绑到下游参数。
`plan()` 返回 `order` 与 `stages`——**同层节点可并行**。

---

## 2. 成熟方案：借什么、不借什么

| 方案 | 核心机制 | 借 | 不借 |
|---|---|---|---|
| **ComfyUI** | 节点=纯函数，端口有类型，图按拓扑序 + 缓存执行 | 端口类型、分层执行、节点即函数 | 巨型节点生态、Python 运行时 |
| **n8n / Node-RED** | 触发器 + 数据流 + 凭证/错误分支 | 触发器节点、错误端口、运行历史 | 云服务集成、凭证管理 |
| **UE Blueprint** | 执行线（白）与数据线（彩）分开 | 执行线/数据线分离、类型着色 | 引擎绑定、编译期字节码 |
| **Scratch / Blockly** | 积木拼装，面向教学 | 低门槛交互、即时反馈 | 生成代码的复杂度 |
| **React Flow** | 画布交互的事实标准（拖拽/连线/缩放） | 节点拖拽、框选、小地图 | 引入 React 运行时（内置 GUI 零依赖） |
| **LiteGraph** | 轻量节点图（ComfyUI 前身） | 紧凑的节点/端口数据结构 | 老旧的渲染实现 |
| **tldraw / Excalidraw** | 无限画布 + 手绘 | 画布手感、缩放平移 | 自由绘图（不是编程语义） |

**取舍**：内置 GUI 必须零依赖，画布用原生 SVG；ultra 是开发工具，允许后续引 React Flow 做交互版，
但**图 JSON 与后端校验必须与前端解耦**——这也是 v0 先做模型 + API 的原因。

---

## 3. v2 已交付（LiteGraph · 六项交互 · 真执行）

画布换成 **vendored LiteGraph.js 0.7.18（MIT，1.1 MB）**：`include_str!` 内嵌、`/assets/litegraph.js` 同源加载，页面零外链。

| 能力 | 实现 |
|---|---|
| 多选 + 框选 | Ctrl 框选；批量拖动（捕获阶段记录选中组，组内跟随并在 pointerup 补最后一帧） |
| 缩放 / 平移 | LiteGraph 原生滚轮、中键/空格；`适应视图` 按节点包围盒计算 |
| 撤销 / 重做 | JSON 快照栈（≤60）；Ctrl+Z / Ctrl+Shift+Z |
| 连线命中 + 对齐线 | 输出端口拖拽 → 40px 几何命中；拖动时与其他节点 x/y 差 <8px 画虚线 |
| 全键盘 | Tab 切选中、方向键 8px（Shift 32px）、Delete 删除、Esc 取消连线 |
| 自动布局 + 持久化 | `POST /api/graph/layout` 按 stages 排布；localStorage 恢复 `x/y` |
| 执行 + 历史 | `POST /api/graph/run` 按 stages 跑、`pointer` 注入；节点转绿、最近 5 次历史 |

CDP 实测（真实鼠标/键盘事件，19 项全 PASS）：

```text
无外链请求/失败            外部=0  失败=0
Ctrl 框选                  selected=2
批量拖动                   [83,42] / [83,42]（同位移）
滚轮缩放                   scale 1.2 → 1.32
中键平移                   offset 变化
适应视图                   scale=1.2，全节点可见
Ctrl+Z / 重做              坐标回退 → 恢复
拖线建边                   {from:count,to:echo,arg:text}
对齐参考线                 触发 31 次绘制
Tab / 方向键 / Delete      选中 → 移动 8px → 删除
自动布局 + 刷新             x/y 从 localStorage 保留
执行 count→echo            text=3（pointer=/value 注入）
运行历史                   ≥1 条
```

## 4. v1 已交付（拖拽式）

- **拖节点**：pointer capture + 8px 网格吸附，坐标写回 JSON 的 `x` / `y`
- **拉线**：从节点右侧输出端点拖到左侧参数圆点（输入端口来自 `GET /api/graph/commands`），松开建边
- **删除**：点选节点/边后 `Delete`；删节点连带删关联边
- **新增**：`+ 节点` 生成唯一 id 并落位
- **双向同步**：每次结构变更写回左侧 JSON，右侧画布按 `x/y` 或执行阶段重排

CDP 实测（真实鼠标/键盘事件，非合成 JS）：

```text
拖拽 count            → 坐标 24,24 → 144,96（8px 吸附）
点边 + Delete         → edges=0
out(count) → in(echo.text) → edge {from:count,to:echo,arg:text}
拖拽后 /api/graph/validate → valid=true
```

## 5. v0 已交付（只读）

- `lilyco-ultra-ui/src/graph.rs`：`GraphSpec` / `Node` / `Edge` / `GraphError` / `Plan`；
  `validate_with()` 查重复 id、未知命令、未知参数、自连、环；`plan()` 用 Kahn 分层。
- `lilyco-ultra-ui/src/graph_page.rs`：`/graph` 页面（JSON 编辑器 + SVG 分层画布 + 状态条）。
- 路由：`POST /api/graph/validate`、`POST /api/graph/plan`、`GET /api/graph/example`。
- 测试：7 个（线性顺序、并行分层、环、自连、未知节点/参数、重复 id、示例图）。
- 修了一个真 bug：facade 的 `ultra` feature 没带 `dep:tokio`，单开 `LILYCO_UI=ultra` 编译失败。

实测：

```text
GET  /graph                     → 200，SVG 2 节点 / 1 边
POST /api/graph/plan（示例）     → order=[count, echo], stages=[[count],[echo]]
POST /api/graph/validate（坏边） → unknown_arg: node=b, command=echo, arg=nope
```

---

## 6. 缺口与下一步

| 优先级 | 缺口 | 状态 / 做法 |
|---|---|---|
| — | 真正执行 | ✅ v2：`/api/graph/run` 按 stages 跑、pointer 注入（demo handler） |
| — | 可交互画布 | ✅ v2：LiteGraph + 组拖动 + 撤销 + 对齐线 + 全键盘 |
| — | 图持久化 | ✅ v2：localStorage（后续可加 `.lilyco-graph.json` 导入导出） |
| P0 | 类型化端口 | 用 `ArgKind` 与上游输出 JSON 形状做兼容检查（number/string/array/bool） |
| P0 | 接真 Registry | `demo_handler` 换成 `executor::spawn`（四端同一套 handler） |
| P1 | 导入/导出 | `.lilyco-graph.json`；与 `lilyco-dev new` 生成器打通 |
| P2 | 触发器与错误端口 | 定时/文件/HTTP 触发；失败边（ComfyUI 的 bypass 思路） |
| P2 | 节点视图 | 执行后把节点的 `Component` 渲染在卡片里（复用 `renderComponent`） |

**判断**：图编程对 lilyco 是「顺着现有协议加一层」，不是另起炉灶。
真正的工作量在**执行语义**（缓存、并行、错误传播）与**交互画布**，而不是数据模型。