# lchatgpt — chatgpt.com 账号后端域（会话历史只读 + 两种额度分开发消息）

> crate `lilyco-chatgpt` / bin `lchatgpt`。把 **chatgpt.com 的账号后端**写成 CLI：
> 会话历史只读查询、模型清单、两套配额，以及向模型真正提问。
> 四端（CLI / TUI / Web / MCP）同 registry，同一份 handler。
>
> 产物是**一个自包含的二进制**：不需要 OpenAI API key，不需要装 ego-lite，也不需要
> 装 chromium。浏览器只是**可选的两样东西之一**：登录态的一种来源
> （`login --from browser`），以及 `ask` 默认那条路（chat 额度）要驱动的那台浏览器。
> 两样都不是硬依赖：端口上没人就回落或明确报错，其余命令照常可用。

## 两种额度，默认只花 chat 的那份

账号上有两套互不相干的配额，`lchatgpt` 把它们当作**两种发送方式**：

| 走法 | 花谁的额度 | 怎么发 | 默认？ |
|---|---|---|---|
| **chat**（默认） | 网页聊天额度 | 驱动一台已登录 chatgpt.com 的浏览器：把问题打进输入框、点发送，再用 REST 读回回复 | ✅ `lchatgpt ask` |
| **codex** | codex 额度 | `POST /backend-api/codex/responses`（SSE） | ❌ 必须显式 `--allow-codex` |

这是刻意的：**codex 额度绝不会被偷偷花掉**。没有 `--allow-codex` 时，代码路径里
连 `codex/responses` 这个字符串都不出现（有单测守着），找浏览器失败也只会报错并
告诉你「只想用 codex 额度的话，显式加 `--allow-codex`」。

`lchatgpt usage` 把两套额度分开报；`lchatgpt models` 也分别说清两条路各自的模型。

## 为什么 chat 额度必须过浏览器

`POST /backend-api/conversation`（纯 HTTP 发消息）必然 403：

```
{"detail":"Unusual activity has been detected from your device. Try again later. (…)"}
```

后端要求 `/backend-api/sentinel/chat-requirements` 发下来的 **turnstile + PoW + SO
设备指纹**。实测：带真 token 打 `POST /backend-api/sentinel/chat-requirements` 回的是
`persona: chatgpt-paid`，`turnstile.required: true`、`proofofwork.required: true`、
`so.required: true` —— 纯 HTTP 要做到这套采集器代价极高，而对真人浏览器来说它就是**免费**的。

所以 chat 这条路是：**让页面自己发**（页面不需要过这些门），`lchatgpt` 只负责把字打进
输入框、点亮发送按钮，然后用**自己的 Bearer** 走 REST 读回回复。

## 三条真实可用的路

1. **只读 REST**（`/backend-api/me`、`/conversations`、`/conversation/{id}`、
   `/conversations/search`、`/models`、`/codex/usage`、`/wham/usage`）—— 普通 Bearer 就能读。
2. **浏览器驱动**（chat 额度）：CDP 连一台已登录的 chromium，打问题、点发送；
   回复用第一条路读回，**不碰 DOM 选择器**（消息节点没有稳定属性，类名全是哈希）。
3. **codex responses**（`POST /backend-api/codex/responses`，SSE）—— 只在 `--allow-codex` 时走。
   ChatGPT 账号只接受 `gpt-6.2-sol` 一个 slug，换别的模型会 400
   `model_not_supported when using Codex with a ChatGPT account`。

## chat 那条路具体怎么走（实测细节）

`ask`（不带 `--allow-codex`）在一台**已经登录 chatgpt.com** 的 chromium 里：

1. **开自己的标签页**：`PUT {cdp}/json/new?https://chatgpt.com/`（Chrome 只认 PUT，GET 回 405），
   用完无论成败都 `GET {cdp}/json/close/{id}` 关掉，不碰用户正在看的那些。
2. **等输入框**：`div[contenteditable="true"]`（`#prompt-textarea` 早就不存在了），最多等 45 秒。
3. **写字**：`Selection.selectAllChildren + deleteFromDocument` 清空 → CDP `Input.insertText` →
   等 500ms 再读回 `innerText` 与发送按钮的 `disabled`。React 收到 input 事件后要一拍才重算按钮，
   插完立刻读会读到 `enabled: false`；实测常在第 3~4 次才两样都对，这里留了 14 次。
4. **点发送**：`button[aria-label="发送"]`（英文界面是 `Send`），最多试 20 次。
   **`Input.dispatchKeyEvent` 造 Enter 发不出去**（实测 40 秒毫无反应），只有真点按钮才行。
5. **认出这次提问落成了哪条会话**（见下一节）。
6. **读回复**：`GET /backend-api/conversation/{id}` 轮询，判据是 assistant 节点的
   `status == "finished_successfully"` 或 `end_turn == true` —— **不看 DOM**。
   超时但有正文时照样交出去，只是 `complete: false`。

### 会话 id：URL 里那个不能用

点完发送，页面 URL 会变成 `/c/local-chatgpt%3A<uuid>`。那个 id 是**页面内部的**：

- `GET /backend-api/conversation/local-chatgpt%3A<uuid>` → **400**（未编码冒号也是 400）
- `GET /backend-api/conversation/<uuid>`（只取 uuid）→ **404**

