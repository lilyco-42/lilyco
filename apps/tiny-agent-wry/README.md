# Tiny Agent — 装上就能用

双击安装，打开，打一句话，点「开始」。就这三步。

## 第一次打开

窗口中间写着「你想让我做什么？」，下面有一个输入框和一个「开始」按钮。

1. 在输入框打一句人话，比如 **这个文件夹里有什么**
2. 点「开始」（或按回车）
3. 结果直接出现在上方

默认在你的**文档**目录工作。想换目录：

```powershell
$env:TINY_AGENT_WORKSPACE = "D:\我的项目"
& "$env:LOCALAPPDATA\Tiny Agent\Tiny Agent.exe"
```

## 两种模式

| 模式 | 怎么用 | 会发生什么 |
| :--- | :--- | :--- |
| **本地模式**（默认） | 什么都不用填 | 只做本地文件操作：列目录。不联网、不花钱 |
| **接模型** | 点右上角「设置」 | 填接口地址 / API Key / 模型，保存后自动使用 |

设置里填什么：

| 字段 | 例子 |
| :--- | :--- |
| 接口地址 | `https://openrouter.ai/api/v1`（以 `/v1` 结尾按 OpenAI 协议）<br>`https://api.deepseek.com/anthropic`（其他按 Anthropic 协议） |
| API Key | `sk-...` |
| 模型 | `deepseek-chat` / `anthropic/claude-3.5-sonnet` |

填完保存，下次点「开始」就用模型回答。

## 安全边界

* 现在**不会改你的文件**。让它改东西，它会回答「改文件这件事我暂时不敢做：还没接上自动备份」。
  等自动备份（jj/git 快照）接上了才放开。
* 不填 Key 时**完全不联网**，可以断网使用。

## 出问题了看哪里

| 现象 | 看这里 |
| :--- | :--- |
| 窗口没出现 | `%LOCALAPPDATA%\tiny-agent\error.log` |
| 一直显示「正在做…」 | 说明模型接口卡住了；点右上角设置把 Key 清空即回到本地模式 |
| 想看页面源码 | `%LOCALAPPDATA%\tiny-agent\ui.html` |
| 启动尺寸/DPI | `%LOCALAPPDATA%\tiny-agent\startup.log` |
| 你保存的设置 | `%LOCALAPPDATA%\Tiny Agent\settings.json` |
