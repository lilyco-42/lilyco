# Graphite 操作面清点（AI 可操作消息总线）

> P0 spike 产物。数据来源：graphite `ab32a6047650996725263b132806c171dc83097b`
> （2026-09-19 main，仓库 `editor/src/messages/` 244 个 .rs 文件）。
> 统计方法：解析全部 `#[impl_message(...)] pub enum` 树，按 Message 根的直接子域聚合
> （子域变体数 = 自身 + 全部后代枚举变体之和；`NoOp`/`Batched` 等根枚举控制变体另计）。

## 1. 总量

| 指标 | 数值 |
| --- | --- |
| 消息枚举（`*Message` + 根 `Message`） | **51 个** |
| 可派发变体总数（20 个顶层域聚合） | **815 个** |
| 出站（`FrontendMessage`，内核 → 前端，观测面） | 88 个 |
| 入站（其余 19 个域，操作面） | **727 个** |

> 历史评估（docs/GRAPHITE_EVALUATION.md）写 "约 405 变体"，当前 main 已增长到 815——
> 消息总线作为操作面的判断不变，且覆盖面比预想更大。

## 2. 顶层域分组（agg = 含子枚举聚合，self = 枚举自身变体）

| 域 | agg | self | 位置 | AI 可操作性评估 |
| --- | --- | --- | --- | --- |
| Portfolio | 342 | 60 | `portfolio/` | ★★★ 文档生命周期/多文档/字体/资源，核心操作面 |
| Tool | 238 | 43 | `tool/` | ★★★ 14 个工具子域，指针模拟即画图 |
| Frontend | 88 | 88 | `frontend/` | ★★ 出站观测面（AI 的 "眼睛"） |
| Dialog | 29 | 17 | `dialog/` | ★ 弹窗管理，AI 需要能关掉阻塞弹窗 |
| ColorPicker | 18 | 18 | `color_picker/` | ★ 取色器 |
| Preferences | 14 | 14 | `preferences/` | ★★ 偏好（save 格式等） |
| AppWindow | 13 | 13 | `app_window/` | ★ 窗口壳层，headless 无关 |
| Clipboard | 12 | 12 | `clipboard/` | ★ 复制粘贴 |
| KeyMapping | 10 | 2 | `input_mapper/` | ☆ 键位映射 |
| Animation | 9 | 9 | `animation/` | ★★ 帧驱动（headless flush 依赖它） |
| Broadcast/Event | 9 | 3 | `broadcast/` | ★★ 事件总线（点击/拖拽事件入口之一） |
| InputPreprocessor | 9 | 9 | `input_preprocessor/` | ★★★ 指针/键盘原始输入（画图关键路径） |
| Layout | 7 | 7 | `layout/` | ★ UI 布局 |
| Defer | 5 | 5 | `defer/` | ★ 图求值后回调 |
| Debug | 4 | 4 | `debug/` | ★ 消息日志 |
| Future | 2 | 2 | `future/` | ★ 异步消息（save 的底层通道） |
| ResourceStorage | 2 | 2 | `resource_storage/` | ★★ 资源注入 |
| Viewport | 2 | 2 | `viewport/` | ★★ 视口尺寸（导出/渲染相关） |
| MenuBar | 1 | 1 | `menu_bar/` | ☆ |
| Network | 1 | 1 | `network/` | ☆ |

## 3. 两大核心子树细分

**Portfolio → Document（agg 265）** —— 文档结构操作：

| 子域 | self | 说明 | AI 可操作 |
| --- | --- | --- | --- |
| DocumentMessage | 102 | 新建/删层/对齐/翻转/保存/导出触发 | T0–T1 分级逐个定 |
| NodeGraphMessage | 90 | 建节点/连线/改输入/事务 | ★★★（AI 画图最高杠杆：直接建图） |
| GraphOperationMessage | 36 | 结构性修改（OpacitySet/BlendingFillSet…） | ★★★ |
| NavigationMessage | 20 | 平移缩放旋转 | ★ |
| DataPanelMessage | 6 | 数据面板 | ★ |
| ResourceMessage | 6 | 资源加载 | ★ |
| OverlaysMessage | 3 | 覆盖层绘制 | ☆ |
| PropertiesPanelMessage | 2 | 属性面板刷新 | ☆ |