页内 `fetch("/backend-api/conversations")` 用 cookie 也是空列表；CDP Network 抓
`POST /backend-api/f/conversation` 的响应只有 `conduit_token`（真数据走 wss）。
所以认会话的办法只能是：**发之前先记下 `GET /backend-api/conversations?limit=20` 里已有的 id，
发之后每 2 秒轮询列表，找新冒出来的那条**，再核对它的最后一条用户消息确实是这次的问题
（防另一台设备同时开会话顶替）。实测约 16 秒落库，这里给 90 秒。

> 这一步失败要在**发消息之前**就失败：快照读不到就不发 —— 否则消息发出去了、额度也花了，
> 却永远认不出是哪条会话。

## 生图：图不在正文里，而且末节点永远 `in_progress`

实测一轮生图，`show-conversation` 看到的 mapping 是这样：

```
client-created-root
  └─ user              parts: ["画一只戴墨镜的橘猫"]                finished_successfully
      └─ assistant     content_type: code（去调画图工具的那个节点）  end_turn: false
          └─ tool      content_type: multimodal_text，图挂在这里     recipient: all
              └─ assistant   reasoning_recap（思考摘要）
                  └─ assistant  content_type: text, parts: [""]      in_progress ← current_node
```

两个后果，都踩过：

1. **正文是空的。** `parts` 里只有一个空字符串，字面意义上没有「回答」。图在中间那个
   `tool` 节点的 `parts[]` 里，形状是
   `{ content_type: "image_asset_pointer", asset_pointer: "sediment://file_<id>", width, height, mime_type }`。
   于是「这一轮完事了」的判据不能只看 `end_turn == true || status == "finished_successfully"`
   —— 末节点永远是 `in_progress`，那个条件一次都不成立。现在的判据是
   「已经出现过 `finished_successfully` + 拿到图 + 正文为空」这个组合。
2. **同一条 pointer 出现两次。** 一次是 ghostrider 中间态，一次是
   `is_visually_hidden_from_conversation` 的展示节点，所以按 `file_id` 去重
   （`sediment://` 前缀切掉就是文件 id）。

一开始没做第 1 条，结果是每次生图都跑满 180 秒轮询、还带 `complete: false`（那 180 秒里
把 `GET /conversation/{id}` 连打 123 次，还顺带把端点打成 429）。补上判据之后实测 33.7 秒、
`complete: true`。轮询间隔也跟着放宽了（1.2s → 2s → 3.5s），429/5xx 一律退避重试。

### 取图要两跳、三个头

`sediment://file_xxx` 不是 URL。取字节是 `GET /backend-api/files/{file_id}/download`
拿一个带签名的 `download_url`，再 GET 那个地址 —— **两跳都要带齐** `authorization` /
`user-agent` / **`chatgpt-account-id`**，少最后一个直接 403。

```bash
lchatgpt ask --prompt "画一只戴墨镜的橘猫，扁平插画" --save-images /tmp/imgs
# → 生成了 1 张图（file_id 见 --json 的 images[]）
#     [1] file_0000000070c4820995dcb2dfc52ff88c 1254x1254 -> /tmp/imgs/file_000…f88c.png
```

`--save-images` 是给人看的（落盘，mime 按 magic number 判，不信后端说的）；
`--inline-images` 是把字节 base64 内联进 `images[].data_base64` 并补上 `mime_type` ——
这个开关是专门给 **MCP 面**的：MCP 客户端拿不到我们的登录态、没法自己下载
`sediment://`，只有把字节塞进结果里它才能渲染成 `{type: "image"}` 内容块。
`show-conversation --inline-images` 同理（读历史里的图）。

`--json` 里那一轮长这样（`data_base64` 已省略）：

```json
{
  "quota": "chat",
  "conversation_id": "6b07c611-1b0c-83ea-bb1a-1e2fe8b6a1a6",
  "model": "gpt-6-2",
  "text": "",
  "images": [
    {
      "file_id": "file_0000000070c4820995dcb2dfc52ff88c",
      "asset_pointer": "sediment://file_0000000070c4820995dcb2dfc52ff88c",
      "mime": "image/png",
      "width": 1254,
      "height": 1254,
      "caption": "一只戴墨镜的橘猫，扁平插画风格，白底",
      "mime_type": "image/png",
      "saved_to": "/tmp/imgs/file_0000000070c4820995dcb2dfc52ff88c.png"
    }
  ],
  "complete": true,
  "polls": 14,
  "elapsed_ms": 26732
}
```

## 命令（10 条：7 条 T0 只读 + `ask` / `login` / `serve` T1 需确认）

