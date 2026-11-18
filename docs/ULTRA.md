# Ultra 可视化编排（总览）

> 三个零依赖页面，共用 `lilyco-ultra-ui` 一个进程：
> 节点图（`/graph`）、Axure 式原型设计器（`/designer`）、Scratch 式积木编程（`/blocks`）。
> 三者互不等同，各自解决一种「编排」。

## 1. 一键启动

```bash
bash scripts/ultra-demo.sh                 # 默认 8090
LILYCO_PORT=8190 bash scripts/ultra-demo.sh
```

等价于：

```bash
cargo run -p lilyco-ultra-ui --example serve -- 8090
```

启动后：

| 页面 | 地址 | 形态 |
|---|---|---|
| 原型设计器 | `http://127.0.0.1:8090/designer` | Axure 式：13 组件拖入画布、图层/属性/预览、导出 React |
| 积木编程 | `http://127.0.0.1:8090/blocks` | Scratch 式：7 个积木垂直咬合、可嵌套、导出 JSON/JS |
| 节点图 | `http://127.0.0.1:8090/graph` | LiteGraph：端口连线、组拖动/撤销/执行/持久化 |
| JSON → React | `http://127.0.0.1:8090/` | 原始 builder：JSON 编辑 + 实时预览 |

## 2. 各页要解决什么

### /designer（原型）

- 13 种组件：标题/文本/按钮/输入框/数字框/选择框/复选框/文本域/图片/分隔线/进度条/链接/计算器
- 拖动、右下角缩放、8px 吸附、框选、Shift 加选、图层 ↑↓、属性（文本/宽高/颜色）
- 画布 zoom、预览 iframe、撤销重做、保存到 `~/.lilyco/designer.json`
- 导出 React HTML（复用 `spec → generator`）与 JSON（可再导入）
- 细节：[ULTRA-DESIGNER.md](ULTRA-DESIGNER.md)

### /blocks（积木）

- 7 个积木：当开始、当点击、运行命令、设置变量、显示文本、如果…则、重复 N 次
- 垂直咬合、拖进凹槽嵌套、改参数实时同步右侧 JS
- 导出 JSON 树与可读 JS（`for (let i = 0; i < 5; i++) { await run("count"); }`）
- 细节：[ULTRA-BLOCKS.md](ULTRA-BLOCKS.md)

### /graph（节点图）

- vendored LiteGraph 0.7.18（MIT，内嵌 `/assets/litegraph.js`，零外链）
- 节点=命令、端口=参数、边=pointer 绑定；分层执行 `count → echo`
- 组拖动、撤销重做、对齐线、缩放平移、全键盘、后端持久化 `~/.lilyco/graph.json`
- 细节：[ULTRA-GRAPH.md](ULTRA-GRAPH.md)

## 3. 发布物

- Release `ultra-v1`：源码包 + 使用说明 + `SHA256SUMS`
- 在线文档：<https://lilyco-42.github.io/lilyco/> 的「可视化编排」章节
- 本仓库三篇细节文档：`docs/ULTRA-{DESIGNER,BLOCKS,GRAPH}.md`

## 4. 验收与已知限制

- 三页 CDP 审计：`/graph` 22 项、`/designer` 21 项、`/blocks` 7 项，均全 PASS；页面外部请求 0。
- Rust：`cargo test -p lilyco-ultra-ui` 17 + 3 + 12 passed。
- **headless 限制**：CDP 键盘事件不可达（`hits=0`）、依赖重绘时序的 `.align` 参考线 DOM 计数为 0；这两类用页面内等价调用验证，真实浏览器快捷键/参考线需目检。
- 设计器导出的 React 页按既有设计引 unpkg（带 SRI）；**设计器/积木/节点图页面本身零外链**。