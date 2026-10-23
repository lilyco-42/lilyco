# Lilyco Manager UI Design Specification v1

## Direction
Desktop developer console: quiet, crisp, technical. No saturated gradients or playful dashboard ornaments. Inspiration: professional editors and infrastructure management consoles, not landing pages.

## Implementation
Vue 3 SFC + TypeScript + Vite + Tailwind CSS 4; use lucide icons, reusable layout/card/button classes. Avoid unsafe HTML insertion, inline event strings and raw script bundles in Rust.

## Visual tokens
- Dark bg #0b1220; surface #141d2d; border #293449; primary #7c9cff; text #e9eefb; muted #9aa7bf.
- Light bg #f5f7fb; surface #ffffff; border #dbe1eb; primary #425cbe; text #172238; muted #65728a.
- Radius: 10-14 px; spacing: 4/8/12/16/24/32; type scale: 12/14/16/20/28 px.
- Color is semantic; use icon + text for status. Maintain WCAG AA contrast target.

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
[ ] Frontend builds and loads offline from Rust binary
[ ] Small 375px and wide 1440px layouts verified
[ ] Keyboard and accessible labels
[ ] Dark/light token compatibility
[ ] Tool call + history end-to-end smoke verified
[ ] Screenshots reviewed before shipping