| 命令 | 分级 | 作用 |
|---|---|---|
| `whoami` | T0 | `GET /backend-api/me` + 配额里的 `plan_type`；返回 `{ email, name, user_id, country, region, plan_type, account_id, credential_source, token }`。`token` 打码成「前 6 位…后 4 位」，`credential_source` 说明凭证是环境变量还是哪份文件读来的 |
| `usage` | T0 | 两套额度分开报。codex 段 `GET /backend-api/codex/usage`（`allowed / limit_reached / credits / primary_window / secondary_window / model_usage`，每窗口带 `used_percent / limit_window_seconds / reset_at / reset_at_iso / reset_in_human`）；chat 段 `GET /backend-api/wham/usage`（网页自己读的那份 rate_limit，结构同 codex 段）+ `POST /backend-api/conversation/init`（`default_model_slug` 与 `limits_progress[]` 的 `feature / remaining / reset_after`）。两套都**没有消息条数**维度 |
| `models` | T0 | `GET /backend-api/models`；返回 `{ title, count, default_model_slug, codex_models, models }`。`codex_models` 是 `ask --allow-codex` 能用的 slug 白名单；`default_model_slug` 是 `ask`（chat 额度）会用的网页默认模型 |
| `list` | T0 | `GET /backend-api/conversations?offset&limit&order`；返回 `{ total, offset, limit, order, count, items }`，每项 `id / title / create_time / update_time / is_starred / is_archived / model_slug`。只有元数据，正文要 `show-conversation` |
| `show-conversation` | T0 | `GET /backend-api/conversation/{id}`；沿 `current_node` 的 `parent` 链回溯出**活动分支**，只留发给用户的 user/assistant 文本，返回 `{ conversation_id, title, create_time, update_time, model_slug, node_count, message_count, returned, truncated, messages }` |
| `search` | T0 | `GET /backend-api/conversations/search?query=…`；返回 `{ query, limit, count, items }`，每项 `conversation_id / title / update_time / is_starred / snippet / payload`。`snippet` 由后端给、按 240 字符截断 |
| `auth` | T0 | **本地**凭证体检，不联网：返回 `{ resolved: { ok, source, capability, can_send, account_id, expires_at, expired, refreshable, token }, candidates, store_path, hints }`。`candidates` 逐条列出 env / 仓库 / codex 登录态哪个可用，`hints` 直接给下一步该跑什么 |
| `ask` | **T1** | **默认花 chat 额度**：驱动浏览器发、REST 读回，返回 `{ quota: "chat", conversation_id, model, text, images, complete, cdp, polls, elapsed_ms }`（`--cdp` 指定浏览器端点）。**生图那一轮 `text` 是空串、图在 `images[]` 里**（`file_id / asset_pointer / mime / width / height / caption`，加 `--save-images DIR` 落盘、加 `--inline-images` 内联 base64）。加 `--allow-codex` 才改走 `POST /backend-api/codex/responses`（SSE），返回 `{ quota: "codex", model, effort, response_id, text, usage, elapsed_ms, events }`，`--effort` 取 low/medium/high/xhigh/max/ultra |
| `login` | **T1** | 把一份登录态收进自己的仓库（`~/.local/share/lchatgpt/auth.json`，0600）。`--from browser` 从正在跑的浏览器抓、`--from codex` 导入 codex 登录态、`--from refresh` 立刻用 refresh_token 续期。返回 `{ action, source, capability, can_send, account_id, email, expires_at, store_path, token, refreshable }` |
| `serve` | **T1** | 起一个 **OpenAI 兼容的 HTTP 端**（默认 `127.0.0.1:8790`：`/health` + `/v1/models` + `/v1/chat/completions` 支持 SSE + `/v1/images/generations` 生图回 b64_json），让 dsh 之类的外部客户端把 ChatGPT 当成一个模型来用。模型 id：`lchatgpt:chat`（默认，花 chat 额度）；加 `--allow-codex` 才多出 `lchatgpt:codex`（花 codex 额度）。**阻塞到进程被杀**，所以没有 `--json` 结果 |

## 登录态：三条来源，能力不同

lchatgpt 不硬绑任何一种登录态，按优先级找，先命中先用（`lchatgpt auth` 完整列出来）：

```
LCHATGPT_ACCESS_TOKEN  (+ 可选 LCHATGPT_ACCOUNT_ID)   ← CI / 临时借用
  ↓ 没有
LCHATGPT_AUTH_FILE 指向的 JSON                        ← 自己指定一份文件
  ↓ 没有
~/.local/share/lchatgpt/auth.json                     ← lchatgpt login 写的仓库（0600）
  ↓ 没有
~/.codex/auth.json                                    ← codex login 的产物
```

三条都读不到时，错误信息会把这三个位置逐条列出来，并给出两条修法：
`codex login` 或 `lchatgpt login --from browser`，或者直接设 `LCHATGPT_ACCESS_TOKEN`。

### 两种 token 的能力差异（这不是猜测，是从 claim 里读出来的）

OpenAI 的 access token 是 JWT，能不能发消息写在 `scp` 里：

| 来源 | client_id | scp 关键项 | 能读 | 能发 |
|---|---|---|---|---|
| codex 登录态 | `app_EMoamEEZ73f0CkXaXp7hrann` | `api.connectors.invoke` | ✅ | ✅ codex 额度 |
| 浏览器登录态 | `app_X8zY6vW2pQ9tR3dE7nK1jL5gH` | `model.request` / `model.read` | ✅ | ❌ 纯 HTTP 401 |

注意这两件事是**分开**的：浏览器登录态的 token 本身发不了消息（401），但它所在的**那台浏览器**
能发（chat 额度）—— 因为发消息的是页面，不是 token。所以 chat 路径用 `auth::load()`
（只要有一份能读的就够，用来读回回复），codex 路径才用 `auth::load_for_send()`。

所以 `jwt.rs` 只解 payload（不验签，也不需要验）判定 `Capability`：
有 `api.connectors.invoke` → `Send`；解得出 scopes 但没有它 → `ReadOnly`；
根本不透明（解不开 JWT）→ `Unknown`，按「不擅自降级」处理。

`ask --allow-codex` 用 `load_for_send()`：手头这份只能读时，会再去仓库和 codex 文件里找一份
能发的；都找不到就直说「当前凭证只能读不能发」，而不是发出去再吃 401。
`ask`（chat 路径）只用 `load()`：只需要能读，回复是从 REST 读回来的。

