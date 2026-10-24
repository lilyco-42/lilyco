# Lilyco Spine — local AI-callable asset metadata inspections

`lspine` is a standalone Lilyco binary that registers two tools:
- `inspect-skeleton`: inspect local Spine JSON metadata. Lists version, bones, slots, animation and skin names, constraints. Supports Spine JSON files `.json` and `.json.bytes` (binary `.skel` is NOT supported yet).
- `inspect-atlas`: summarize plain-text atlas pages and region names and detect missing page images.

Both implement `#[derive(App)]`, so they integrate with Lilyco's CLI/TUI/Web/MCP architecture and are accessible from Tool Hub as `spine__inspect-skeleton` and `spine__inspect-atlas`.

Usage:

```bash
lspine --list
lspine --mcp
```

The tools are read-only, bounded to 32 MiB inputs, work entirely on the user's computer, and never require proprietary Spine Runtime or third-party animation editor assets.

**Licensing boundary:** These two tools only parse documented JSON/atlas text. They are NOT equivalent to a real Spine renderer, editor or viewer. Before adding rendering, exporting or Spine Runtime SDK integration, review https://esotericsoftware.com/spine-editor-license and confirm appropriate license holder status, version pairing and downstream distribution requirements.

Commercial roadmap: multi-version compatibility diagnosis, dangling atlas regions, frame export/CI batch review, timeline export (licensed runtime or suitable independent implementation), Unity/Godot pipeline checks. Current release is the first low-risk slice, not a finished commercial Spine editor.
