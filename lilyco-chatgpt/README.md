# lilyco-chatgpt (`lchatgpt`)

**Drive the chatgpt.com account backend from the terminal: read-only conversation history, model list, two quota pools — and a command that actually asks the model, plus an OpenAI-compatible endpoint that lets other hosts (dsh) use the account as a model provider.**

One `#[derive(App)]` struct per command, so every command also shows up as a TUI form, a Web console, and an MCP tool with no extra code.

Everything ships as **one self-contained binary**: no OpenAI API key, no ego-lite install, no chromium install. A browser is **optional in two separate ways**: it is one source of login state (`lchatgpt login --from browser`), and it is what `ask` drives on its default path. Neither is a hard dependency — with nothing listening on a CDP port the chat path fails with an actionable error and every other command keeps working.

## Two quota pools: chat by default, codex only on request

An account has two unrelated quotas, and `lchatgpt` treats them as two ways to send:

| Path | Spends | How it sends | Default? |
|---|---|---|---|
| **chat** | web chat quota | drives a chromium already logged into chatgpt.com: type the prompt, click send, read the answer back over REST | ✅ `lchatgpt ask` |
| **codex** | codex quota | `POST /backend-api/codex/responses` (SSE) | ❌ needs `--allow-codex` |

This is deliberate: **the codex quota is never spent silently.** Without `--allow-codex` the string `codex/responses` does not even appear on the code path (a unit test holds that), and a failed browser lookup only reports the error plus a hint that `--allow-codex` exists.

## Why the chat path needs a browser

Not because of the token — because of the gate. `POST /backend-api/conversation` sits behind a Turnstile challenge plus an "SO" device collector: `POST /backend-api/sentinel/chat-requirements` answers `persona: chatgpt-paid` with `turnstile.required: true`, `proofofwork.required: true` and `so.required: true`. A real browser clears that for free; reimplementing the collector over plain HTTP is not worth it.

So the chat path **lets the page send the message** and only does the typing and the reading: CDP `Input.insertText` into `div[contenteditable="true"]`, click `button[aria-label="Send"]` (a synthetic Enter key event does **not** submit — measured, 40 s of nothing), then poll `GET /backend-api/conversation/{id}` with the CLI's own Bearer until the assistant node reports `status: finished_successfully`. No DOM scraping: message nodes carry no stable attributes and the class names are hashed.

> The id in the URL is not usable: after sending, the page sits on `/c/local-chatgpt%3A<uuid>`, which REST answers with 400 (encoded or not) and 404 (bare uuid). The real id appears ~16 s later in `GET /conversations`, so the CLI snapshots recent ids *before* sending, then polls the list for a new one whose last user message equals the prompt.

Nothing else touches the web chat endpoint: read-only REST is all the other commands need.

## Images: the picture is not in the text

An image turn has **no answer text at all**. Measured mapping for one generation:

```
client-created-root
  └─ user              parts: ["a cat in sunglasses"]                    finished_successfully
      └─ assistant     content_type: code (the draw-tool call)            end_turn: false
          └─ tool      content_type: multimodal_text — the image lives here  recipient: all
              └─ assistant   reasoning_recap
                  └─ assistant  content_type: text, parts: [""]           in_progress ← current_node
```

Two consequences, both learned the hard way:

1. **The tail node never finishes.** `end_turn == true || status == "finished_successfully"` never
   becomes true for an image round, so the poll used to run the full 180 s and still report
   `complete: false`. The criterion is now "a `finished_successfully` node was seen **and** images
   arrived **and** the tail text is empty". With that, the same generation completes in ~34 s.
2. **The same pointer appears twice** (a ghostrider intermediate and a hidden display node), so
   images are de-duplicated by `file_id` (the `sediment://` prefix stripped off).

Fetching the bytes takes two hops and three headers: `GET /backend-api/files/{file_id}/download`
returns a signed `download_url`, and **both** hops need `authorization` / `user-agent` /
`chatgpt-account-id` — miss the last one and it is a flat `403`.