### 续期

只有 codex 那份带 `refresh_token`，能续。`ask` 之前会自动续（`refresh::ensure_fresh`：
未过期不动，过期且有 refresh_token 就换，换失败也不拦着提问——让后端 401 说话）；
想手动强制续就用 `lchatgpt login --from refresh`。

续期走 `POST https://auth.openai.com/oauth/token`（`grant_type=refresh_token`，
client_id 固定为 codex 的 `app_EMoamEEZ73f0CkXaXp7hrann`，scope `openid profile email`）。
**refresh_token 会轮换**，所以换完必须写回仓库——`ensure_fresh` 与 `login --from refresh`
都会做这件事。

> OAuth 的 authorize 端点走不通：PKCE 请求恒回 `400 invalid_authorize_request`，
> 真浏览器里打开则会跳到「补手机号」那一步。所以这个 crate 不提供 `login` 授权流，
> 只搬运已有的登录态。

### 浏览器登录态怎么抓（可选，不启动浏览器）

`lchatgpt login --from browser` 用纯 Rust 的 CDP 客户端（tungstenite，本机 `ws://`
不需要 TLS）连一个**已经在跑的** chromium，读它的 cookie 换 token：

1. 找端点：`--cdp` → `$LCHATGPT_CDP` → ego-lite 的 `~/.config/ego-lite/config.json` 里的
   `cdpPort` → 常见端口 `9229/9222/9223/9333`。**显式给了 `--cdp` 就只试那一个**，
   不偷偷回落（否则你指名 A 浏览器，却从 B 浏览器抓来另一个账号，那是事故）。
2. 找标签页：`GET {cdp}/json/list` 里找 `url` 含 `chatgpt.com` 的 page；
   没有就 `PUT {cdp}/json/new?https://chatgpt.com/` 开一个，抓完关掉。
3. 在页面里同源 fetch `/api/auth/session`（`credentials: 'include'`），拿 `accessToken`；
   新开的标签页要等首页就绪，所以最多重试 6 次、每次隔 900ms。
4. 抓不到就把「试过的端点 + 每条的失败原因」全列出来。

抓到的这份**只读**，而且没有 refresh_token。所以 `login` 带一道防降级守卫：
仓库里若已有一份能发的凭证，新的这份只能读、又没加 `--force`，就保留仓库里那份并告警。

典型的一条龙：

```bash
lchatgpt login --from browser    # 从 ego-lite 的 chromium 抓（可选路线）
lchatgpt login --from codex      # 导入 codex 登录态：能读能发能续期（推荐）
lchatgpt auth                    # 看一眼现在生效的是哪份、能不能发
```

## 给别的程序当模型提供端（`serve` → dsh）

`lchatgpt serve` 把 ChatGPT 账号包成一个 **OpenAI 兼容的 HTTP 端**，于是任何会打
`/v1/chat/completions` 的客户端都能把它当成一个模型。这是给 **dsh（DeepSeek Harness）**
这类宿主准备的：宿主里那条 `llm-pi-ai` route 写 `api: openai-completions` 指过来即可。

```bash
lchatgpt serve --port 8790          # 只露 lchatgpt:chat → 花 chat 额度（驱动浏览器）
lchatgpt serve --allow-codex        # 多露一条 lchatgpt:codex → 花 codex 额度
```

| 端点 | 作用 |
|---|---|
| `GET /health` | `{ ok, service, quota: "chat" \| "chat+codex", models }` |
| `GET /v1/models` | OpenAI 的 `{ object: "list", data: [...] }`，pi-ai 的模型发现认这个形状 |
| `POST /v1/chat/completions` | `stream: true` 时走 SSE（标准 `chat.completion.chunk` + 末尾 `data: [DONE]`）；`false` 时一次性给 `chat.completion` |
| `POST /v1/images/generations` | 用 **chat 额度**生图：`{created, data: [{b64_json, revised_prompt}]}`。只回 b64_json —— 后端给的图片地址带签名、且必须带登录态头才能取，给 URL 客户端也打不开 |

额度归属**不因为套了一层 HTTP 就变**：默认只有 `lchatgpt:chat`，它走的就是上面那条
「驱动浏览器」的路；`lchatgpt:codex` 只有在启动时显式 `--allow-codex` 才会出现在
`/v1/models` 里，否则请求它是 400 并告诉你加这个开关。**看不到的模型不会被偷偷换过去。**

生图端点走的就是 `ask` 的 chat 路径（一次生成约 30 秒）。抓不到图时回 400「这一轮没有生成图片」
并带上网页那一轮的文字，而不是回一个空数组。`response_format=url` 一律 400 并说明原因。

多轮对话会被压平：chat 路本来就只有「发一段、读一段」，所以 `messages` 里的历史被
拼成一段文字（多条 user 消息带 `User:` / `Assistant:` 前缀，system 放最前）。codex 路
保留逐条消息，`system` / `developer` 折进 `instructions`。

### 接进 dsh（实测跑通）

dsh 的 profile 改 `~/.dsh/profiles/<name>/cordis.patch.yml`，在 `llm-pi-ai` 那条的
`config.providers` 下加一个 route：

```yaml
      lchatgpt:
        displayName: lchatgpt
        api: openai-completions
        baseURL: http://127.0.0.1:8790/v1
        # 本地端不校验 key，但 pi-ai 要求 apiKey 或 Authorization 头二者有其一
        headers:
          Authorization: Bearer unused
        models:
          - id: "lchatgpt:chat"
            input: [text]
            name: ChatGPT (chat quota, 走浏览器)
            contextWindow: 128000
            maxTokens: 32000
```

