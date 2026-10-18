# Tiny Agent MVP — 第一性原理与最小实现

> 目标：不是"能聊天的窗口"，而是**一个真的会做事、且做坏了能撤回的最小 agent**，
> 并且能作为 lilyco 生态里的一个 app 被四端调度。

---

## 1. 第一性原理

把"AI agent"这个词拆到不能再拆，只剩四个必备件：

```text
        ┌──────────────── Loop ────────────────┐
        │                                      │
   决策 (Llm) ──► 执行 (Host) ──► 观察 (ObsBuf) ──┤
        ▲                                      │
        └──────── 保护 (Machine) ◄──────────────┘
```

| 缺哪个 | 会退化成 |
| :--- | :--- |
| 没有 决策 | 它就是脚本，不是 agent |
| 没有 执行 | 它只是聊天，不能改变世界 |
| 没有 观察 | 它无法知道自己错在哪，只能瞎猜 |
| 没有 保护 | 它不敢放手改，人必须全程盯着 |

**推论**：MVP 的验收标准不是"界面好看"，而是**循环跑通、工具真被调用、mutating 前必有快照、连错能回滚**。

---

## 2. 四个件对应的代码

| 件 | 文件 | 关键约束 |
| :--- | :--- | :--- |
| 决策 | [`agent/llm.rs`](../tiny_lilyco/src/agent/llm.rs) | `Llm::plan() -> Step`，只允许「调一个工具」或「停」 |
| 执行 | [`agent/runtime.rs`](../tiny_lilyco/src/agent/runtime.rs) | `Host` trait；循环本身不做任何 I/O |
| 观察 | [`agent/tool.rs`](../tiny_lilyco/src/agent/tool.rs) | `ObsBuf` 定容 8KB，`ObsKind` 标明类型（text/diff/console/image/exit） |
| 保护 | [`agent/ooda.rs`](../tiny_lilyco/src/agent/ooda.rs) | `mutating` 工具**必须先 `checkpointed()`** 才能 `finish_act()` |

保护这一条是硬约束，不是约定：状态机拒绝从 `Act` 直接进 `Verify`，
所以任何宿主都无法"忘记"打快照。

```rust
let permit = machine.begin_act(&action)?;   // mutating => CheckpointFirst
if permit == ActPermit::CheckpointFirst {
    host.checkpoint(tool_name)?;            // 只有这里能推进
    machine.checkpointed()?;
}
```

---

## 3. 最小工具集

三个就够证明"真的在干活"：

| 工具 | 参数 | 变更是 |
| :--- | :--- | :--- |
| `read_file` | 相对路径 | 否 |
| `list_dir` | 相对路径（空=根） | 否 |
| `edit_file` | `path\|old\|new` | 是 |

两条安全规则写在工具里，不写在提示词里：

1. **沙箱根**：所有参数是相对路径，`resolve()` 用 `root.join` 拼，
   `sanitize_rel` 拒绝 `..`、绝对路径、盘符。
2. **唯一锚点**：`plan_replace` 发现多处匹配直接报错，**不猜**。
   模型想改，必须先把上下文读准。

---

## 4. 与 lilyco 生态的兼容

[`agent/app.rs`](../tiny_lilyco/src/agent/app.rs) 里的 `AgentApp` 实现 `TinyApp`：

```rust
impl TinyApp for AgentApp {
    type Args = &'static str;      // goal
    type Output = Outcome;
    type Error = &'static str;
    fn schema() -> CommandSchema { CommandSchema::new("agent", 1024) }
    fn execute(...) -> ...        // 跑 drive()
}
```

这意味着它和其它 lilyco 应用**完全同构**：一个 struct，CLI / TUI / Web / MCP 四端
由 facade 自动派生，无需为 agent 单独写一套入口。`schema()` 同时就是 MCP 的 tool 描述。

---

## 5. 现在的边界（诚实清单）

| 能力 | 状态 |
| :--- | :--- |
| 决策循环 | ✅ 已实现，`ScriptedLlm` 可离线复现；真实 HTTP 模型仍是缺口 |
| 工具执行 | ✅ 三个真工具，带沙箱与唯一锚点校验 |
| 观察回灌 | ✅ 上一轮 observation 会传给下一次 `plan()` |
| 检查点/回滚 | ✅ 状态机强制；`Host` 由宿主实现（CLI 接 `jj`/`git`） |
| lilyco 四端 | ✅ `AgentApp: TinyApp` 已就绪；尚未接进 facade 的 `run::<T>()` |
| 真实模型 | ❌ 未接（无 key、无 HTTP 客户端）。接口已留：实现 `Llm` 即可 |
| CLI 二进制 | ❌ 未加。宿主实现 `Host` 是唯一缺口 |

---

## 6. 验证

`Tiny Lilyco Remote CI` 覆盖：

- 嵌套预算（≤ 4 层 / ≤ 20 列）
- `thumbv7em-none-eabi` + `wasm32` 裸机交叉检查（核心必须保持 `no_std`）
- `cargo clippy -D warnings` × 2（`no_std` / `std`）
- `cargo test` × 2：包含 loop、policy、tools、app 共 20 个新增用例

其中关键用例：

| 用例 | 断言 |
| :--- | :--- |
| `runs_every_scripted_call` | 2 次调用全部执行、结果 clean |
| `mutating_call_forces_a_checkpoint` | `edit_file` 触发且仅触发 1 次 checkpoint |
| `read_only_call_takes_no_checkpoint` | 只读工具不产生快照 |
| `repeated_failure_rolls_back` | 连续失败后 `undo` 被调用 |
| `unknown_tool_stops_cleanly` | 未知工具不崩溃、不执行 |
| `budget_caps_the_run` | 预算 3 时恰好停在第 3 次 |
| `edit_file_refuses_ambiguous_anchor` | 多处匹配时拒改并报错 |
| `edit_file_replaces_exactly_once` | 唯一匹配时精确替换 |
| `read_file_rejects_traversal` | `../secrets` 被拒 |
