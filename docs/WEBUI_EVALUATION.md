# WebUI 组件评估（lyco 预研，2026-09-19）

> 目标：为 lilyco Web 端（schema→表单 + SSE 进度）选型 UI 组件方案。
> 结论先行：**Pico.css（classless）替换 Layui + 保留 vanilla JS 架构 + 补 HTML 转义**。

## 0. 现状体检（lilyco-gui/src/lib.rs，822 行）

- 渲染：Rust `format!` 字符串模板（19 处）→ 按 6 种 `ArgKind` 生成控件 HTML
- UI 框架：**Layui 全量内嵌 480KB**（layui.css 124KB + layui.js 356KB，`include_str!` 进二进制），
  实际只用到 form/progress/card/button ≈ 5% 能力
- 交互：手写 vanilla JS（fetch POST /run + EventSource SSE + token 头），约百行，工作正常
- 安全：loopback 限定 + Origin 校验 + 随机 token（lfiles 探针已验收）；**无 HTML 转义函数**
  —— `value="{...}"` 等插值点裸拼，当前内容是开发者 schema（风险低）但模式脆弱，任何方案都应先补
- 响应式：@media 640px 手写适配 ✓

## 1. 候选活跃度取证（gh api，2026-09-19）

| 候选 | ⭐ | 最后推送 | 许可 | 状态 |
|---|---|---|---|---|
| htmx | 49,483 | 2026-09-18 | 自定义 BSD | 极活跃 |
| Alpine.js | 31,936 | 2026-09-15 | MIT | 活跃 |
| Layui | 30,572 | 2026-09-01 | MIT | 社区维护 |
| Pico.css | 16,860 | 2026-05-09 | MIT | 稳定 |
| petite-vue | 9,703 | **2024-07** | MIT | **停更** |
| Leptos/Dioxus | — | — | MIT/Apache | 活跃但需 wasm 工具链 |

## 2. Fit Matrix（R1 单二进制无构建链 · R2 动态表单 · R3 SSE 进度 · R4 中文/移动端 · R5 安全 · R6 体积 · R7 维护≈0）

| 方案 | R1 | R2 | R3 | R4 | R6 | R7 | 判定 |
|---|---|---|---|---|---|---|---|
| A. 现状 Layui 内嵌 | ✓ | ✓ | ✓ | ✓ | **✗ 480KB/95%闲置** | ✓ | 退役 |
| **B. Pico.css + 现有 vanilla JS** | ✓ | ✓（语义 HTML 即样式） | ✓（JS 零改动） | ✓（自带响应式+暗色） | **✓ −460KB** | ✓ | **✅ 采纳** |
| C. htmx（SSE 扩展） | ✓ | ~（需改 HTML partial 范式） | ✓ | ✓ | ✓ | ~ | 备选（范式迁移 payoff 低） |
| D. Alpine / petite-vue | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 出局（vanilla 已覆盖，petite 停更） |
| E. Leptos/Dioxus（wasm） | ✗ 工具链 | ✓ | ✓ | ✓ | ✗ +1-2MB | ✗ | 出局（违背单二进制分发） |
| F. 纯自研 CSS | ✓ | ✓ | ✓ | ~自养 | ✓✓ | ✗ | 备选（Pico 不满足时退此） |

## 3. 决策（build-vs-buy）

**采纳 B**：`lilyco-gui` 本质是"一张动态表单 + 一条进度流"，Pico 的 classless 语义样式
直接命中，JS 层零改动、无范式迁移；Layui 退役（git 历史保留）。htmx 留作未来若出现
多页面/局部刷新需求的升级路径。

**先决修复（任何方案都需要，P0 级）**：`lilyco-gui` 补 `html_escape()` 并应用到全部
插值点（about/name/default/list 值/遥测输出区）。

## 4. 最小验证路径（一个 PR）

1. assets：删 layui.css/layui.js，嵌 pico.min.css（~14KB gz，MIT）
2. 模板去 `layui-*` class → 语义标签；进度条用原生 `<progress>`
3. 补 `html_escape()` + 全插值点
4. 验收：web_probe.py 全过（首页下拉 / ?cmd= / CSRF 401 / SSE / 逐字比对）+ CI 绿
   —— 注意与 lfiles 域（并行工作线）的探针协同，迁移前先对齐其验收脚本
