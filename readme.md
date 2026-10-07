<div align="center">
  <img src="docs/banner.svg" width="720" alt="banner">
</div>

# Lilyco

**One struct. Four interfaces (CLI / TUI / Web / MCP). Zero boilerplate. Cross-platform (Windows / Linux / Android).**

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![CI](https://img.shields.io/github/actions/workflow/status/lilyco-42/lilyco/ci.yml?branch=main&label=CI)](https://github.com/lilyco-42/lilyco/actions)
[![Release](https://img.shields.io/github/v/release/lilyco-42/lilyco)](https://github.com/lilyco-42/lilyco/releases)

Lilyco is a Rust framework that generates **CLI**, **TUI**, **Web UI**, and a **standard MCP server** — from a single struct definition. Every app is an AI tool by default: agents (DeepSeek Harness, Claude Code, Cursor…) call it directly through MCP or JSON-stream. Same binary runs on Windows, Linux, and Android (Termux).

---

## Quick Start

```rust
use lilyco::prelude::*;

#[derive(App)]
#[app(run = "run_compress")]
struct ImgCompress {
    #[arg(must_exist = true)]
    input: PathBuf,
    #[arg(default = 75, range = 1..=100)]
    quality: u8,
}

fn run_compress(app: &ImgCompress, ctx: &Context) -> Result<serde_json::Value, AppError> {
    // your logic here; ctx emits progress, logs, cancellation
    Ok(serde_json::json!({ "done": true }))
}

fn main() {
    lilyco::run::<ImgCompress>();
}
```

From one struct you get:
- `imgpress --input photo.jpg --quality 50` — CLI
- Interactive TUI form (when run in a terminal)
- `imgpress --gui` — Web UI with SSE progress
- `imgpress --mcp` — MCP stdio server (AI agents call it directly)

---

## Framework Crates

| Crate | What it does |
|---|---|
| [`lilyco-core`](lilyco-core) | Context, Progress, AppError, traits |
| [`lilyco-macros`](lilyco-macros) | `#[derive(App)]` — generates CLI/TUI/Web/MCP |
| [`lilyco-cli`](lilyco-cli) | clap-based CLI front-end |
| [`lilyco-tui`](lilyco-tui) | crossterm interactive form |
| [`lilyco-gui`](lilyco-gui) | Web UI (SSE progress) |
| [`lilyco-mcp`](lilyco-mcp) | MCP stdio server |
| [`lilyco`](lilyco) | Facade — `lilyco::run::<T>()` dispatches all four ends |

## Application Crates

| Binary | Crate | What it does |
|---|---|---|
| `lbin` | [lilyco-binfmt](lilyco-binfmt) | Office file reader (docx/xlsx/pptx/odt) |
| `lbrush` | [lilyco-brush](lilyco-brush) | Rust-native bash on Windows |
| `lgrep` | [lilyco-grep](lilyco-grep) | Fast grep |
| `lfiles` | [lilyco-files](lilyco-files) | File ops |
| `lplc` | [lilyco-plc](lilyco-plc) | PLC / companion board control |
| `lffmpeg` | [lilyco-ffmpeg](lilyco-ffmpeg) | ffmpeg transcode/resize/trim with live progress |
| `lvision` | [lilyco-vision](lilyco-vision) | Vision / OCR |
| `lsrt` | [lilyco-srt](lilyco-srt) | Auto-subtitle: faster-whisper ASR + ffmpeg burn-in |
| `laic` | [lilyco-aic](lilyco-aic) | Alice in Cradle tooling |
| `lpet` | [lilyco-pet](lilyco-pet) | Pet companion |
| `lmpkg` | [lilyco-mpkg](lilyco-mpkg) | Memory package registry |

Install any app binary:
```bash
cargo binstall lilyco-ffmpeg   # or lilyco-srt, lilyco-brush, ...
```

---

## Documentation

- [Why Lilyco](docs/WHY.md) — the problem and design motivation
- [Quick Start](docs/QUICKSTART.md) — full worked example (imgpress)
- [Architecture](docs/ARCHITECTURE.md) — how the four ends dispatch
- [Architecture detail](docs/ARCH-DETAIL.md) — design principles + ASCII diagram
- [Crate Reference](docs/CRATE-REFERENCE.md) — core traits, types, macros, per-crate API
- [Type → Widget mapping](docs/TYPE-WIDGET.md)
- [AI Integration](docs/AI-INTEGRATION.md) — MCP, JSON Schema, progress protocol
- [Examples](docs/EXAMPLES.md) — more apps built on lilyco
- [DSH Integration](docs/DSH.md)
- [Testing](docs/TESTING.md)
- [Installation](docs/INSTALL.md)
- [Roadmap](docs/ROADMAP.md)
- [Research & references](docs/RESEARCH.md)

---

## License

MIT OR Apache-2.0
