# Ultra 原型设计器

> Android Layout Editor 式原型工具：组件库 → 画布 → 约束锚点 → 设备预览 → 导出 React。
> 底座是 vendored 的 **GrapesJS 0.22.5**（BSD-3-Clause，UMD 单文件），在设计器之上扩展双视图、
> 组件树、约束锚点、设备帧、对齐/分布与登录页模板。页面**零外链**；产物复用 `spec → generator`。

```bash
LILYCO_UI=ultra LILYCO_PORT=8090 cargo run -p <你的载体>
# 打开 http://127.0.0.1:8090/designer
```

## 1. 架构与底座

| 层 | 实现 |
|---|---|
| 编辑器内核 | GrapesJS 0.22.5，`lilyco-ultra-ui/assets/grapesjs.min.js`（1.1 MB，保留 BSD 头） |
| 加载方式 | `include_str!` 编译期内嵌，`GET /assets/grapesjs.min.js` 返回，无 CDN、无构建链 |
| 设计器扩展 | 页面内脚本：组件块、锚点约束、蓝图视图、设备帧、对齐/分布、模板、序列化 |
| 导出链路 | `DesignDoc` → `spec`（`布局: "absolute"` + `位置[]`）→ `generator` → React HTML |
| 离线约束 | `canvas.styles/scripts = []`、`cssIcons = ''`，审计实测外部请求 0 |

## 2. 界面

```text
┌────────┬──────────────────────────┬───────────────┐
│ 组件库  │  画布（设备帧 390/820/600） │  属性 / 图层    │
│ 14 块   │  设计视图 ↔ 蓝图视图        │  约束与样式     │
└────────┴──────────────────────────┴───────────────┘
```

| 区域 | 作用 |
|---|---|
| 组件库 | 14 种组件，点击添加或拖到画布 |
| 画布 | 设备帧 + 绝对定位；GrapesJS 负责拖动/缩放/框选 |
| 属性 | GrapesJS Style Manager，按选中元素显示 |
| 图层 | GrapesJS Layer Manager，展示容器嵌套树 |
| 顶栏 | 设计/蓝图、设备选择、模板、对齐、分布、保存、加载、预览、导出 |

## 3. 组件（14 种）

| 类型 | 默认 props | 尺寸 |
|---|---|---|
| 标题 | `{内容, 级别}` | 280×48 |
| 文本 | `{内容}` | 280×44 |
| 按钮 | `{文本, 动作, 样式}` | 140×46 |
| 输入框 | `{标签, 变量, 占位符}` | 280×74 |
| 数字框 | `{标签, 变量, 默认}` | 280×74 |
| 选择框 | `{标签, 变量, 选项}` | 280×74 |
| 复选框 | `{标签, 变量, 默认}` | 180×40 |
| 文本域 | `{标签, 变量, 行数}` | 280×110 |
| 图片 | `{链接, 宽度}` | 220×140 |
| 分隔线 | `{}` | 280×14 |
| 进度条 | `{进度, 标签}` | 280×46 |
| 链接 | `{文本, 链接}` | 120×32 |
| 计算器 | `{变量, 模式}` | 220×64 |
| 卡片 | `{标题}` | 360×480（容器，可嵌套） |

## 4. 交互

| 操作 | 行为 |
|---|---|
| 点击组件库 / 拖到画布 | 创建元素；拖入点决定初始位置 |
| 设计视图 ↔ 蓝图视图 | 蓝图只保留虚线轮廓、尺寸与锚点，隐藏内容；画布中线为约束参考 |
| 设备选择 | 手机 390×844 / 平板 820×1180 / 自定义 600×900，同步改画布帧尺寸并重算锚点 |
| 约束锚点 | 水平 `left/center/right` × 垂直 `top/center/bottom`，选中元素后设置 |
| 左对齐 | 未选中时作用于当前层级兄弟；选中时只作用于选中项 |
| 水平分布 | 同级 ≥3 个元素，首尾固定、中间等距 |
| 登录页模板 | 「卡片」容器包 6 个元素：标题、说明、邮箱、密码、登录按钮、错误提示 |
| 预览 | 顶栏「预览」用 `editor.getHtml()+getCss()` 写入 iframe，`srcdoc` 实时渲染 |
| 保存 / 加载 | `POST /api/designer/save`、`GET /api/designer/load`（`~/.lilyco/designer.json`） |
| 导出 HTML | 后端 `DesignDoc → UiSpec → generator` 输出 React 页面 |
| 导出 JSON | 下载 `prototype.design.json`，可再由 `applyDesign` 载入 |