三个关键点（都踩过）：

1. `api:` 只能取 pi-ai 这一版支持的四种协议（`openai-completions` / `openai-responses` /
   `anthropic-messages` / `google-generative-ai`）。写 `openai-codex-responses` 会被拒 ——
   那条协议只能走 catalog provider 的 OAuth 登录，不是 patch 能表达的。
2. **不要写 `apiKeyEnv`**。`apiKeyEnv` 是「去 harness 凭证库里取一条」的引用，取不到就是
   `MISSING_CREDENTIAL`；本地端本来就没有 key，用 `headers.Authorization` 占位就够了
   （pi-ai 见到非空 authorization 就放行）。
3. 改完 patch **必须重启 dsh**（桌面版要重启 Electron，`--profile desktop` 被 CLI 拒绝）。

验证不用开 UI：用 headless profile 加一个临时 overlay 就能一问一答。

```bash
lchatgpt serve --port 8790 &
dsh --profile headless --patch /tmp/lchatgpt-provider.yml --json "只回答两个字：香蕉"
# → {"type":"text","text":"香蕉"} … {"type":"final","text":"香蕉"}
```

实测：dsh 一次提问会发两个 POST（主对话 + 生成会话标题），主请求带着 23 个工具定义和
`max_completion_tokens: 32000`；整轮 19 秒拿到 `香蕉`。**网页路不报 token 用量**，所以
`step_end` 的 usage 是全 0 —— 这不是 bug，chatgpt.com 的网页接口本来就不给。

## 当 MCP 服务端（`--mcp`）：版本协商、resources、图片内容块

`lchatgpt --mcp` 是 stdio JSON-RPC，注册表里那 10 条命令就是 10 个工具。dsh 这类客户端会挑三件事：

1. **协议版本要按客户端说的回。** 支持 `2025-11-25` / `2025-06-18` / `2025-03-26` /
   `2024-11-05`，`initialize` 按 `params.protocolVersion` 协商，只有认不出来才回落
   `2024-11-05`。dsh 的 MCP SDK 是 legacy 握手，回一个它列表外的版本会直接抛
   `Server's protocol version is not supported`。
2. **`capabilities` 必须声明 `resources`。** dsh 那边有守卫
   `getServerCapabilities()?.resources !== void 0` 才肯真的发 `resources/list` —— 少了这个键，
   客户端永远不问。所以 capabilities 是 `{"tools": {}, "resources": {}}`：`resources/list`
   把命令当资源（uri `lilyco://command/<name>`），`resources/read` 回该命令的 schema JSON。
3. **图片走真内容块。** 工具结果里带 `{type: "image", data, mimeType}`：顶层 `images[]` 与
   `messages[].images[]` 都收，只认 png/jpeg/webp/gif，最多 8 块；出块之后把文本块里的
   `data_base64` 删掉（一张 1.5 MB 的 PNG 会让上下文翻倍 —— 实测文本块从 1559917 字节
   降到 624 字节，图只在 image 块里出现一次）。

安全门不变：`ask` / `login` / `serve` 是 T1，默认策略 `DenyElevated` 直接拒；7 条只读命令随便调。
也就是「让 Agent 自己查额度、读会话、翻历史」安全，「让它替你发消息、换凭证、开端口」要人点头。

### 接进 dsh 的 MCP 侧

```yaml
- insert:
    - id: mcp-lchatgpt
      name: '@deepseek-ai/dsh-mcp-client'
      config:
        serverName: lchatgpt
        transport: stdio
        command: /绝对路径/lchatgpt
        args: ['--mcp']
        env:
          HOME: /home/lilyco
        startupTimeoutMs: 20000
        toolCallTimeoutMs: 60000
        failOnStartupError: false
```

`serverName` 决定工具前缀：`mcp__lchatgpt__whoami` / `mcp__lchatgpt__show-conversation` …
改完要重启 dsh（桌面版重启 Electron，`--profile desktop` 被 CLI 拒绝）。
patch 层顶层只能改**已有** id，新增条目必须写成 `- insert:` 列表，否则报
`patch: entry "mcp-lchatgpt" not found`。

> **别用「问模型你有哪些工具」来验收。** ChatGPT 网页模型本身不做 function calling，
> 问它工具会一本正经地回答「这些 MCP 工具均未注册」，或者报一堆不存在的能力名（`python.exec` /
> `web.run` / `genui.*`）。判据只能是 dsh 真发出去的请求体（`tools` 数组里有没有
> `mcp__lchatgpt__*`），或者拿一个假 LLM 端点注入 `tool_call` 看它会不会真执行。
> 两条都实测过：请求体 36 个工具里 10 个是我们的；强注 `mcp__lchatgpt__whoami` 之后
> dsh 真的执行了并拿到真实 JSON（`turn_end completed`、退出码 0）。

## 参数语义

- **数字 0 = 用默认值。** `--limit 0` / `--messages 0` 都回退到各自的默认（20 / 20 / 10）。
  这是四端一致性的硬要求：Web / MCP 面省略数字参数时送的就是 `0`。
- **枚举/字符串空串 = 用默认值。** `--order ""` 等价于 `updated`；`--model ""` 等价于
  `gpt-6.2-sol`；`--effort ""` 等价于 `low`；`login --from ""` 等价于 `auto`
  （有 `~/.codex/auth.json` 就导 codex，否则抓浏览器）。理由同上：别的端省略参数只会送空串。
