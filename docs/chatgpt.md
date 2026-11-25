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

## 命令（9 条：7 条 T0 只读 + `ask` / `login` T1 需确认）

| 命令 | 分级 | 作用 |
|---|---|---|
| `whoami` | T0 | `GET /backend-api/me` + 配额里的 `plan_type`；返回 `{ email, name, user_id, country, region, plan_type, account_id, credential_source, token }`。`token` 打码成「前 6 位…后 4 位」，`credential_source` 说明凭证是环境变量还是哪份文件读来的 |
| `usage` | T0 | 两套额度分开报。codex 段 `GET /backend-api/codex/usage`（`allowed / limit_reached / credits / primary_window / secondary_window / model_usage`，每窗口带 `used_percent / limit_window_seconds / reset_at / reset_at_iso / reset_in_human`）；chat 段 `GET /backend-api/wham/usage`（网页自己读的那份 rate_limit，结构同 codex 段）+ `POST /backend-api/conversation/init`（`default_model_slug` 与 `limits_progress[]` 的 `feature / remaining / reset_after`）。两套都**没有消息条数**维度 |
| `models` | T0 | `GET /backend-api/models`；返回 `{ title, count, default_model_slug, codex_models, models }`。`codex_models` 是 `ask --allow-codex` 能用的 slug 白名单；`default_model_slug` 是 `ask`（chat 额度）会用的网页默认模型 |
| `list` | T0 | `GET /backend-api/conversations?offset&limit&order`；返回 `{ total, offset, limit, order, count, items }`，每项 `id / title / create_time / update_time / is_starred / is_archived / model_slug`。只有元数据，正文要 `show-conversation` |
| `show-conversation` | T0 | `GET /backend-api/conversation/{id}`；沿 `current_node` 的 `parent` 链回溯出**活动分支**，只留发给用户的 user/assistant 文本，返回 `{ conversation_id, title, create_time, update_time, model_slug, node_count, message_count, returned, truncated, messages }` |
| `search` | T0 | `GET /backend-api/conversations/search?query=…`；返回 `{ query, limit, count, items }`，每项 `conversation_id / title / update_time / is_starred / snippet / payload`。`snippet` 由后端给、按 240 字符截断 |
| `auth` | T0 | **本地**凭证体检，不联网：返回 `{ resolved: { ok, source, capability, can_send, account_id, expires_at, expired, refreshable, token }, candidates, store_path, hints }`。`candidates` 逐条列出 env / 仓库 / codex 登录态哪个可用，`hints` 直接给下一步该跑什么 |
| `ask` | **T1** | **默认花 chat 额度**：驱动浏览器发、REST 读回，返回 `{ quota: "chat", conversation_id, model, text, complete, cdp, polls, elapsed_ms }`（`--cdp` 指定浏览器端点）。加 `--allow-codex` 才改走 `POST /backend-api/codex/responses`（SSE），返回 `{ quota: "codex", model, effort, response_id, text, usage, elapsed_ms, events }`，`--effort` 取 low/medium/high/xhigh/max/ultra |
| `login` | **T1** | 把一份登录态收进自己的仓库（`~/.local/share/lchatgpt/auth.json`，0600）。`--from browser` 从正在跑的浏览器抓、`--from codex` 导入 codex 登录态、`--from refresh` 立刻用 refresh_token 续期。返回 `{ action, source, capability, can_send, account_id, email, expires_at, store_path, token, refreshable }` |

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
- `login --reveal` 才打印完整 token，默认打码；`login --cdp` 指定浏览器端点。

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

# 显式花 codex 额度（--allow-codex 是唯一的开关）
lchatgpt ask --allow-codex --prompt "用一句话解释 borrow checker" --effort low
lchatgpt ask --allow-codex --prompt "…" --json-stream   # SSE 事件流，给脚本 / AI 消费

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
  命令会失败并指路 `--allow-codex`；其余 8 条命令完全不受影响。
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
- 不处理附件、不读图。`ask` 的 chat 路径**确实**会在网页里留下真实会话（`conversation_id` 就是它）；
  codex 路径是 `store: false` 的无状态调用，不留会话。
- chat 那条路只发纯文本：输入框支持粘贴图片等，但这里只走 `Input.insertText`。
