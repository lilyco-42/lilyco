# DOMAIN-GUIDE.md — 在 lilyco 上从零造一个「给 AI 用的软件」

> 面向 AI agent 和人类开发者。读完你能把任何工具/服务做成 **CLI/TUI/Web/MCP 四端、
> 天生 AI-callable** 的 lilyco 域应用。活例：[`lilyco-letsgal`](../lilyco-letsgal)
> （故事 DSL → LetsGal Studio 工程编译器）。
>
> 配套必读：[AGENTS.md](../AGENTS.md)（硬性规则）、[CODEGRAPH.md](CODEGRAPH.md)（代码图谱）。
> 本文只讲「怎么做」；「为什么」看 AGENTS.md 每条规则的注释。

## 0. 心法：什么是「给 AI 用的软件」

人类用 GUI 看按钮；AI 用 schema 看能力。一个 AI-first 应用必须满足：

1. **能力自描述** —— `--schema` 输出机器可读的命令注册表，agent 不读源码就能调用你；
2. **结果可机器消费** —— 返回 `serde_json::Value`，人类端的漂亮展示由框架渲染，不归你管；
3. **进度可观测** —— 长任务发 `Progress` 事件流（MCP 侧变成通知，TUI 侧变成进度条）；
4. **确定性优先** —— 同输入恒同输出；需要 LLM 的环节走采样桥，别把随机性混进业务函数；
5. **一次定义，四端免费** —— 你只写业务 struct，CLI/TUI/Web/MCP 由 `#[derive(App)]` 派生。

## 1. 十分钟最小应用（单命令）

```
my-tool/
├── Cargo.toml
└── src/main.rs
```

```toml
# Cargo.toml —— 应用 crate 只依赖 facade，禁止直接依赖 lilyco-core（AGENTS.md 硬性规则 3）
[package]
name = "my-tool"
version = "0.1.0"
edition = "2021"

[dependencies]
lilyco = { path = "../lilyco" }   # 发布后用 crates.io 版本并锁版本
serde_json = "1"
```

```rust
use lilyco::prelude::*;

/// 把一句话变响（示例业务）
#[derive(App)]
#[app(name = "boom", about = "响一声", run = "run_boom")]
struct Boom {
    /// 响几声（doc comment 就是给 AI 看的参数说明）
    #[arg(default = "1")]
    times: String,
}

fn run_boom(app: &Boom, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let n: u32 = app.times.parse().map_err(|_| AppError::InvalidArg("times 不是数字".into()))?;
    let result = serde_json::json!({ "boomed": n });
    ctx.done(result.clone(), 0);          // 恒以 done 收尾
    Ok(result)
}

fn main() {
    lilyco::run::<Boom>();                // ← main 只有这一行，四端自动分发
}
```

这 30 行已经是完整产品：`my-tool boom --times 3`（CLI）、TUI 交互、
`http://127.0.0.1` Web 表单（字段自动从 struct 生成）、MCP server 里一个叫
`boom` 的 tool。**写一个 struct，四个界面**。

## 2. 多命令：让 AI 看到你的全部能力

单 struct 只有一个命令。真实应用把每个业务操作做成一个 `App` struct，
注册进 `Registry`（活例见 `lilyco-example/examples/multi.rs`）：

```rust
use lilyco::prelude::*;

#[derive(App)]
#[app(name = "build", about = "编译故事 DSL → 工程", run = "run_build")]
struct Build { /* args... */ }

#[derive(App)]
#[app(name = "validate", about = "校验工程完整性", run = "run_validate")]
struct Validate { /* args... */ }

fn main() {
    let mut registry = Registry::new();
    registry.register(RegisteredCommand::from_app::<Build>()).unwrap();
    registry.register(RegisteredCommand::from_app::<Validate>()).unwrap();
    lilyco::run_registry("lletsgal", registry);   // 四端自动分发
}
```

AI 侧从此可以 `lletsgal --schema` 拿到全部命令 + 参数说明，逐个直调。
**命名纪律**：命令名 = 动词（build/validate/init），别名给高频缩写（`.alias("b")`）；
`about` 写成「做什么 + 从什么到什么」，它是 AI 的说明书，值得花时间写。

