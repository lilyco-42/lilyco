# Lilyco Manager UI Design Specification v1

## Direction
Desktop developer console: quiet, crisp, technical. No saturated gradients or playful dashboard ornaments. Inspiration: professional editors and infrastructure management consoles, not landing pages.

## Implementation
Vue 3 SFC + TypeScript + Vite + Tailwind CSS 4; use lucide icons, reusable layout/card/button classes. Avoid unsafe HTML insertion, inline event strings and raw script bundles in Rust.

## Visual tokens
- Source of truth: `lilyco-42/ui-spec`, vendored in `frontend/src/design/tokens.css`.
- Light first: canvas `#ffffff`, canvas-soft `#f7f7f5`, surface `#ffffff`, ink `#202123`, ink-soft `#62635f`, line `#e6e6e2`, brand `#167153`.
- Dark via `html[data-theme="dark"]`: canvas `#202123`, surface `#292a2d`, ink `#f4f4f2`, line `#3b3c3f`, brand `#67c79e`.
- Semantic status colors: success/warning/danger/info each with a soft background token.
- Radius: 10/12/20/999 px; spacing: 4/8/12/16/24/32/58; type scale: 12/14/16/20/28 px.
- Color is semantic; use icon + text for status. Maintain WCAG AA contrast target; disabled states use tokens rather than opacity.

## Layout
- Left persistent navigation (Overview, Tools, Runs, MCP, Settings); mobile collapses into horizontally scrollable navigation.
- Header: page title, description, refresh and connectivity status.
- Overview: real operational metrics only. Never show invented counts.
- Tool list: search, name, description, read-only status, run action; loading/empty/error states.
- Tool result: monospace formatted JSON, bounded scroll, copy affordance.
- Runs: explicit timestamps, errors and execution results, most recent first.
- MCP/Settings: clearly marked Not available when backend missing; never show fake enabled controls.

## Interaction contract
- Loading disables duplicate submissions.
- Errors shown inline, not just console.
- All controls reachable by keyboard and show focus ring.
- Avoid leaking API token into URLs sent to third parties. No third-party assets.
- No hidden app state: backend truth populates every operational metric.

## Agent completion checklist
[x] Frontend builds and loads offline from Rust binary
[x] Small 375px and wide 1440px layouts verified
[x] Keyboard and accessible labels
[x] Dark/light token compatibility
[x] Tool call + history end-to-end smoke verified
[x] Screenshots reviewed before shipping

## Applications view

- List manifests with name, id/binary, description, capability badges and installed state.
- Show real supervised state only: running PID, exit code, log line count.
- Actions: start, stop, logs, schema, open (loopback URL only). Disable start when not installed.
- Schema result shows the real command count; never invent a count before the call succeeds.
- Manifest warnings render inline as an amber list, not as a success state.
- Release panel shows current/latest tag, platform, matched asset and the real update state.
  Install/update is disabled until a release check succeeds; errors are shown inline.
  Managed installs also expose a confirmed uninstall action that removes only the managed copy.
- Development-task panel lists only manifest tasks; running a task shows live bounded output,
  exit code and a disabled state until the job finishes.

## Design system usage

- Prefer semantic utilities (`bg-canvas`, `bg-surface`, `text-ink`, `text-ink-soft`,
  `border-line`, `bg-brand`, `text-brand`, `badge-success`) over hard-coded colors.
- Shared classes: `.card`, `.btn`, `.btn-brand`, `.btn-secondary`, `.btn-danger`, `.badge`,
  `.alert`, `.log`. Do not recreate button/input styling inline.
- Theme toggle lives in the sidebar and mobile header; persist in `localStorage['lilyco-theme']`.

## MCP view

- Provider cards show discovery success/failure per app and the real tool count.
- Tools are grouped by app and display the namespaced name (`app__tool`).
- Config panel shows the generated `servers` map or client snippet; copy and write actions
  report success/failure inline. When `LILYCO_TOOLHUB` is unset, show the hint instead of a
  fake enabled control.

## Ecosystem registry view
Show source repositories with name, category, description, upstream URL, and explicit 'discovered / not integrated' state. No run/install buttons until a vetted adapter exists. Do not label a repository supported because its name begins with lyco/lilyco.

