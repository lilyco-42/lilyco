# Lilyco Manager Technical Specification — v0.2 draft

## Scope
Windows/Linux/macOS local-first manager: launch a loopback HTTP server on an ephemeral port and open the default browser. The browser is the UI, not an Electron bundle. An embedded frontend must not require Node at runtime.

## Stable boundaries
- `tiny_lilyco`: optional no_std domain and protocol building blocks. No tokio, SQLite or Vue dependency.
- `lilyco` facade: future command discovery and execution bridge; existing `CommandSchema` and executor remain sole authorities.
- `manager-server` (currently src/main.rs): Axum REST, authentication, session and event transport.
- `storage`: SeaORM + SQLx SQLite asynchronous history; introduce migrations before schema evolves.
- `frontend`: Vue 3/Vite/Tailwind bundled to a standalone HTML asset at release build time.

## API MVP
- `GET /`: self-contained HTML, no external CDN scripts.
- `GET /api/tools`: {tools:[{name,description,read_only}]}
- `POST /api/tools/:name/run`: {ok:true,output:object}; only allowlisted tools.
- `GET /api/history`: {runs:[{id,tool,output,created_at}]} maximum 50 returned; storage retains the latest 500 rows.
- Auth for /api/: x-lilyco-token; reject missing tokens. State mutations additionally require `Origin` to match `Host` to reduce cross-origin and DNS rebinding risk. Constant-time token comparison remains a follow-up hardening item.

## Lifecycle & security
- Bind loopback only, random port and CSPRNG session token.
- No arbitrary subprocess execution from incoming API.
- Tool permission: read-only default, write operations disabled until explicit user approval and audit trail.
- Persistent DB in chosen application data directory rather than process CWD (planned).
- Background/daemon mode must not spawn a browser.
- Shutdown must flush state and stop worker tasks.

## Build and release
- CI assembles the Vite UI before `cargo build --release`, on Ubuntu, macOS and Windows.
- `index.html` is embedded using `include_str!` from frontend/dist.
- Windows x64, Linux x64 and macOS universal artifacts; interactive browser smoke tests run as a Linux-only gate, while `cargo fmt`/`clippy`/`test`/`build` run on all three hosts.
- Performance budgets (`scripts/perf-check.mjs`) gate bundle size, offline single-file constraints and release binary size.
- Future MCP stdio adaptor must expose schemas, not unrestricted shell execution.

## Roadmap
M0: replace prototype HTML with structured design system.
M1: real Lilyco tool discovery and safe read-only calls.
M2: execution progress and history with SSE.
M3: permissioned MCP/CLI gateway, installation and updating. (install/update + dev tasks done)
M4: optional wtx/Turso comparative benchmarks and backend ADR.

## Design system v1

- Tokens are vendored from `lilyco-42/ui-spec` into `frontend/src/design/tokens.css` with an
  `lc-` prefix to avoid Tailwind `@theme` self-reference. Light is the default; the dark block
  lives under `html[data-theme="dark"]`.
- Tailwind v4 `@theme inline` maps tokens to semantic utilities: `bg-canvas`, `bg-surface`,
  `text-ink`, `text-ink-soft`, `border-line`, `bg-brand`, `text-brand`, `bg-brand-soft`,
  `text-success`, `text-warning`, `text-danger`, plus radius tokens.
- A shadcn-compatible bridge defines `--background`, `--foreground`, `--card`, `--primary`,
  `--muted`, `--border`, `--ring`, etc.; `bg-background` / `text-foreground` are available.
- Shared component classes (`.btn`, `.btn-brand`, `.btn-secondary`, `.card`, `.badge`,
  `.alert`, `.log`) keep focus rings, disabled contrast and 44px-friendly touch targets.
- Theme preference is stored in `localStorage['lilyco-theme']`; `index.html` applies it before
  first paint to avoid a flash.

## Application lifecycle v1

