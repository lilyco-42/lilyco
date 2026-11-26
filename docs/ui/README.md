# Lilyco UI 指南

> `--gui` 的默认 Web 入口使用 [shadcn/ui 适配器](../../lilyco-gui/web/README.md)。此目录里的设计令牌和原生控件说明对应 `/classic`。

> 一套**令牌 + 规则**的设计系统，用来做「本地开发者控制台」。
> 借 shadcn/ui 的克制与语义令牌，不借它的运行时。

---

## 这套指南解决什么

`lilyco` 的 Web 端（`--gui`）不是营销页，也不是通用后台。
它只服务一件事：**把一个命令的 schema 摊成表单，把执行过程摊成进度与日志，把结果摊成 JSON。**

所以这里的设计目标不是「好看」，而是四件更硬的事：

1. 一眼看懂「这个命令要什么参数、跑没跑完、结果在哪」；
2. 在 320px 宽的 OBS Dock 里不塌、在 1920px 宽屏上不空旷；
3. 键盘与读屏用户能完整操作；
4. 单文件内嵌进每个二进制，**零外部请求**。

---

## 先读哪一篇

| 你是 | 从这开始 | 再往下 |
|---|---|---|
| 第一次用 lilyco 写界面 | [01 · 入门介绍](01-introduction.md) | [章节教程](tutorial/README.md) |
| 想改颜色 / 字号 / 间距 | [03 · 设计语言](03-design-language.md) | [04 · 令牌参考](04-tokens.md) |
| 要加一个控件或一块面板 | [组件参考](components/README.md) | [03 · 设计语言](03-design-language.md) |
| 前后端分离，用 React + shadcn | [05 · React 映射](05-react-shadcn-mapping.md) | [04 · 令牌参考](04-tokens.md) |
| 多框架适配与 10 年期演进规划 | [06 · 10 年期 UI 抽象规范](06-ten-year-ui-architecture.md) | `lilyco-core/src/theme.rs` |

建议第一次按 01 → 02 顺序读，教程每一步都能跑起来。03 之后是工具书，按需查。

---

## 文档地图

| 章节 | 内容 |
|---|---|
| [01 · 入门介绍](01-introduction.md) | 它是什么、为什么长这样、与 shadcn 的关系、心智模型 |
| [章节教程](tutorial/README.md) | 7 章：从空 crate 到四端可跑、能交给 agent 调用（每章可运行 + 小结 + 练习） |
| [02 · 速览](02-tutorial.md) | 一页抄完的骨架与四个最常见的坑 |
| [03 · 设计语言](03-design-language.md) | 单一强调色锁、hairline 分层、8px 节奏、排版、动效、无障碍 |
| [04 · 令牌参考](04-tokens.md) | 明暗两套令牌全表、对比度实测值、改令牌的唯一路径 |
| [组件参考](components/README.md) | 六个输入控件 + 页面骨架 + 输出区，逐页给结构/状态/无障碍 |
| [05 · React 映射](05-react-shadcn-mapping.md) | 与 React + shadcn/ui 的令牌、组件、主题对照 |
| [视图组件协议](view-protocol.md) | `Component` 52 变体、事件形状、51 条 shadcn 映射 |

---

## 三条不可协商的约束

读任何一篇之前先记住这三条，它们解释了后面所有「为什么不这样做」。

| 约束 | 具体是什么 | 为什么 |
|---|---|---|
| **零外部依赖** | 无 CDN、无 webfont、无图标文件、无外链。图标只能是 Unicode 字符 | CSP 是 `default-src 'none'`，任何外链都会被浏览器拦掉；页面要内嵌进二进制 |
| **只跑回环** | 只绑 `127.0.0.1`，令牌每次启动随机，请求带 `X-Lilyco-Token` | 这是本机工具的控制面，不是公网服务 |
| **DOM 名是契约** | `__FIELDS__` 等占位符、`#field-<arg>` 等 id、`.dropzone` 等 class 都有测试盯着 | 服务端渲染与页面行为靠它们对齐，改名等于拆接口 |

---

## 唯一来源

- **数值**只有一个来源：[`lilyco-gui/DESIGN.md`](../../lilyco-gui/DESIGN.md)。本文档只叙述规则与用法，不复制第二份数值表。
- **实现**只有一个来源：`lilyco-gui/assets/app.css` 与 `lilyco-gui/assets/index.html`。
- 两者冲突时，以**代码 + 测试**为准，然后回来改文档。

改样式前请先读 [03 · 设计语言](03-design-language.md) 的「改之前先看闸门」一节：
仓库里有 10 条测试专门盯着颜色字面量、体积预算、装配顺序与无障碍结构。

---

## 快速看效果

```bash
# 模板项目已开 tui + web 特性，直接起页面
cd ../lilyco-template && cargo run -- --gui
```

只想看命令行形态（示例 crate 默认 headless，没有 `--gui`）：

```bash
cargo run -p lilyco-example -- --input photo.jpg --schema
```

自己新建 crate 时记得在依赖里写 `features = ["tui", "web"]`，见教程第 0 步。