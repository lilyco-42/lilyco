## DSH Integration

**DeepSeek Harness 接入（实测验证）**：lilyco 应用以 **MCP 服务器**形态挂进 dsh，模型直接获得原生工具。

### 原理

dsh 的能力扩展单元是 cordis 插件；外部 Rust 二进制经官方 [`@deepseek-ai/dsh-mcp-client`](https://github.com/deepseek-ai/deepseek-harness) 插件桥接（spawn 进程 + 注册 `ctx.tools`），模型看到 `mcp__<server>__<tool>` 原生工具。

### 一键接入

```bash
curl -fsSL https://raw.githubusercontent.com/lilyco-42/lilyco/HEAD/install.sh | bash
```

脚本自动：下载 release 二进制 → 安装 `dsh-mcp-client` 插件 → 写 profile patch。重启 dsh web 后，模型获得：

| 服务器 | 工具 |
|---|---|
| `mcp__lbrush` | `Brush`（真 bash 执行器：变量/管道/重定向/`&&`/`||`/if-for-while） |
| `mcp__lvision` | `Crop` / `Resize` / `DominantColors` / `PixelDiff` / `ExtractForeground` / `Trace` / `HtmlScreenshot` / `ImageInfo` |

### 已知坑（实测）

- **首轮不可见**：若 profile 使用了 router-flash 类 agent preset（首轮 core 工具过滤），新会话第一轮看不到 mcp 工具 —— 让模型先调用任意一次工具，下一轮全目录放开。
- 插件无 `dsh.bundle` 时是 plain dependency，patch insert 显式引用其 name 即可加载。
- 加/改插件后必须**重启 dsh web**（可用 `tasklist` 验证 `lbrush-windows.exe --mcp` 子进程确认插件已连接）。