`--save-images DIR` writes the files (MIME sniffed from magic bytes, not trusted from the backend).
`--inline-images` instead embeds `images[].data_base64` + `mime_type` — that exists for the **MCP**
surface: an MCP client has no login state and cannot fetch a `sediment://` pointer, so the bytes have
to travel inside the tool result. `show-conversation --inline-images` does the same for history.

Polling also used to trip a 429: a fixed 1.2 s interval hammered `GET /conversation/{id}` 123 times
in one long round, and the endpoint then answered 429 for about half a minute (taking
`show-conversation` down with it). The interval now relaxes with the round count (1.2 s → 2 s → 3.5 s)
and 429 / 5xx are retried with backoff, roughly halving the request count.

## Commands

| Command | Tier | Purpose |
|---|---|---|
| `whoami` | T0 | which account the credential belongs to (`/me` + `plan_type`); token is masked |
| `usage` | T0 | both quotas side by side: codex (`/codex/usage`) and chat (`/wham/usage` rate limits + `/conversation/init` feature limits), with reset times |
| `models` | T0 | every model this account can pick (`/models`), the web default slug, and the codex whitelist |
| `list` | T0 | recent conversations (`--offset` `--limit` `--order`) |
| `show-conversation` | T0 | render one conversation's **active branch** as a transcript |
| `search` | T0 | keyword search over history |
| `auth` | T0 | **local** credential checkup, no network: every candidate source, which one wins, whether it can send, when it expires, and what to run next |
| `ask` | **T1** | send one prompt — **chat quota by default** (drives a browser, reads the reply over REST), returning `{ quota, conversation_id, model, text, images, complete, polls, elapsed_ms }`. Image turns come back with an **empty `text` and a populated `images[]`**; `--save-images DIR` writes the PNGs, `--inline-images` embeds them as base64. `--allow-codex` switches to SSE codex responses |
| `login` | **T1** | store a login state in lchatgpt's own store (`--from browser` / `codex` / `refresh`) |
| `serve` | **T1** | an **OpenAI-compatible HTTP endpoint** (`127.0.0.1:8790` by default: `/health`, `/v1/models`, `/v1/chat/completions` with SSE, `/v1/images/generations` returning `b64_json`) so a host like dsh can use the account as a model. Models: `lchatgpt:chat` (default) and, only with `--allow-codex`, `lchatgpt:codex`. Blocks until killed — it has no `--json` result |

Reads are T0 so an automated surface (MCP) can use them freely; `ask`, `login` and `serve` are T1 and the default MCP policy (`DenyElevated`) refuses them.

```bash
lchatgpt auth                                    # what credential am I actually using?
lchatgpt login --from codex                      # import ~/.codex/auth.json (read + send + refresh)
lchatgpt login --from browser                    # optional: harvest from a running chromium over CDP
lchatgpt whoami --json
lchatgpt usage
lchatgpt list --limit 5
lchatgpt show-conversation --conversation-id 6b068588-ab08-83ea-a7fd-49cdd76f6682 --all
lchatgpt search --query Temu
lchatgpt ask --prompt "explain the borrow checker in one sentence"          # chat quota
lchatgpt ask --prompt "a cat in sunglasses, flat art" --save-images /tmp/imgs  # image turn: text empty, PNGs in images[]
lchatgpt ask --allow-codex --prompt "explain the borrow checker" --effort low  # codex quota
lchatgpt serve --port 8790                       # OpenAI-compatible endpoint for dsh (blocks)
lchatgpt --tui     # picker
lchatgpt --gui     # web console
lchatgpt --mcp     # stdio JSON-RPC
lchatgpt --schema  # machine contract
```

## Credentials

Four sources, first hit wins (`lchatgpt auth` lists them all):

