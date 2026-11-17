# 教程

> 从空 crate 写到一个四端可跑、能交给 agent 调用的工具。
> 结构参照 [The Rust Programming Language](https://doc.rust-lang.org/book/)：
> 每章一个可运行增量，读完就能跑；每章结尾有小结与练习。

---

## 这套教程怎么读

**不要一口气读完再动手。** 每一步都照抄、跑通、再改一点，然后再往下。

```text
读一节 → 抄代码 → cargo run → 对照「现在你应该看到」→ 改动一个值再看 → 下一节
```

教程里所有示例都在本机真编译、真运行过；贴出的输出是实测结果（文件字节数这类会随文件变化的值除外）。
如果某一步和你看到的不一致，先按该章的「常见偏差」排查，不要跳过去。

---

## 贯穿项目：`filecount`

七章共用同一个项目，能力逐章长大：

| 章 | 你会加上什么 | 结束时能做什么 |
|---|---|---|
| [1 · 第一个命令](01-hello-schema.md) | 一个 struct + 一个函数 | `--help` / `--schema` 能看到参数 |
| [2 · 参数就是界面](02-args-are-ui.md) | 类型、默认值、区间、必填 | 四种端都拿到正确的参数形状 |
| [3 · 让长任务看得见](03-progress-and-cancel.md) | 进度、日志、取消 | `--json-stream` 输出完整事件流 |
| [4 · 一个二进制多个命令](04-multi-command.md) | 注册表 + 别名 | `--gui` 出现命令下拉，`?cmd=` 可切换 |
| [5 · 让页面像产品](05-make-it-product.md) | 令牌、主题、响应式 | 换肤不动代码，验收跑测试闸门 |
| [6 · 给 agent 用](06-agent-surface.md) | MCP 契约 | agent 能 `tools/list` 并真的调用一次 |
| [7 · 发布与验收](07-ship-and-verify.md) | release、跨平台、清单 | 交付一个别人能装、能验的二进制 |
| [8 · 声明组件，而不是画组件](08-view-components.md) | `Component` 协议 + `#[view]` | 后端只声明组件，前端（GUI/shadcn）各自渲染 |

附录（按需查，不必顺序读）：

- [A · 注解速查](../appendix/A-attributes.md) —— `#[app]` / `#[arg]` 的全部属性与类型映射
- [B · 常见报错](../appendix/B-errors.md) —— 9 条真实报错原文、原因与修法
- [C · 术语表](../appendix/C-glossary.md)
- [D · 排错索引](../appendix/D-troubleshooting.md) —— 按症状查

---

## 先决条件

| 项 | 要求 | 检查 |
|---|---|---|
| Rust | 1.75+（edition 2021） | `rustc --version` |
| cargo | 随 rustup 安装 | `cargo --version` |
| 本仓库 | 有 `/home/lilyco/code/lilyco` 可读 | `ls lilyco/Cargo.toml` |
| 浏览器 | 能开 `http://127.0.0.1:*` | 第 1 章会用到 |

不需要 Node、Python、Docker。Web 端是内嵌单页，不依赖任何前端工具链。

---

## 验证记录

每章的代码与输出都实跑过。构建命令、退出码、四端形态、六类控件计数、MCP 三步、
附录 B 九条报错的复现命令，以及**哪些没验、为什么**，都在
[验证记录](VERIFICATION.md)。

---

## 每章的固定结构

| 段落 | 作用 |
|---|---|
| 本章你会学到 | 一张 3–5 条的清单，读完可以逐条自查 |
| 正文 | 每节一个小目标，代码在前、解释在后 |
| 现在你应该看到 | 真实输出或界面形态，不一致就是有问题 |
| 小结 | 3–5 条要点，不引入新知识 |
| 练习 | 2–3 道，附提示；答案不在文档里，跑出来即验证 |

---

## 与其它几篇的分工

| 你想 | 去哪 |
|---|---|
| 知道这套页面为什么长这样 | [入门介绍](../01-introduction.md) |
| 改颜色 / 间距 / 断点 | [设计语言](../03-design-language.md) + [令牌参考](../04-tokens.md) |
| 查某个控件的结构与状态 | [组件参考](../components/README.md) |
| 用 React + shadcn 做另一个前端 | [React 映射](../05-react-shadcn-mapping.md) |

教程只讲「怎么把东西做出来」；规则与数值的解释在上面这几篇。

---

## 一页速览（给已经会的人）

只想快速抄一份骨架，看 [速览版](../02-tutorial.md)；想逐章学会，从 [第 1 章](01-hello-schema.md) 开始。