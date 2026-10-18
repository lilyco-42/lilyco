# Tiny Agent — 开箱即用的桌面 agent

双击安装，打开就是一个能跑的 agent 窗口：输入一句目标，回车，它就在你的目录里执行。

## 安装

下载 `tiny-agent-gui-x.y.z-x64.msi` → 双击 → 完成（**不需要管理员**，装到
`%LOCALAPPDATA%\Tiny Agent`，开始菜单有快捷方式）。

## 第一次使用

1. 打开「Tiny Agent」
2. 在底部输入框打一句目标，例如 `list the workspace`
3. 回车（或点 **Run**）
4. 看左边的 trace

默认工作目录是**你的用户主目录**。想换目录，用环境变量启动：

```powershell
$env:TINY_AGENT_WORKSPACE = "D:\my-project"
& "$env:LOCALAPPDATA\Tiny Agent\Tiny Agent.exe"
```

## 两种大脑

| 模式 | 怎么开 | 行为 |
| :--- | :--- | :--- |
| **离线计划**（默认） | 不用配置 | 立刻执行 `list_dir` + `read_file README.md`，零依赖、零等待 |
| **真实模型** | `TINY_AGENT_PROVIDER=ccswitch` | 读 cc-switch 当前 provider，走 Anthropic 协议 |

```powershell
$env:TINY_AGENT_PROVIDER = "ccswitch"
& "$env:LOCALAPPDATA\Tiny Agent\Tiny Agent.exe"
```

真实模型是 **opt-in** 的原因：provider 若失效，窗体要等 HTTP 超时，表现为一直
`Running…`。离线计划保证"打开就有结果"。

## 界面

* **Trace**：每一步一行 —— `goal / workspace / provider / 工具调用 → 结果`
* **Diff / Preview**：预留给后续版本
* 左下角三个主题：Tokyo Night（默认）/ Neutral Light / Neutral Dark，全部来自
  `tiny_lilyco::ui` 的设计令牌

## 安全边界

* 当前版本**只读**：`edit_file` 会返回 `refused (dry run)`。写操作要等 checkpoint
  真正接上 `jj`/`git` 之后才放开 —— 宁可先不能用，也不要在没有回滚的情况下改你的文件。
* trace 只展示工具的**前 12 行**输出，避免长目录刷屏。

## 排障

| 现象 | 看这里 |
| :--- | :--- |
| 窗口没出现 | `%LOCALAPPDATA%\tiny-agent\error.log` |
| 一直 `Running…` | 说明在用真实模型且上游卡住；去掉 `TINY_AGENT_PROVIDER` 回到离线计划 |
| 想看页面源码 | `%LOCALAPPDATA%\tiny-agent\ui.html` |
| 启动尺寸/DPI | `%LOCALAPPDATA%\tiny-agent\startup.log`（如 `dpi x1.25 ok 1375x900`） |

## 自测

```powershell
$env:TINY_AGENT_SELFTEST = "1"
& "$env:LOCALAPPDATA\Tiny Agent\Tiny Agent.exe"
```

启动 1.2 秒后自动跑一次，用来确认装的是好的。
