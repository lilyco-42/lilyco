# M2 双向通道设计：UI 动作回传给 App

## 问题

当前框架是单向的：

```
App::run(&A, &Context)  ──Progress──▶  executor  ──▶  TUI / CLI / Web
```

TUI 运行在宿主线程，`App::run` 运行在 executor 线程。聊天模板已经能产生 `ChatAction`（`Send` / `Delete` / `Reply` …），但这些 intent 只留在 `TuiApp` 本地队列里，App 拿不到，因此无法真正发送或撤回。

## 约束

- `core::executor` 仍是唯一执行宿主，后端不得自建线程池或进度循环。
- 依赖单向：`core` 不依赖 `tui` / `gui`；动作类型必须定义在 `lilyco-core`。
- TUI 只发 intent，不直接访问 OneBot / 网络；权限门仍在 App/Gateway。
- 旧 App 不轮询动作时行为完全不变。
- 不引入全局可变状态。

## 候选方案

### A. 反向 action channel（推荐）

executor 在 spawn 时额外创建一条 `mpsc` 通道：

```
TuiApp ──UiAction──▶ action_tx ═══▶ action_rx ──poll──▶ Context ──▶ App::run
```

- `lilyco-core::action::UiAction`：`Send(String)` / `Reply(usize)` / `Edit(usize)` / `Delete(usize)` / `Copy(usize)` / `React(usize)` / `Open(usize)`。
- `executor::spawn_with_actions(run, cancel, action_rx) -> Task`；已有 `spawn` 保持不变（内部调用 `spawn_with_actions` 并传 `None`）。
- `Context::poll_action(&self) -> Option<UiAction>`：`try_recv`，无通道返回 `None`。
- facade 的 `build_handler` 增加可选 `action_tx`；`run_tui` 创建通道，把 `tx` 交给 `TuiApp`，`rx` 交给 handler。
- `TuiApp::drain_actions()` 优先把动作写入 `action_tx`；无 sender 时保留本地队列（供测试与旧宿主）。

优点：不改 `App` trait 签名；旧 App 零改动；动作与进度共用 executor 生命周期。
缺点：App 必须在 `run` 内轮询；长期聊天会话需要 `run` 本身长期运行（这正好符合聊天场景）。

### B. `App::on_ui_action` 回调

在 trait 增加 `fn on_ui_action(&self, action: UiAction)`，TUI 直接调用 App 实例。

需要 App 实例同时被 handler 与 TUI 持有（`Arc<A>`），并要求 `A: Send + Sync`；会改动所有 App 的线程约束，破坏面大。不推荐。

### C. 单独 ChatSession 协议

为聊天新增一套 `ChatApp` trait 与长期会话循环。

表达力最强，但等于在框架里开第二套执行模型，违反“executor 唯一宿主”。二期再评估。

## 推荐落地步骤（A）

1. `lilyco-core`：新增 `action.rs`（`UiAction`）与 `Context::poll_action`；executor 增加 `spawn_with_actions`。
2. `lilyco` facade：`build_handler` 接受 `Option<Sender<UiAction>>`；`run_tui` / `run_tui_multi` 创建通道并注入。
3. `lilyco-tui`：`TuiApp` 增加 `action_tx: Option<Sender<UiAction>>` 与 `with_action_sender`；`drain_actions` / Enter 发送时写通道。
4. 测试：
   - core：`spawn_with_actions` 收到动作、`poll_action` 无通道返回 `None`。
   - facade：TUI 发 `Send` 后 App 侧收到（可用 channel 直接断言）。
   - tui：有 sender 时本地队列为空、无 sender 时保留旧行为。
5. 真机：`search --tui` 输入后 Enter，App 侧日志出现收到 `Send`；不真实发送，只验证通道。

## 安全口径

- TUI 的 `Delete` / `Send` 永远只是 intent；App 收到后仍必须走 `Actions` 能力与 `resource/field` 权限门。
- 未授权时 App 返回 `Error::Permission`，TUI 只显示错误，不重试。
- 通道断开（App 已退出）时 `send` 失败静默丢弃，不影响 TUI 退出。

## 未决

- 长期聊天会话由谁决定退出：`run` 循环结束、用户 `q`、还是 App 收到 `Shutdown` 动作。默认用 `Cancel` 现有机制，不新增 `Shutdown`。
- 多命令切换时通道是否复用：一期每个命令运行独立 spawn，通道随 task 生命周期创建，切换即重建。