- `--order` 只认 `updated` / `created`，别的值直接 `InvalidArg`，不静默纠正。
- `--messages 0` 与 `--all` 的区别：前者取默认 20 条，后者无视上限取全部。
- `--include-reasoning` 才把 `thoughts` / `reasoning_recap` 节点放进 `messages`；
  默认只出正文（否则一条回答里混进 66 条思考摘要）。
- `ask --allow-codex` 是**唯一的**花 codex 额度的开关：不加就只有 chat 那条路。
  它不是默认值的「切换」，而是显式同意——Flag 省略即 false，别的端（Web / MCP）
  不传就是 false，所以不存在「手滑花了 codex」。
- `ask --cdp` 指定 chat 路径要驱动哪台浏览器；留空则按 `$LCHATGPT_CDP` → ego-lite 配置 →
  常见端口依次找。**显式给了就只试那一个**（和 `login --cdp` 同一条规矩）。
- `ask --model` 只在 `--allow-codex` 时有效，且只接受 `gpt-6.2-sol`；别的 slug 在发请求
  **之前**就报错并说明原因。chat 路径的模型由网页自己选（见 `models` 的 `default_model_slug`）。
- `--effort` 同理只在 `--allow-codex` 时有效。
- `ask --save-images` 给目录就落盘（空串 = 不下载）；`--inline-images` 内联成 base64。
  两个都只影响「图怎么交付」，不影响花哪份额度。
- `show-conversation --inline-images` 同理，读历史里的图并内联（T0，只读）。
- `login --reveal` 才打印完整 token，默认打码；`login --cdp` 指定浏览器端点。
- `serve --port 0`（或省略）= 8790；`--host ""` = `127.0.0.1`（只监听本机）。
  `serve --allow-codex` 决定 `/v1/models` 里有没有 `lchatgpt:codex`，这是**启动时**的开关：
  没开就是没开，请求里写 `lchatgpt:codex` 会 400 并提示加参数。
- `serve --cdp` 和 `serve --effort` 分别透传给 chat 路与 codex 路，语义同 `ask`。
- `serve` 没有 `--json` / `--json-stream`：它不「跑完就退出」，而是阻塞到进程被杀。

## 典型用法

```bash
# 凭证体检：现在用的是哪份、能不能发、什么时候过期
lchatgpt auth

# 这份凭证是谁的
lchatgpt whoami --json

# 额度还剩多少 / 什么时候恢复（发送前先看这条）
lchatgpt usage

# 最近的会话 → 读其中一条（--all 要完整时间线）
lchatgpt list --limit 5
lchatgpt show-conversation --conversation-id 6b068588-ab08-83ea-a7fd-49cdd76f6682 --all

# 按关键词找历史
lchatgpt search --query Temu

# 真正提问：默认花 chat 额度（要一台已登录 chatgpt.com 的 chromium）
lchatgpt ask --prompt "用一句话解释 borrow checker"
lchatgpt ask --prompt "…" --cdp 127.0.0.1:9229

# 生图：提示词写清楚，图在 images[] 里（text 会是空串）
lchatgpt ask --prompt "画一只戴墨镜的橘猫，扁平插画" --save-images /tmp/imgs
lchatgpt ask --prompt "…" --inline-images --json       # 给 MCP 客户端出 image 内容块

# 显式花 codex 额度（--allow-codex 是唯一的开关）
lchatgpt ask --allow-codex --prompt "用一句话解释 borrow checker" --effort low
lchatgpt ask --allow-codex --prompt "…" --json-stream   # SSE 事件流，给脚本 / AI 消费

# 给 dsh 之类的外部客户端当模型提供端（OpenAI 兼容，阻塞不退出）
lchatgpt serve --port 8790              # 只露 lchatgpt:chat
lchatgpt serve --allow-codex            # 额外露出 lchatgpt:codex
# 同一套命令的其他三个面
lchatgpt --tui
lchatgpt --gui        # 浏览器界面（http://127.0.0.1:<port>）
lchatgpt --mcp        # stdio JSON-RPC
lchatgpt --schema     # 导出 JSON Schema
```

## 真实输出

`lchatgpt auth` 人读面只有一行结论（`--json` 要完整清单）：

```
   50%  列出候选源
  100%
  [Info] 在用: file:/home/lilyco/.local/share/lchatgpt/auth.json（send）
```

`lchatgpt auth --json`（这份是 `login --from codex` 收进来的，所以能发也能续）：

```json
{
  "candidates": [
    { "available": false, "note": "环境变量直接给的 token（CI / 临时借用）",
      "path": null, "source": "env:LCHATGPT_ACCESS_TOKEN" },
    { "available": true, "note": "lchatgpt 自己的仓库（lchatgpt login 写入，可自动续期）",
      "path": "/home/lilyco/.local/share/lchatgpt/auth.json",
      "source": "store:/home/lilyco/.local/share/lchatgpt/auth.json" },
    { "available": true, "note": "codex CLI 的登录态（能读也能发，最稳）",
      "path": "/home/lilyco/.codex/auth.json",
      "source": "file:/home/lilyco/.codex/auth.json" }
  ],
  "hints": [],
  "resolved": {
    "account_id": "8f5a5659-8834-4fbe-9684-f9109233a744",
    "can_send": true,
    "capability": "send",
    "expired": false,
    "expires_at": "2026-12-04T11:18:55Z",
    "ok": true,
    "refreshable": true,
    "source": "file:/home/lilyco/.local/share/lchatgpt/auth.json",
    "token": "eyJhbG…iKwg"
  },
  "store_path": "/home/lilyco/.local/share/lchatgpt/auth.json"
}
```

