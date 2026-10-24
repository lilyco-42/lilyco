# Lilyco Tool Hub — Human UI Design System / 商用界面规范
Version 1.0 | 2026-10-24

## 组件来源、许可和升级规则

**唯一默认组件设计系统：Naive UI 2.45.x (Vue 3)**, MIT licensed:
- Official components: https://www.naiveui.com/
- Source and license: https://github.com/tusen-ai/naive-ui
- Commercial use permitted under MIT; preserve package license/attribution notices when bundling.
- Native system open/save file picker via Python tkinter.filedialog, not hand-drawn picker.
- **Do not hand-draw UI controls, invent decorative vector assets, or mix in random third-party templates**. Use Naive UI Card, Form, Input, InputNumber, Switch, Select, Button, Modal, Alert, Tag, DataTable, Empty, Skeleton, Grid, Space and Layout.
- Use the existing MIT-licensed library themes. Do not copy unlicensed premium dashboards or premium UI assets from screenshots.
- Upgrade libraries via reviewed PR and dependency lockfile, check changelog/licensing and run cross-platform checks.

PrimeUI migration note: PrimeVue's older MIT releases remain MIT, but active PrimeUI development has a new Community/Commercial license with eligibility conditions. Do not copy PrimeUI premium themes/components without an appropriate license.

## Product interface
Tool Hub is an OS-local client for BOTH humans and AI. Both surfaces use one ToolHub registry and tool namespaced routing. GUI does NOT introduce a second inference path.

Primary navigation:
- All tools / category filters + search
- Parameter entry with MCP JSON Schema, optional advanced JSON editor
- Results including structured JSON and user-selected local output previews
- Session history: 100 most recent calls, no argument payloads persisted
- Provider health and client location indicator

Tool categories: vision, media, ffmpeg, files, grep, mpkg. New tools are discovered, never hardcoded into frontend logic.

## Visual tokens and layout
- Theme: Naive UI built-in dark, visual accent lavender-blue (`#8299ee`), success teal (`#59c7a9`), dark neutral background `#10151f`.
- Typography: OS system fonts, Segoe UI/Inter/Microsoft Yahei UI fallback. Primary labels >= 14 px, supporting text >= 12 px.
- Density: spacing base 4 px, with 8 / 12 / 16 / 24 / 32 px. Consistent 10 px radius from theme.
- Layout: 252 px desktop sidebar and responsive single-column under 960 px; tool chooser left, invocation right; mobile navigation is provided in the header as compact Naive UI buttons.
- Use default library focus/hover/disabled/feedback rather than hand-drawn states.
- Text contrast: target WCAG AA, 4.5:1 normal body labels and clear strong focus outlines. Don't encode state by color alone.
- Chinese first, maintain raw MCP tool names and description to avoid ambiguity. Never translate argument names used by Schema.
- Include loading/empty/error/success states, confirmation for destructive or code-executing tools.

## Human interaction standard
1. Choose a category, search, select a tool.
2. Each simple JSON Schema field becomes Naive FormItem and an appropriate typed control; required fields clearly marked. Longer/complex schemas use the JSON editor.
3. Local files use OS-native Open/Save picker when possible, with a manual-path fallback for headless devices.
4. Explain what happens before execution; high-risk filesystem actions and Manim/HTML execution require an explicit confirm step. Downstream Lilyco MCP safety gate still applies.
5. Tool status is either running, succeeded, or error; never declare success before the MCP tool has returned.
6. Output stays on client. Preview only files returned by the tool call and registered for this session (40 MiB cap), not arbitrary absolute paths from HTTP queries.
7. AI calls continue to use MCP without the GUI being open; human calls go to the identical local ToolHub backend.

## Security & privacy
- Bind only `127.0.0.1` on a random port, NEVER `0.0.0.0`.
- Auto-generated 256-bit token in URL fragment on launch, replaced immediately and stored only in browser sessionStorage. Every API requires `X-Lilyco-Token`; JSON tool calls also require strict same-origin check.
- Static resources are bundled locally: no CDN JavaScript or assets, no cloud rendering, no fonts fetched from network.
- CSP blocks external scripts/resources, frames and unsafe object embeds; backend refuses arbitrary filesystem access from static URLs.
- Reject unknown endpoints, bad JSON, payloads >256 KiB; do not echo user arguments in session history. Native file chooser runs only after explicit local click.
- Local tool processes run with OS user's rights. **Do not expose the GUI to Internet or LAN, do not launch untrusted scripts.** Store sensitive work in OS-protected folders.
- Heavy image/video/model operations are delegated to local MCP providers, which remain responsible for their permissions and resource limits.

## Mandatory test gates
- `python -m unittest discover -s apps/lilyco-toolhub/tests -v`
- `cd apps/lilyco-toolhub/gui && bun install --frozen-lockfile && bun run typecheck && bun run build`
- Windows/Linux GitHub Actions for Python HTTP tests, GUI build, and MCP tool bundle.
- At least one real Chromium GUI smoke test of the built interface and an actual local MCP tool invocation.

