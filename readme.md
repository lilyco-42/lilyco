# Triforge

**Write once, run anywhere. CLI, TUI, Web — auto-generated from your Rust types.**

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

---

## What problem does this solve?

You write a Rust tool — say a video transcoder. You define a struct with fields:

```rust
struct Transcode {
    input: PathBuf,
    codec: Codec,
    quality: u8,
    dry_run: bool,
}
```

To make this usable, you need:

- A **CLI** so scripts and AI agents can call it (e.g., `transcode --input a.mp4 --codec h265`)
- A **TUI** so terminal users get an interactive form with real-time preview
- A **Web UI** so coworkers can open a browser and click buttons
- A **JSON Schema** so LLMs can discover the interface via function calling

That's 4 interfaces. Triforge generates all 4 from the same struct definition.

## Quick Start

```bash
cargo new mytool && cd mytool
cargo add triforge-core triforge-macros triforge-cli serde_json
```

```rust
// src/main.rs
use triforge_macros::{App, ValueEnum};
use triforge_core::prelude::*;
use std::path::PathBuf;

#[derive(ValueEnum)]
enum Codec { H264, H265, Av1 }

#[derive(App)]
#[app(about = "Transcode video files")]
struct Transcode {
    #[arg(about = "Input file", must_exist = true)]
    input: PathBuf,

    #[arg(about = "Output codec", default = "h264")]
    codec: Codec,

    #[arg(about = "Quality 0-51", default = 23, range = 0..=51)]
    quality: u8,

    #[arg(about = "Preview only, don't transcode")]
    dry_run: bool,
}

fn main() { triforge_cli::run::<Transcode>(); }
```

```bash
# CLI mode (default)
$ cargo run -- --input video.mp4 --codec h265 --quality 18 --dry-run

# Schema export for AI function calling
$ cargo run -- --anthropic-tool
$ cargo run -- --openai-tool

# JSON output for scripts
$ cargo run -- --json --input video.mp4 --output out.mp4
```

## Three Interfaces, One Definition

### CLI

```console
$ transcode --input video.mp4 --codec h265 --quality 18 --dry-run
{ "dry_run": true }

$ transcode --schema
{
  "name": "Transcode", "about": "Transcode video files",
  "args": [
    { "name": "input", "kind": {"type":"path","must_exist":true}, "required": true },
    { "name": "codec", "kind": {"type":"enum","values":["h264","h265","av1"]}, "default": "h264" },
    { "name": "quality", "kind": {"type":"number","min":0,"max":51}, "default": 23 },
    { "name": "dry_run", "kind": "flag" }
  ]
}
```

### TUI (terminal form)

```
 Transcode — Transcode video files
 $ transcode --input video.mp4 --codec h265 --quality 18
────────────────────────────────────────────────────────
          (*) input: [video.mp4________________________]
              codec: [h264] h265 [Av1]                  (←→ to choose)
           quality: [18]                                (↑↓ to adjust)
           dry_run: [x]                                 (Space to toggle)
────────────────────────────────────────────────────────
 [Tab] Switch  [Enter] Confirm  [Esc] Quit  [F1] Help
```

### Web UI

```console
$ cargo run --features gui
Triforge GUI ready: http://localhost:8080
```

Dark-themed form. Fill fields, click Run, progress streams via SSE.

## Type → Widget Mapping

| Rust Type | CLI | TUI | Web |
|-----------|-----|-----|-----|
| `bool` | `--flag` | `[x]` checkbox | `<input type=checkbox>` |
| `String` | `--name <val>` | text input | `<input type=text>` |
| `u8`/`f64`/... | `--count <num>` | number ± arrows | `<input type=number>` |
| custom enum | `--mode <choice>` | ←→ cycle | `<select>` dropdown |
| `PathBuf` | `--file <path>` | text input | `<input type=text>` |
| `Vec<T>` | `--tag a --tag b` | multi-line (Enter/Delete) | dynamic inputs |

## AI-Ready by Design

```bash
$ mytool --anthropic-tool
```

```json
{
  "name": "transcode",
  "description": "Transcode video files",
  "input_schema": {
    "type": "object",
    "properties": {
      "input": { "type": "string", "description": "Input file" },
      "codec": { "type": "string", "enum": ["h264", "h265", "av1"] },
      "quality": { "type": "number", "minimum": 0, "maximum": 51 },
      "dry_run": { "type": "boolean" }
    },
    "required": ["input"]
  }
}
```

Valid Anthropic tool use / OpenAI function calling definition. Paste it into your API call — the model can invoke your Rust tool directly.

## Progress Protocol

All three interfaces consume a unified `Progress` enum:

```rust
ctx.emit(Progress::Started { total: Some(100), message: Some("Encoding...".into()) });
for frame in 0..100 {
    if ctx.is_cancelled() { return Err(AppError::Cancelled); }
    ctx.tick(frame, Some(100), format!("Frame {frame}"));
}
ctx.done(serde_json::json!({"size_mb": 42}), 3200);
```

| Interface | Progress Display |
|-----------|-----------------|
| CLI (`--json-stream`) | One JSON object per line to stdout |
| TUI | Progress bar + scrolling log + elapsed time |
| Web | Progress bar (SSE) + scrollable log panel |

## Crate Structure

| Crate | Purpose | Status |
|-------|---------|--------|
| `triforge-core` | Core traits (`App`, `ValueEnum`), types (`ArgSchema`, `Progress`), `Context` | ✅ |
| `triforge-macros` | `#[derive(App)]`, `#[derive(ValueEnum)]` proc macros | ✅ |
| `triforge-cli` | CLI renderer → `clap::Command`, schema/tool export, `--json-stream` | ✅ |
| `triforge-tui` | TUI renderer → ratatui form, live CLI preview, 11 tests | ✅ |
| `triforge-gui` | Web GUI → axum server, embedded HTML, SSE progress | ✅ |

## Current Limitations

- **`#[derive(App)]` only works on named-field structs** — no tuple structs or enums
- **Number range validation** works at CLI (clap) but not in TUI/Web widgets yet
- **Subcommands** in CLI only — TUI/Web not yet
- **Web GUI `run()`** uses mock runner — real App trait dispatch pending
- **Macro-generated `run()`** calls `unimplemented!()` — provide your own impl
- **Not yet on crates.io** — use git dependency for now

## Running Tests

```bash
cargo test --workspace
```

All 58 tests pass across 4 crates.

## Installation

Use git dependency until published to crates.io:

```toml
[dependencies]
triforge-core = { git = "https://github.com/lilyco-42/lilyco" }
triforge-macros = { git = "https://github.com/lilyco-42/lilyco" }
triforge-cli = { git = "https://github.com/lilyco-42/lilyco" }
triforge-tui = { git = "https://github.com/lilyco-42/lilyco" }
```

## License

MIT OR Apache-2.0, at your option.