`lchatgpt login --from browser` 从 ego-lite 的 chromium 抓到的登录态（**只能读**，下面是原样输出）：

```
  [Info] 登录态来源: browser
   33%  已从浏览器取到 token
   67%  已写入凭证仓库
  100%
  [Info] 凭证已存入 /home/lilyco/.local/share/lchatgpt/auth.json
  [Info] 能力: read-only（只能读；发消息请 codex login 之后跑 lchatgpt login --from codex）
  [Info] 到期: 2026-12-05T12:13:46Z
{
  "account_id": "8f5a5659-8834-4fbe-9684-f9109233a744",
  "action": "browser",
  "can_send": false,
  "capability": "read-only",
  "email": "lilyco42ovo@gmail.com",
  "expires_at": "2026-12-05T12:13:46Z",
  "refreshable": false,
  "source": "browser:http://127.0.0.1:9229",
  "store_path": "/home/lilyco/.local/share/lchatgpt/auth.json",
  "token": "eyJhbG…8tOA"
}
```

仓库里已经躺着一份**能发**的凭证时，只读的新登录态默认不会盖上去：

```
  [Warn] 新的登录态只能读，仓库里那份能发消息 —— 保留仓库里那份（/home/lilyco/.local/share/lchatgpt/auth.json）。
  真要替换请加 --force
```

`lchatgpt whoami`：

```
   50%  查询配额
  100%
  [Info] 凭证来源: file:/home/lilyco/.codex/auth.json
{
  "account_id": "8f5a5659-8834-4fbe-9684-f9109233a744",
  "country": "US",
  "credential_source": "file:/home/lilyco/.codex/auth.json",
  "email": "lilyco42ovo@gmail.com",
  "name": "lilyco",
  "plan_type": "plus",
  "region": "California",
  "token": "eyJhbG…iKwg",
  "user_id": "user-UECiF0K595YtNMplPo4Pz7yq"
}
```

`lchatgpt usage`（两套额度分开报，实测输出）：

```
  100%
  [Info] codex 额度：可以发送（plan: plus）—— 只影响 `ask --allow-codex`
  [Info] chat 额度：网页默认模型 gpt-6-2-thinking —— `ask` 默认走这条，不花 codex
  [Info]   5 小时窗: 已用 99%（3 小时 12 分后恢复）
  [Info]   周窗: 已用 81%（5 天 20 小时后恢复）
  [Info]   （这两个窗口和上面 codex 那份数值一样：网页额度也记在这套窗口里）
  [Info]   消息条数没有接口可查；能不能发就看 `ask` 成不成
  [Info]   deep_research: 剩 25，2026-12-25T13:16:51Z 重置
  [Info]   file_upload: 剩 80，…
```

codex 那段额度耗尽时会变成：

```
  [Warn] codex 额度已用尽（plus），此时 `lchatgpt ask --allow-codex` 会返回 429，18 分 35 秒后恢复（2026-11-25T12:01:15Z）
```

`lchatgpt ask`（chat 额度那条路的真实回包）：

```json
{
  "cdp": "http://127.0.0.1:9229",
  "complete": true,
  "conversation_id": "6b06e806-87b4-83ea-a959-ec07efa76042",
  "elapsed_ms": 44640,
  "model": "gpt-6-2",
  "polls": 2,
  "quota": "chat",
  "text": "葡萄"
}
```

`quota` 说明这次花的是哪份额度；`conversation_id` 是**REST 认得的那条**（不是 URL 里的
`local-chatgpt:`），可以直接拿去 `show-conversation`。这条链路里几十秒大头都花在
等页面就绪与等会话落库上，跟模型本身的速度无关。

`lchatgpt ask --allow-codex`（codex 额度那条路的真实回包）：

```json
{
  "quota": "codex",
  "model": "gpt-6.2-sol",
  "effort": "low",
  "response_id": "resp_0a7842ede6de4778016b06e760030487d08875ef036d52e854",
  "text": "好",
  "usage": {
    "input_tokens": 34,
    "output_tokens": 5,
    "total_tokens": 39
  },
  "elapsed_ms": 1825,
  "events": 1
}
```

codex 这条路撞额度上限时不会 panic，也不会重试，而是把 429 翻成人话（真跑出来的）：

```
Error: 执行失败: POST /codex/responses → HTTP 429: The usage limit has been reached
ChatGPT 额度已用尽（plan_type: plus），约 1099 秒后（2026-11-25T12:01:15Z）恢复。
只读命令（whoami/usage/models/list/show-conversation/search/auth）不受影响；用 `lchatgpt usage` 看配额详情
```

chat 这条路找不到浏览器时，报错会明确指路：

```
Error: 输入无效: chat 额度这条路要驱动一个已经登录 chatgpt.com 的 chromium：
1. 起 ego-lite（或任意带 --remote-debugging-port 的 chromium）并登录 chatgpt.com
2. 或用 --cdp 指定端点；…
3. 只想用 codex 额度的话，显式加 --allow-codex
试过的端点：
  - http://127.0.0.1:1: 执行失败: 连不上 CDP http://127.0.0.1:1: io: Connection refused (os error 111)
```