```
LCHATGPT_ACCESS_TOKEN  (+ optional LCHATGPT_ACCOUNT_ID)   ← CI / borrowed
  ↓
LCHATGPT_AUTH_FILE                                        ← point at any JSON
  ↓
~/.local/share/lchatgpt/auth.json                         ← written by lchatgpt login (0600)
  ↓
~/.codex/auth.json                                        ← produced by `codex login`
```

Those tokens are *not* interchangeable, and the difference is a claim, not a guess:

| Source | client_id | `scp` that matters | read | send over HTTP |
|---|---|---|---|---|
| codex login | `app_EMoamEEZ73f0CkXaXp7hrann` | `api.connectors.invoke` | yes | yes (codex quota) |
| browser session | `app_X8zY6vW2pQ9tR3dE7nK1jL5gH` | `model.request` / `model.read` | yes | **401** |

Those are two different questions: a browser-session token cannot send over HTTP, but the *browser it lives in* can send — because the page sends the message, not the token. That is why the chat path only needs `auth::load()` (read access, to fetch the answer) while the codex path needs `auth::load_for_send()`.

Refresh: only the codex credential carries a `refresh_token`. `ask --allow-codex` refreshes automatically (`ensure_fresh`: untouched while valid, one grant when expired, and a failed refresh does not block the prompt) and `login --from refresh` forces it. `refresh_token` **rotates**, so every refresh writes the new one back to the store. The OAuth *authorize* endpoint is closed to us (`400 invalid_authorize_request` for any PKCE request, and a real browser lands on "add your phone number"), so this crate never offers a login flow — it only moves an existing login state around.

### Harvesting from a browser (optional)

`login --from browser` talks to an **already running** chromium over the DevTools Protocol using a pure-Rust websocket client (tungstenite, `ws://` on loopback needs no TLS):

