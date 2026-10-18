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
| CLI 二进制 | ✅ `tiny-agent`：真实目录上的多步执行，默认 dry-run |
| 真实模型 | ✅ OpenAI 兼容客户端（OpenRouter / NIM / cc-switch），CI 用 stub 验证链路 |
| MCP 暴露 | ❌ 未接。`ToolSpec` 已具备名字/摘要/参数预算，可直接生成 `tools/list` |

---

## 5.1 CLI 用法

```bash
# 只读计划（默认）：观察现有文件
tiny-agent . --call list_dir: --call read_file:Cargo.toml

# 真正执行写操作：显式 --apply 才会改文件，且 mutating 前必有 checkpoint
tiny-agent . --call edit_file:"README.md|old|new" --apply
```

CI 里的真实输出（`/tmp/tiny-smoke`）：

```text
root   : /tmp/tiny-smoke
mode   : dry-run
vcs    : Git
    list_dir() -> text
      note.txt
    read_file(note.txt) -> text
          1| alpha
          2| beta
result : 2 calls, 0 failures, 0 checkpoints, rolled_back=false
phase  : done
```

---

## 5.2 接真实模型

三家都是同一套 OpenAI 兼容协议（`POST {base}/chat/completions`），所以只有一个客户端：

| `--provider` | base_url | 说明 |
| :--- | :--- | :--- |
| `openrouter` | `https://openrouter.ai/api/v1` | 需要 OpenRouter key |
| `nim` / `nvidia` | `https://integrate.api.nvidia.com/v1` | NVIDIA NIM（有免费额度） |
| `ccswitch` / `local` | `http://127.0.0.1:8788/v1` | 本地代理；端口不同就用 `--base-url` 覆盖 |
| 任意 | `--base-url` | 自带网关时用它 |

凭据与模型走参数或环境变量：

```bash
export TINY_AGENT_API_KEY=sk-...
export TINY_AGENT_MODEL=anthropic/claude-3.5-sonnet   # 或 nvidia/llama-3.1-70b 等

tiny-agent . --provider openrouter --goal "读完 Cargo.toml，把版本号改成 0.2.0"
tiny-agent . --provider nim --apply
```

约定：

* **默认 dry-run**，写操作必须 `--apply`；未授权时工具返回 `NotPermitted`，不会静默降级。
* 模型每轮最多调一个工具，参数是一个字符串：`<path>` 或 `<path>|<old>|<new>`。
* 模型返回的 `{"path":..,"old":..,"new":..}` 也会被 `normalize_args` 拍平成同一格式。
* 请求构建与响应解析都是纯函数（`build_request` / `parse_step` / `normalize_args`），
  CI 用本地 stub 服务器验证整条 HTTP 链路，不联网。

---

## 5.32 cc-switch（本机代理）

cc-switch 存的是 **Anthropic 协议**配置，不是 OpenAI：

| 位置 | 内容 |
| :--- | :--- |
| `~/.cc-switch/cc-switch.db` | SQLite，`providers.settings_config` 里是各 provider 的 `env` |
| `~/.claude/settings.json` | cc-switch **物化**出的当前生效配置（base / token / model） |
| `proxy_config` 表 | 本地代理地址，实测 `127.0.0.1:15721` |

我们读后者：`ccswitch::load()` —— 读 `~/.claude/settings.json` 的 `env` 块。
**为什么不直接读 db**：SQLite 需要引擎，而 cc-switch 已把当前 provider 原样写进
settings.json，那是 Claude Code 自己读的文件；要枚举全部 provider 才需要 db。

```bash
tiny-agent . --provider ccswitch --probe        # 先探活
tiny-agent . --provider ccswitch --goal "..."   # 通过后再跑
```

`--probe` 打印 `provider / endpoint / model / key(打码)` 和真实错误，用来区分
"key 失效" 和 "地址写错"。

---

## 5.3 借鉴了哪些设计（结构，不是复杂度）


| 来源 | 抄的地方 | 在本项目中的形态 |
| :--- | :--- | :--- |
| Claude Code | 权限模式：默认只读，写操作要显式授权 | `--apply`；未授权时 `ToolError::NotPermitted` |
| Codex | 沙箱与审批分离 | `Host::checkpoint` 独立于 `drive`，回滚不依赖模型 |
| opencode | 单二进制 + 工具表 | `tiny-agent` 一个入口，`Registry` 一张表 |
| zerostack | 确定性、低占用 | 计划固定 → trace 固定；核心零分配 |


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


