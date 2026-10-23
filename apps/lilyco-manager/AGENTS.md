# Lilyco Manager Agent Rules

Read `../../AGENTS.md`, `docs/TECHNICAL_SPEC.md`, and `docs/UI_SPEC.md` before making changes. These requirements apply to humans and all coding agents (Codex, Claude, Cursor, Copilot, AgentDock).

## Required engineering constraints
- Preserve the `tiny_lilyco` `no_std` boundary; web/database/UI exist only in this standalone application.
- Use Axum + SeaORM (SQLx SQLite) for the current working backend. Swapping wtx/Turso requires an ADR and CI proof.
- Reuse Lilyco facade, `CommandSchema::validate_args`, and core executor when integrating real commands. Never execute arbitrary shell strings from HTTP.
- Browser process listens only on 127.0.0.1, must authenticate all non-public APIs, and must validate Origin/Host for state-changing browser requests. Never place tokens in logs.
- Only registered, read-only sample tools may run until a permission layer is implemented.
- UI is Vue 3 + TypeScript + Vite + Tailwind. Prefer reusable components; no inline monolithic HTML or JS string rendering. Follow `docs/UI_SPEC.md`.
- Keep UI text in Chinese and stable identifiers/protocol fields in English.
- Do not claim a feature works without CI and (for GUI behavior) browser verification.
- Do not build locally in this project; use GitHub Actions for compile/test, except source formatting and frontend dependency resolution.
- Commit only deliberate source files; never commit secrets, `node_modules`, `dist`, or local databases.

## Mandatory review gates
1. Cargo fmt, clippy -D warnings, test, release compilation on Windows + Linux.
2. Frontend npm/bun frozen dependency install + production build; fail CI on any build error.
3. API contract tests for auth failure, tool listing, read-only invocation, history.
4. Accessibility: keyboard navigation, visible focus, color contrast, narrow widths, dark/light.
5. Update TECHNICAL_SPEC / UI_SPEC when architecture or interactions change.

See also `../../docs/CODEGRAPH.md` for framework-level architecture.

- Ecosystem catalog must contain PUBLIC repos only; do not commit or embed private repository metadata. Discovery status is never execution permission.

- Fixed executable adapter calls may only expose reviewed read-only operations. Never accept executable names or argv from HTTP clients. Keep 4-second timeout, bounded output and explicit installed/unavailable states.

- For any Canvas/API relay work, read docs/integrations/CANVAS_RELAY.md. Never expose billing provider secrets, treat Canvas keys as client-visible, and verify model capability and real usage before paid release.


- Local Commerce Studio uses browser Canvas2D and opt-in WebGPU for exposure only; no network image uploads. See docs/integrations/LOCAL_COMMERCE_STUDIO.md. Never market this as full WebGPU/WASM AI masking before model tests.
- New project catalog entries require provenance and manual approval; third-party project pages are links, never code execution. Validate catalog in CI.