1. Endpoints: `--cdp` → `$LCHATGPT_CDP` → `cdpPort` from `~/.config/ego-lite/config.json` → `9229/9222/9223/9333`. An explicit `--cdp` is tried **alone** (never falls through, so you cannot get another browser's account by accident).
2. Targets: the `chatgpt.com` page in `GET {cdp}/json/list`, or a new tab opened with `PUT {cdp}/json/new?https://chatgpt.com/` and closed afterwards.
3. `Runtime.evaluate` a same-origin `fetch('/api/auth/session', {credentials:'include'})` and read `accessToken` (6 attempts, 900 ms apart, because a fresh tab needs a moment).
4. On failure it prints every endpoint it tried and why each one failed.

A harvested token can only read and has no `refresh_token`, so `login` refuses to silently downgrade: if the store already holds a send-capable credential, the read-only one is reported and discarded unless you pass `--force`.

## Using the account as a model provider (`serve` → dsh)

`lchatgpt serve` wraps the account in an **OpenAI-compatible HTTP endpoint**, so any client that speaks `/v1/chat/completions` can treat ChatGPT as a model. It exists for hosts like **dsh (DeepSeek Harness)**, whose `llm-pi-ai` plugin serves a route with `api: openai-completions`.

| Endpoint | Purpose |
|---|---|
| `GET /health` | `{ ok, service, quota: "chat" \| "chat+codex", models }` |
| `GET /v1/models` | OpenAI `{ object: "list", data: [...] }` — the shape pi-ai's discovery expects |
| `POST /v1/chat/completions` | SSE (`chat.completion.chunk` … `data: [DONE]`) when `stream: true`, a single `chat.completion` otherwise |
| `POST /v1/images/generations` | generates on the **chat quota**: `{created, data: [{b64_json, revised_prompt}]}`. Only `b64_json` — the upstream download URL is signed and needs the login headers, so handing it to a client is useless |

Quota ownership does not change because there is an HTTP layer in front: **only `lchatgpt:chat` is listed by default**, and it is the browser-driving path. `lchatgpt:codex` appears only when the server was started with `--allow-codex`; asking for it otherwise is a `400` that names the switch. A model the endpoint does not list is never silently substituted.

The image endpoint is the same chat path `ask` uses, so one generation takes ~30 s; when the round produced no image it answers `400` with the text the page returned instead of an empty array, and `response_format=url` is a `400` that explains why.

Multi-turn history is flattened: the chat path can only send one blob and read one blob back, so `messages` are joined into one text (one `User:` / `Assistant:` prefixed block per turn, system first). The codex path keeps turns apart and folds `system` / `developer` into `instructions`.

### Wiring it into dsh

Add a route under `config.providers` of the `llm-pi-ai` entry in `~/.dsh/profiles/<name>/cordis.patch.yml`:

```yaml
      lchatgpt:
        displayName: lchatgpt
        api: openai-completions
        baseURL: http://127.0.0.1:8790/v1
        # the local endpoint ignores keys, but pi-ai needs an apiKey or an Authorization header
        headers:
          Authorization: Bearer unused
        models:
          - id: "lchatgpt:chat"
            input: [text]
            name: ChatGPT (chat quota, drives a browser)
            contextWindow: 128000
            maxTokens: 32000
```

Three things that are easy to get wrong:

1. `api:` must be one of the four protocols this pi-ai build serves (`openai-completions`, `openai-responses`, `anthropic-messages`, `google-generative-ai`). `openai-codex-responses` is **rejected** — that one is reachable only through a catalog provider's OAuth login, not through a patch.
2. **Do not set `apiKeyEnv`.** It is a reference into the harness credential store; with nothing stored you get `MISSING_CREDENTIAL`. A placeholder `headers.Authorization` is enough (pi-ai accepts any non-empty authorization).
3. Restart dsh after editing a patch. The desktop profile is owned by the Electron app (`--profile desktop` is refused on the CLI), so only the UI restart applies it.

Verify without opening a UI — a headless profile plus a temporary overlay answers one prompt and exits:

```bash
lchatgpt serve --port 8790 &
dsh --profile headless --patch /tmp/lchatgpt-provider.yml --json "answer with two words: banana"
# -> {"type":"text",...} ... {"type":"final",...}
```

Measured: one dsh turn sends two POSTs (the conversation and a session-title request); the main one carries 23 tool definitions and `max_completion_tokens: 32000`; the whole turn took 19 s. **The web path reports no token usage**, so `step_end` usage is all zeros — the chatgpt.com web API simply does not expose it.

## Serving MCP (`--mcp`): version negotiation, resources, image blocks

`lchatgpt --mcp` speaks stdio JSON-RPC, and the same registry turns the 10 commands into 10 tools.
Three things a strict client such as dsh checks, all of which had to be added:

1. **Answer with the protocol version the client asked for.** `2025-11-25` / `2025-06-18` /
   `2025-03-26` / `2024-11-05` are supported; `initialize` echoes `params.protocolVersion` and only
   falls back to `2024-11-05` when it does not recognise it. The dsh MCP SDK does the legacy
   handshake and throws `Server's protocol version is not supported` for anything outside its list.
2. **`capabilities` must contain `resources`.** dsh guards on
   `getServerCapabilities()?.resources !== void 0` before it ever sends `resources/list`, so
   capabilities is `{"tools": {}, "resources": {}}`: commands are exposed as resources
   (`lilyco://command/<name>`) and `resources/read` returns that command schema JSON.
3. **Images travel as real content blocks.** A result carrying `{type: "image", data, mimeType}`
   is lifted out of both top-level `images[]` and `messages[].images[]` (png/jpeg/webp/gif only,
   at most 8), and the `data_base64` is then stripped from the text block — a 1.5 MB PNG would
   otherwise double the context (measured: text block 1 559 917 → 624 bytes, image block keeps the bytes).

The safety gate is unchanged: `ask` / `login` / `serve` are T1 and the default `DenyElevated`
policy refuses them, so an agent can read quotas and history freely but cannot send, swap credentials,
or open a port without a human in the loop.

### Wiring the MCP side into dsh

```yaml
- insert:
    - id: mcp-lchatgpt
      name: '@deepseek-ai/dsh-mcp-client'
      config:
        serverName: lchatgpt
        transport: stdio
        command: /absolute/path/to/lchatgpt
        args: ['--mcp']
        env:
          HOME: /home/lilyco
        startupTimeoutMs: 20000
        toolCallTimeoutMs: 60000
        failOnStartupError: false
```

`serverName` sets the tool prefix (`mcp__lchatgpt__whoami`, `mcp__lchatgpt__show-conversation`, …).
Restart dsh afterwards (desktop: restart Electron; the CLI refuses `--profile desktop`). A patch
layer can only modify **existing** ids at the top level — a new entry must be written as a
`- insert:` list, otherwise you get `patch: entry "mcp-lchatgpt" not found`.

> **Never verify this by asking the model which tools it has.** The ChatGPT web model does no
> function calling of its own, so it will earnestly answer that none of the MCP tools are
> registered, or invent capabilities (`python.exec`, `web.run`, `genui.*`). The only honest
> oracle is the request body dsh actually sends (does `tools` contain `mcp__lchatgpt__*`), or a
> fake LLM endpoint that injects a `tool_call` and watches whether it runs. Both were measured:
> 36 tools in the request, 10 of them ours, and an injected `mcp__lchatgpt__whoami` did execute and
> return the real account JSON (`turn_end completed`, exit code 0).

## Two things that bite

- **A conversation is a graph, not a line.** `mapping` is `node-id → {id,parent,children,message}`; `current_node` points at what the user last saw. `show-conversation` walks `parent` back from `current_node`, reverses, then drops tool-call nodes (`recipient != "all"`) and reasoning summaries (unless `--include-reasoning`). Measured on a real conversation: 2265 nodes were text 273, code 1668, reasoning_recap 66, thoughts 255 — filtering is not optional.
- **Two timestamp shapes in one API.** Conversation `create_time`/`update_time` are ISO-8601 strings; message-node `create_time` is a float epoch. Both are normalised locally, without pulling in chrono/time.

Numbers follow the house rule: **`0` means "use the default"** (Web/MCP send 0 for omitted numbers), and an empty string means "use the default" for string args (`--order ""` → `updated`, `login --from ""` → auto).

## Docs

Full command table, parameter semantics, real output, and known limits: [`docs/chatgpt.md`](../docs/chatgpt.md). Four-surface acceptance probe: [`scripts/acceptance/chatgpt_probe.py`](../scripts/acceptance/chatgpt_probe.py).

## Known limits

- The codex path is verified live end to end (`output_text.delta` streaming, `response_id`, token usage), but only on `gpt-6.2-sol` — a ChatGPT account gets `400 model_not_supported` for every other slug, so the command whitelists it. The chat path takes whatever model the web page picks.
- The chat path needs a live, logged-in chromium — that is the price of spending the chat quota. No browser means no chat send (the command fails and points at `--allow-codex`); every other command is unaffected.
- The chat path is slow: one prompt measured 20–45 s, almost all of it waiting for the page, the send button, and the conversation to land. The codex path is ~2 s.
- When a window is exhausted the backend answers `429 usage_limit_reached`; that is reported as structured output with the reset time instead of retrying or panicking. Read-only commands are unaffected.
- A browser login state is read-only and cannot refresh itself; re-run `login --from browser` when it lapses.
- `serve` is not a general OpenAI API shim: no tool calling, no token accounting on the chat path, and a multi-turn history is flattened into one prompt. `stream: true` on the chat path emits the answer in one or two deltas, because the web conversation is polled rather than streamed.
- **Only plain text goes into the composer** (`Input.insertText`): files cannot be uploaded and images
  cannot be used as input. **Generating images is supported** (the model produces them; they arrive in
  `images[]`). An image turn answer text is usually the empty string — that is not a bug, see the
  Images section; when `text` is empty and `images[]` is not, the human surface prints "N images generated". The chat path does leave a real conversation behind (the returned `conversation_id` is it); the codex path is a `store: false` stateless call.