GrapesJS 自带 `Ctrl+Z / Ctrl+Shift+Z` 撤销重做与 `Delete` 删除。

## 5. 约束模型

- 锚点编码在元素属性上：`data-anchor="center-top"`，格式 `水平-垂直`。
- 约束求解在 `applyAnchors()`：递归遍历组件树，子元素以父容器宽高为可用区。
  - `left` → `20px`；`center` → `floor((可用宽 - 元素宽) / 2)`；`right` → `可用宽 - 元素宽 - 20`
  - `top/bottom/center` 同理使用可用高
- 触发时机：载入模板、切换设备（160 ms 后重算）。**导入快照不重算**，保证往返一致。
- 可视化：每个组件左上角有锚点圆点（`::before`），蓝图视图另有画布中线虚线。

## 6. 数据格式（唯一契约）

设计稿（`~/.lilyco/designer.json`）：

```json
{
  "title": "Lilyco 原型",
  "size": "medium",
  "elements": [
    { "id": "按钮1", "type": "按钮", "x": 24, "y": 32, "w": 120, "h": 44,
      "color": "#2563eb", "anchor": "left-top",
      "props": { "文本": "确定", "动作": "ok", "样式": "primary" } }
  ]
}
```

`elements` 始终是**扁平绝对坐标**：容器（卡片）本身不导出，子元素按「父绝对 + 子相对」累加成绝对坐标。
这样后端 spec 无需理解嵌套，导入导出也保持稳定。

导出转成 UiSpec 的绝对定位模式（流式 JSON 仍兼容）：

```json
{ "窗口": { "标题": "…", "大小": "medium", "布局": "absolute",
  "元素": [{ "类型": "按钮", "文本": "确定", "动作": "ok", "样式": "primary" }],
  "位置": [{ "x": 24, "y": 32, "w": 120, "h": 44, "color": "#2563eb" }] } }
```

导出前按类型合并 `DEF_PROPS`，避免 `spec::ElementSpec` 缺必填字段导致 400。

## 7. API

| 路由 | 作用 |
|---|---|
| `GET /designer` | 设计器页面 |
| `GET /assets/grapesjs.min.js` | vendored 内核（immutable 缓存） |
| `POST /api/designer/save` | 保存 DesignDoc 到 `~/.lilyco/designer.json` |
| `GET /api/designer/load` | 读取（404 = 尚无保存） |
| `POST /api/designer/export` | 返回 `{ ok, spec, html }`，`html` 为 React 页面 |

## 8. 实测（CDP，17 项全 PASS）

```text
设计器初始化（GrapesJS）
页面无外链请求/失败  外部=0 失败=0
无未捕获 JS 异常
组件库 14；模板 7 节点（卡片 + 6 元素）
组件树嵌套 [["卡片", 6]]
设备切换 386×150 → 816×150
约束锚点重算 13px → 220px；锚点属性 cross-device 保持 center-top
蓝图视图 canvas body.blueprint
左对齐子元素 left 一致 [22,22,22,22,22,22]
水平分布间距均匀 [0,0,0,0,0]
导出 DesignDoc 6 元素；导出 HTML 8856B 且含「登录」
保存/加载往返一致 6 → 6
导出 → 再导入一致 6 元素
预览 iframe 有内容
```

Rust 侧：`cargo test -p lilyco-ultra-ui` 覆盖 DesignDoc 转换、JSON 键、错误定位、空稿 4 组用例。

审计方式：headless Chromium + CDP（`Runtime.evaluate` / `Network` / `Runtime.exceptionThrown`），逐项断言。
另测 `/graph`（LiteGraph）与 `/blocks`（Scratch 积木）未受本轮影响。

## 9. 已知限制

- **headless 下 CDP 键盘不可达**：`Delete`、方向键、`Tab` 焦点等快捷键需真实浏览器验证；
  自动化审计改用页面内等价调用（`alignLeft`/`distributeH`/`toggleBlueprint`）。
- **蓝图视图**只表达结构与锚点，不渲染真实交互样式；与导出产物在细节上略有差异。
- 导出的 React 页面沿用既有设计，引 `unpkg` 的 React 18 + Babel（带 SRI）；
  **设计器页面本身零外链**。
- 卡片容器只做定位与嵌套展示，导出的 UiSpec 仍是绝对定位扁平结构，不生成容器组件。
- 旧 `/`、`/graph`、`/blocks`、`/api/render`、`/api/validate` 均已回归通过。