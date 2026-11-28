# Chat TUI 模板规划

目标：在 lilyco 框架内提供一套**可复用的聊天 TUI 模板**。应用只声明会话与消息，终端布局、焦点、选择、输入和消息动作由框架统一处理；Web / GUI / TUI 共享同一份语义视图协议。

参考来源与交互依据见 `lilyco-tui/AGENTS.md` 的「聊天 TUI 参考」。主参考为 concord（Rust + ratatui），不复制其代码。

## 非目标（一期）

- 不做图片 / 语音 / 视频渲染（二期用 `ratatui-image` 等可选依赖）。
- 不做 Markdown 与代码高亮（二期）。
- 不做网络层；TUI 只消费 `Component` 与 App 提供的 action。
- 不复制任何 GPL 代码；discordo / tele 仅借鉴交互与分层。

## 分层

```
App（如 lilyco-qq）
  ├─ views(): 声明 chat / sidebar / composer 槽位
  └─ run():  发送 Message / MessageList / Composer 更新
        ↓ Progress::View
lilyco-tui
  ├─ ViewState：槽位与组件存储、滚动、裁剪、滚动条
  ├─ ChatState：焦点 pane、选中消息、输入模式、草稿
  ├─ component：Component → 终端渲染（新增 Message / MessageList / Composer / Pane）
  └─ 键位路由：1-4 / Tab / j/k / gg/G / i / Esc
        ↓
lilyco-core
  └─ view::Component 新增聊天语义组件
```

## 新增语义组件（lilyco-core）

| 组件 | 字段 | 说明 |
|---|---|---|
| `Message` | `author`、`time`、`body`、`edited`、`reply_to`、`tone`、`mine` | 单条消息；`tone` 表达系统/错误/普通 |
| `MessageList` | `messages`、`selected`、`unread` | 消息流；`selected` 为选中索引，`unread` 为未读起点 |
| `Composer` | `value`、`placeholder`、`mode`、`attachments` | 输入区；`mode` 为 `Normal` / `Insert` |
| `Pane` | `id`、`title`、`active`、`children` | 通用面板容器，替代临时拼装 |

所有组件只描述语义，不携带终端布局参数。

## 键位（默认，可被 keymap.toml 覆盖）

| 键 | 动作 |
|---|---|
| `1`–`4` | 焦点直达 pane |
| `Tab` / `Shift+Tab` | 焦点循环 |
| `h` / `l` | 焦点左右 |
| `j` / `k` | 选择下 / 上 |
| `gg` / `G` | 顶 / 底 |
| `Ctrl+d` / `Ctrl+u` | 半页滚动 |
| `i` | 进入插入模式 |
| `Esc` | 退出插入 / 返回 |
| `Enter` | 发送（经 App 权限门） |
| `Ctrl+J` | 插入换行 |
| `y` `r` `R` `e` `d` `o` `v` | 消息动作，仅发出 intent |

## 里程碑

1. **M1 只读聊天流**：✅ 已完成（core 组件 + TUI 渲染 + 焦点/选择/滚动 + 单测 + 真机截图）。
2. **M2 输入**：⚠ 部分完成——composer 插入模式、草稿、`Send` intent 已做；**真实发送需要 UI→App 双向通道**，设计见 `docs/chat-tui-m2-channel.md`（推荐反向 `UiAction` channel），未接网络。
3. **M3 动作**：✅ 已完成——`Copy/Open/Send` 直接入队；`Delete/Edit/Reply/React` 先弹确认层（`Enter` 确认 / `Esc` 取消，居中覆盖层，边框用 `theme.warn`），确认后才产生 intent。
4. **M4 增强**：⚠ keymap/theme 外置 ✅；Markdown 子集 ✅（标题/引用/列表/围栏代码/粗斜删除线/行内代码，纯自研无依赖）；代码高亮与图片（`ratatui-image`）待做。
5. **M5 应用接入**：✅ lilyco-qq 已更新 rev 到 `c553be3`，`search` 声明 `chat` 并发送 `MessageList`，`recent` 声明 `conversations` 并发送 `Sidebar`；本地提交 `b3b5619`（该仓库未配置远程，未 push）。

## 验收

- `cargo test -p lilyco-core -p lilyco-tui` 全绿；新增组件有渲染断言。
- `cargo clippy -p lilyco-tui --all-targets` 新增文件 0 警告。
- 真机 80×24 与 110×32 截图：焦点边框、选中行、未读徽标、composer、状态栏均可见。
- 发送/删除动作在未授权时被拒绝，且 TUI 不自行绕过权限门。