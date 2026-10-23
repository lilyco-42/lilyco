# Lilyco Manager Technical Specification — v0.2 draft

## Scope
Windows/Linux local-first manager: launch a loopback HTTP server on an ephemeral port and open the default browser. The browser is the UI, not an Electron bundle. An embedded frontend must not require Node at runtime.

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
- `GET /api/history`: {runs:[{id,tool,output,created_at}]} maximum 50.
- Auth for /api/: x-lilyco-token; use constant-time token comparison as follow-up hardening, reject missing tokens. Protect against cross-origin and DNS rebinding before production.

## Lifecycle & security
- Bind loopback only, random port and CSPRNG session token.
- No arbitrary subprocess execution from incoming API.
- Tool permission: read-only default, write operations disabled until explicit user approval and audit trail.
- Persistent DB in chosen application data directory rather than process CWD (planned).
- Background/daemon mode must not spawn a browser.
- Shutdown must flush state and stop worker tasks.

## Build and release
- CI assembles the Vite UI before `cargo build --release`.
- `index.html` can be embedded using `include_str!` from frontend/dist.
- Windows x64 EXE and Linux x64 artifact; interactive browser smoke tests are a separate gate.
- Future MCP stdio adaptor must expose schemas, not unrestricted shell execution.

## Roadmap
M0: replace prototype HTML with structured design system.
M1: real Lilyco tool discovery and safe read-only calls.
M2: execution progress and history with SSE.
M3: permissioned MCP/CLI gateway, installation and updating.
M4: optional wtx/Turso comparative benchmarks and backend ADR.

## Ecosystem discovery v0
The embedded ecosystem.json is a reviewed snapshot of PUBLIC GitHub repositories only. GET /api/ecosystem returns project metadata to authenticated local users. 'discovered' never implies installed, integrated, executable or trustworthy. Private repositories and secrets must never be bundled into the public repository or release. Future adapters require reviewed manifests, capabilities, signatures, explicit permissions, and sandboxing.

## Integration adapters v1
GET /api/adapters reports installed/unavailable for whitelisted binaries. Tools desktop.status and lyco.version have fixed argv and bounded subprocess timeouts. This is not a general subprocess runner. Adapter binaries are trusted executable code; installation is an explicit user action, not triggered by visiting a web page.