## 3. 长任务与进度流

任何超过 ~1s 的工作都发进度。**不变量：事件流恒以 `Done`/`Error` 结尾**
（executor 有合成兜底，但别依赖兜底——AGENTS.md 硬性规则 4）：

```rust
ctx.emit(Progress::Started { total: Some(3), message: Some("解析 DSL…".into()) });
// ...第一步
ctx.tick(1, Some(3), "注册角色…");
// ...第二步
ctx.tick(2, Some(3), "写入章节…");
// ...第三步
ctx.done(result.clone(), started.elapsed().as_millis() as u64);
```

活例：`lilyco-letsgal/src/main.rs` 的 `Mode::Build` 分支（解析→注册→写入三步）。

## 4. 参数校验：只在 core 写一遍

校验规则**必须加在 `CommandSchema::validate_args`（core，带测试）**，
各端自动生效；禁止在某端私加校验（AGENTS.md 硬性规则 2，四端语义对齐的根基）。
应用侧只做「值域翻译」（如字符串 → enum，用 `ValueEnum` derive）。

## 5. 安全边界（默认安全的理由）

- MCP 调用面自动注入 **DenyElevated** 策略——AI 远程调用拿不到提权；
- Web **只绑 127.0.0.1**；
- 删除/覆盖/外呼 ≥T1，走确认流，别图省事绕过；
- 参数一律走 schema 传值，**禁止拼 shell 字符串**。

## 6. 端到端验收清单（每 PR 必过，CI 同款）

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets     # -D warnings 级别
cargo test --workspace                     # 新能力必须带测试
cargo run -p my-tool -- --schema           # schema 冒烟：AI 能读懂你的命令表
```

**四端语义对齐**：改可见/别名/隐藏语义时，对照 `docs/CODEGRAPH.md` §5 矩阵，
四端一起改，缺一即回归。

## 7. 活例复盘：lilyco-letsgal 是怎么长出来的

需求：把 Node 版 letsgal-ai（galgame 工程生成器）重写为 lilyco 域应用。

| 步骤 | 产出 | 教训 |
|---|---|---|
| ① 核心 DSL 解析器 | `src/dsl.rs`（`parse_story`，纯函数，与 Node 版语义 1:1） | 业务核心**先写成可测试的纯函数库**，与界面无关 |
| ② 工程操作 | `src/project.rs`（init/upsert/validate，`Result<_, String>`） | IO 层薄封装，错误用 String 起步、够用再细分 |
| ③ 块构建器 | `src/lib.rs`（curtain/dialogue/branch…，对齐 Node `blocks.js`） | 逐块对齐参考实现，`stable_id` 用 md5 保证重编译不漂移 |
| ④ 接框架 | `src/main.rs`：一个 `#[derive(App)]` + `lilyco::run::<Letsgal>()` | 业务库**一行都不改**，框架只是外皮 |
| ⑤ 测试 | `tests/integration.rs`：DSL→写入→validate 全链 + roundtrip | 验收口径 = 与参考实现语义一致，不是「能跑」 |

**框架接入成本 = 一个 main.rs（~107 行）**。这就是「One struct. Four interfaces.」的含义。

## 8. 血训（从 letsgal 与各域应用沉淀）

- **JSON 输出结构即产品 API**——改字段 = 破坏所有已接入的 agent，加字段可以、改语义不行；
- **中文参数/文件名是常态**（故事 DSL 全中文），路径与 JSON 处理别假设 ASCII；
- 参考实现（Node 版）是**语义基准**：移植时逐函数对照，验收看输出等价，
  不看「行为差不多」；
- 文档里「未来会加」的能力不要先写进 schema——空壳命令是 AI 眼里的假能力，
  比没有更糟。

## 9. 下一步读什么

- [CODEGRAPH.md](CODEGRAPH.md)：要改框架内部前必读（全部关键符号带文件：行号）
- [AGENTS.md](../AGENTS.md)：PR 规则、依赖方向、发布流程
- `lilyco-example/examples/multi.rs`：多命令可跑样板
- `lilyco-letsgal/`：域应用完整活例（本文所有代码片段的出处）
