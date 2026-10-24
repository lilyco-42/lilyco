# Agent implementation requirements — Lilyco Tool Hub GUI

- Must read docs/UI_STANDARDS.md BEFORE changing any GUI layout, colors, typography, form inputs, icons, components or bundled assets.
- Use Naive UI as the commercial-grade component library; no hand-drawn UI elements or copied paid template code. Preserve upstream MIT attribution.
- GUI must delegate to the SAME ToolHub registry the AI uses; no separate image/video server. Never run inference on Lilyco business infrastructure.
- No externally served JS, CSS, fonts or UI CDN dependencies in production; all GUI assets are packaged locally.
- Keep local-only binding, strict Origin and per-launch random token authentication, destructive-operation confirmation, backend safety enforcement and no argument history.
- Human usability: typed Schema forms, native file picker, meaningful messages, client preview, responsive desktop/mobile, keyboard navigation. Continue to support raw JSON advanced mode for complex fields.
- All changes require Python unittest suite, Vue typecheck/build, browser smoke test, and GitHub Actions on Windows/Linux.

- Commercial tool candidate priority and license gates: consult docs/COMMERCIAL_LILYCO_TOOLS_2026.md and lilyco-spine/README.md before adapting external creative/audio tools. Preserve third-party attribution; do not monetize unauthorized music sources.
