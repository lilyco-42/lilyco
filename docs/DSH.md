## DSH Integration

**DeepSeek Harness 接入（实测验证）**：lilyco 应用以 **MCP 服务器**形态挂进 dsh，模型直接获得原生工具。

> **UI 选型建议**：凡涉及 AI 对话、Agent 工具调用与多轮流式交互的网页端场景，**优先推荐直接使用 DSH（DeepSeek 开源网页 UI & Harness）作为前端宿主**，复用其成熟的响应式布局与插件生态，避免自研简陋聊天页出现排版与流式体验缺陷。

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
---

## 反向：lilyco 应用当 dsh 的模型提供端（实测验证）

上面是「lilyco 应用作为工具挂进 dsh」。反过来的方向也通了：**lilyco 应用自己起一个 OpenAI 兼容的端点，dsh 把它当成一个模型 provider**。已经打通的是 `lchatgpt serve`（见 [`docs/chatgpt.md`](chatgpt.md) 的同名章节）。

```bash
lchatgpt serve --port 8790          # GET /health、/v1/models、POST /v1/chat/completions（SSE）
```

在 dsh profile 的 `cordis.patch.yml` 里，给 `llm-pi-ai` 的 `config.providers` 加一条 route：

```yaml
      lchatgpt:
        displayName: lchatgpt
        api: openai-completions
        baseURL: http://127.0.0.1:8790/v1
        headers:
          Authorization: Bearer unused
        models:
          - id: "lchatgpt:chat"
            input: [text]
            name: ChatGPT (chat quota, 走浏览器)
            contextWindow: 128000
            maxTokens: 32000
```

### 这一侧也有坑（都实测踩过）

- **`api:` 只能是四种之一**：`openai-completions` / `openai-responses` / `anthropic-messages` / `google-generative-ai`。写 `openai-codex-responses` 会被 pi-ai 直接拒绝——那条协议只能靠 catalog provider 的 OAuth 登录，无法用 patch 表达。
- **别写 `apiKeyEnv`**：它是 harness 凭证库的引用，取不到就是 `MISSING_CREDENTIAL`。本地端本来没有 key，用 `headers.Authorization: Bearer unused` 占位即可（pi-ai 只要见到非空 authorization 就放行，value 不看）。
- **`cordis.patch.yml` 里手写的 `models` 才是真正决定这条 route 服务什么模型的东西**；UI 里的「fetch available models」只是发现，不要指望它。
- **`--profile desktop` 不能用 CLI 验证**（`error: profile "desktop" is managed exclusively by the Electron application`），改完 patch 只能重启桌面版看 UI。要脚本化验证就用 headless profile 加 `--patch` overlay：

  ```bash
  dsh --profile headless --patch /tmp/lchatgpt-provider.yml --json "只回答两个字：香蕉"
  ```

- 一次 dsh 提问会**发两个 POST**：主对话（带全部工具定义，实测 23 个）与「生成会话标题」的小请求。两者都会打到你的端点，别只处理第一个。
- pi-ai 的 `streamIdleTimeoutMs` 默认 300000（5 分钟）：本地端首字要等十几秒没问题，但 SSE 要发 keep-alive 心跳，别让它空着。
- harness 只抢 `user-agent` 一个归属头，`Authorization` 之类你可以自由设。