Manifests (`lilyco.app.json`) are described in `docs/MANIFEST.md`. The manager loads them from
`LILYCO_MANAGER_APPS` or the per-user config directory at startup and exposes:

- `GET /api/apps` — manifest summary, binary installed state, supervised process status, warnings.
- `GET /api/apps/:id/schema` — fixed `--schema` argv, 5s timeout, 512KB cap, JSON only.
- `POST /api/apps/:id/start` — fixed argv/env from the manifest; 409 if already running.
- `POST /api/apps/:id/stop` — only processes started by this manager; graceful SIGTERM, then 3s kill timeout.
- `GET /api/apps/:id/logs?limit=N` — bounded stdout/stderr ring buffer (max 500 lines).

The HTTP layer never accepts an executable name, argv or env. Manifest ids are validated
(`[a-z0-9]` start, `-`/`.` allowed) and binaries must be bare PATH names.

## Release installation v1

- `GET /api/apps/:id/release` — GitHub latest release, current managed tag, platform asset,
  update availability. Network failures are returned as data, not a 500.
- `POST /api/apps/:id/install` — serialized by one process-wide install lock. Stops a
  manager-supervised instance first, downloads the selected asset, requires `sha256:` digest
  from the GitHub API, verifies before extraction, then atomically writes `current.json`.
- Archives: tar.gz, zip, or a raw binary. Extraction caps: 256MB download, 512MB expanded,
  5000 entries, depth 4. `enclosed_name` prevents zip traversal.
- Managed layout: `<data>/lilyco/apps/<id>/{downloads,releases/<tag>,current.json}`.
- `POST /api/apps/:id/uninstall` removes the manager-owned directory only. It stops a
  manager-supervised instance first and never touches the source repository or a PATH binary.
- Binary resolution order: `LILYCO_MANAGER_BIN_<ID>` > managed install > `PATH`.

## Development tasks v1

- `GET /api/apps/:id/dev` — fixed task names/argv from the manifest plus recent jobs.
- `POST /api/apps/:id/dev/:task` — runs a fixed argv in the manifest's repository directory.
  One running job per (app, task) returns 409. Output is captured into a 500-line ring buffer;
  the 32 newest jobs are retained.
- `GET /api/dev/jobs/:job/logs?limit=N` — job status, exit code and bounded lines.
- The HTTP layer selects only a task name. It cannot supply argv, cwd or env.

## MCP aggregation v1

- `GET /api/mcp/tools` — for every installed manifest with `mcp`, spawn its fixed `--mcp` argv,
  perform `initialize` + `tools/list`, namespace tools as `<app>__<tool>`, then terminate the
  child. 10s timeout, 1MB per response line, 200 tools per app, 1000 total.
- `GET /api/mcp/config` — builds a `tools.json` `servers` map for `lilyco-toolhub`.
- `POST /api/mcp/config` — writes the map to `$XDG_CONFIG_HOME/lilyco/tools.json` (or `$HOME/.config`).
- Client snippet is produced only when `LILYCO_TOOLHUB` points to an absolute existing `hub.py`.

MCP has no global tool-name registry; namespacing and dynamic `tools/list` discovery are
mandatory. Duplicate aliases are skipped instead of silently overwriting.

## Ecosystem discovery v0
The embedded ecosystem.json is a reviewed snapshot of PUBLIC GitHub repositories only. GET /api/ecosystem returns project metadata to authenticated local users. 'discovered' never implies installed, integrated, executable or trustworthy. Private repositories and secrets must never be bundled into the public repository or release. Future adapters require reviewed manifests, capabilities, signatures, explicit permissions, and sandboxing.

## Integration adapters v1
GET /api/adapters reports installed/unavailable for whitelisted binaries. Tools desktop.status and lyco.version have fixed argv and bounded subprocess timeouts. This is not a general subprocess runner. Adapter binaries are trusted executable code; installation is an explicit user action, not triggered by visiting a web page.