`lchatgpt list --limit 3 --json`：

```json
{
  "count": 3,
  "items": [
    {
      "create_time": "2026-11-24T12:57:10.051984Z",
      "id": "6b0589a5-9534-83e9-9d81-3694b581a017",
      "is_archived": false,
      "is_starred": false,
      "model_slug": null,
      "title": "查看ChatGPT会话",
      "update_time": "2026-11-25T07:06:24.265361Z"
    }
  ],
  "limit": 3,
  "offset": 0,
  "order": "updated",
  "total": 4
}
```

## 会话是一张图，不是一条线

`GET /backend-api/conversation/{id}` 的 `mapping` 是 **node-id → { id, parent, children, message }`
的图。一条消息的 `children` 里可能有编辑重答、工具调用、思考摘要多个分支；
`current_node` 指向用户最后看到的那条。所以 `show-conversation` 的做法是：

1. 从 `current_node` 沿 `parent` 一路回溯到根（带 `HashSet` 防环 + 20000 层上限）；
2. 反转成「根 → 叶」；
3. 丢掉 `recipient != "all"` 的节点（那是发给 `web.run` / `api_tool.call_tool` 的调用参数，
   不是给人看的话）；丢掉没有 `message` 的根节点；
4. 只收 `author.role` 为 `user` / `assistant` 且文本非空的节点；
5. `content_type` 为 `text` / `multimodal_text` 取 `parts`（字符串或 `{text}` 对象），
   `code` 取 `text` 字段标成 `kind: "code"`，`thoughts` / `reasoning_recap` 需显式开关。

一份真实会话的实测分布：2265 个节点里 text 273、code 1668、reasoning_recap 66、
thoughts 255、multimodal_text 2；角色分布 user 65 / assistant 1356 / tool 843。
不按上面这套筛，读出来的是「工具参数 + 思考摘要」而不是对话。

## 两种时间戳不能混

同一个 API 里：

- **会话列表**的 `create_time` / `update_time` 是 **ISO-8601 字符串**（`2026-11-24T12:57:10.051984Z`）；
- **消息节点**的 `create_time` 是 **浮点 epoch 秒**（`1795589543.562025`）。

`util::iso8601_any` 两种都吃：字符串原样透传，数字走本地的 civil-from-days 换算
（不引 chrono / time —— 整个 workspace 都没直接依赖它们）。

## 人读面只打 500 字节

`lilyco-cli` 在 human 模式下只在 `Done.result` 小于 500 字节时打印它。所以这里的命令
都自己用 `ctx.log` 把要人看的行打出来（会话标题 + 每轮对话、模型清单、命中摘要），
`--json` / `--json-stream` 才拿完整结构。要完整数据一律显式加 `--json`。

## 已知边界

- **codex 那条路只在 `gpt-6.2-sol` 上验证过**：ChatGPT 账号走 codex responses 时，其他 slug
  （`gpt-5-1` / `gpt-6-2` …）一律 400 `model_not_supported`，所以命令里做了白名单，
  传别的模型直接 `InvalidArg` 而不是发出去再报错。chat 那条路的模型由网页决定，不由 CLI 挑。
- **chat 那条路要一台活着的浏览器**：这是它花 chat 额度的代价。没有浏览器就没有 chat 发送，
  命令会失败并指路 `--allow-codex`；其余 9 条命令完全不受影响（`serve` 只在真有人提问时才碰浏览器）。
- **chat 那条路慢**：实测一条消息 20~45 秒，绝大部分是在等页面就绪、等按钮点亮、等会话落库。
  codex 那条路 2 秒级。
- **浏览器登录态只能读**：它缺 `api.connectors.invoke`，`ask` 会明确拒绝并指向
  `login --from codex`。这不是 bug，是两种 OAuth 客户端的 scope 差别。
- **浏览器登录态不能续期**：`/api/auth/session` 不返回 refresh_token。过期后重新
  `login --from browser` 即可。
- 浏览器抓取是**可选**的：不装 ego-lite、不装 chromium、端口上没人时，`login --from browser`
  会列出试过的端点并失败；其余命令仍可只用 env / 仓库 / codex 登录态。
- 只读命令在额度耗尽时**完全正常**，这也是把只读与发送拆成两个分级的原因。
- `search` 的 `payload.snippet` 是后端把整篇回答压成一行，长的能到几万字符；
  `items[].snippet` 已按 240 字符截断，完整内容在 `items[].payload`。
- **chat 那条路只发纯文本**：输入框支持拖文件、粘图片，但这里只走 `Input.insertText`，
  所以不能上传附件、不能以图问图。**生图是支持的**（图是模型产出的，走 `images[]`）。
- 生图那一轮的正文通常是空字符串 —— 这不是 bug，见上面「生图：图不在正文里」；
  `text` 空、`images[]` 非空时人读面打的是「生成了 N 张图」。
- 轮询会撞 429：早年固定 1.2 秒轮询，一条长任务打到 123 次之后 `GET /conversation/{id}`
  整体 429 半分钟（连 `show-conversation` 都跟着失败）。现在间隔按轮数放宽（1.2s → 2s → 3.5s），
  并且 429 / 5xx 退避重试，请求数从 ~150 降到 ~60。
- `ask` 的 chat 路径**确实**会在网页里留下真实会话（`conversation_id` 就是它）；
  codex 路径是 `store: false` 的无状态调用，不留会话。