**Tool（agg 238，14 个工具子域）** —— 指针模拟画图：

| 子域 | self | | 子域 | self |
| --- | --- | --- | --- | --- |
| Path | 37 | | Pen | 18 |
| TransformLayer | 20 | | Gradient | 17 |
| Select | 16 | | Shape（矩形/椭圆/多边形） | 15 |
| Text | 14 | | Spline | 13 |
| Fill | 8 | | Freehand | 8 |
| Brush | 7 | | Eyedropper | 7 |
| Artboard | 9 | | Navigate | 6 |

## 4. 本 spike 已接通的操作（最小闭环）

| lilyco 工具 | 安全级 | 消息链 |
| --- | --- | --- |
| `graphite-doc-new` | T0 | `PortfolioMessage::Init` → `PortfolioMessage::NewDocumentWithName { name }` |
| `graphite-add-rect` | T0 | `ToolMessage::SelectWorkingColor { color, primary: true }` → `ToolMessage::ActivateToolShapeRectangle` → `InputPreprocessorMessage::PointerMove/PointerDown/PointerMove/PointerUp`（矩形拖拽）→ `AnimationMessage::IncrementFrameCounter`（flush） |
| `graphite-set-fill` | **T1** | `DocumentMessage::SetFillForSelectedLayers { fill: 0..=1 }`（选中图层须存在） |
| `graphite-save` | T0 | `DocumentMessage::SaveDocument` → 轮询 `Message::NoOp` 回收异步结果 → 断言 `FrontendMessage::TriggerSaveDocument { name, content }` |

观测面：`GraphiteHost::send` 返回全部 `FrontendMessage`；`node_count()` 经
`TriggerSaveDocument.content` 的 JSON 路径 `/network_interface/network/nodes` 统计。

## 5. P1 推荐接入顺序（AI 画画 agent 视角）

1. **T0 直通面**：`NodeGraphMessage`（建节点 90 变体）+ `GraphOperationMessage`（36）——
   直接操作节点图比模拟指针更稳、可批量、可并行；这是 "AI 画画" 的主通道
2. **T0**：`DocumentMessage::Export` 系列（PNG/SVG 导出需先验证 headless 渲染，见 §6）
3. **T1**：删除/覆盖类（`DeleteSelectedLayers`、`RemoveArtboards`）——破坏性操作
4. **T2**：批量文件读写（`OpenDocumentFile`、工作副本挂载）
5. **出站订阅**：`FrontendMessage` 88 变体按需白名单（图层结构、属性、错误诊断）

## 6. 已知边界（spike 结论）

- **渲染/GPU**：本 spike 全链不触 GPU（wgpu 经 vello 仍被编译进依赖树，但未初始化设备）。
  PNG 导出路径 = `PortfolioMessage::SubmitDocumentExport { … }`（经全局 `NODE_RUNTIME`
  泵 `node_graph_executor::run_node_graph().await` + `poll_node_graph_evaluation` 回收
  `RenderOutput`）——官方测试可跑，但 wgpu 设备在无 GPU CI 上的行为未验证，
  列为 P1 首个验证点
- **单宿主**：`Editor::ENVIRONMENT` 是 OnceLock，每进程只能建一个 Editor；
  多工具共享实例（`shared_host()`），"活动文档" 是全局态，并发驱动需串行化
- **可见性**：`dispatcher.message_handlers.*` 均为 `pub(crate)`，外部无法直读文档状态；
  断言/观测只能走 `FrontendMessage` 或序列化产物——这反而强化了 "消息总线即操作面" 的架构判断
