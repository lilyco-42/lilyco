# Lilyco

**One struct. Three interfaces. Zero boilerplate.**

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-72%20passed-green)](https://github.com/lilyco-42/lilyco)

Lilyco is a Rust framework that generates **CLI**, **TUI**, and **Web UI** — plus **AI function-calling schemas** — from a single struct definition. You write the business logic once; the framework handles everything else.

---

## Table of Contents

- [Why Lilyco](#why-lilyco)
- [Quick Start](#quick-start)
- [Architecture](#architecture)
- [Crate Reference](#crate-reference)
  - [lilyco-core](#lilyco-core)
  - [lilyco-macros](#lilyco-macros)
  - [lilyco-cli](#lilyco-cli)
  - [lilyco-tui](#lilyco-tui)
  - [lilyco-gui](#lilyco-gui)
- [Type → Widget Mapping](#type--widget-mapping)
- [AI Integration](#ai-integration)
- [Progress Protocol](#progress-protocol)
- [Examples](#examples)
- [Testing](#testing)
- [Installation](#installation)
- [Limitations & Roadmap](#limitations--roadmap)

---

## Why Lilyco

A typical Rust CLI tool needs about 200 lines of clap boilerplate before the first line of actual logic. Add a TUI? Another 400 lines. A web dashboard? A different codebase entirely. Want LLMs to call your tool? You're writing JSON Schema by hand.

Lilyco collapses all of this into a single `#[derive]`:

```rust
#[derive(App)]
#[app(about = "Compress image files", run = "compress")]
struct ImgCompress {
    #[arg(about = "Input file", must_exist = true)]
    input: PathBuf,

    #[arg(about = "Quality 1-100", default = 75, range = 1..=100)]
    quality: u8,

    #[arg(about = "Output format", default = "jpeg")]
    format: Format,

    #[arg(about = "Dry run")]
    dry_run: bool,
}
```

From this you get:

- `imgpress --input photo.jpg --quality 50 --format webp` — CLI
- Interactive TUI form with live command preview — TUI
- Browser-based form with SSE progress — Web
- Valid Anthropic/OpenAI tool definition — AI

---

## Quick Start

Create a new project and add the dependencies:

```bash
cargo new imgpress && cd imgpress
cargo add lilyco-core lilyco-macros lilyco-cli serde serde_json image
```

Paste this into `src/main.rs`:

```rust
use std::path::PathBuf;
use std::time::Instant;
use image::{DynamicImage, GenericImageView};
use image::imageops::FilterType;
use lilyco_core::prelude::*;
use lilyco_macros::{App, ValueEnum};

// 1. Define your types
#[derive(Debug, ValueEnum)]
enum Format { Jpeg, Png, Webp }

#[derive(App)]
#[app(about = "Compress image files", run = "compress")]
struct ImgCompress {
    #[arg(about = "Input image", must_exist = true)]
    input: PathBuf,

    #[arg(about = "Quality 1-100", default = 75, range = 1..=100)]
    quality: u8,

    #[arg(about = "Output format", default = "jpeg")]
    format: Format,

    #[arg(about = "Max width, 0 = no resize")]
    width: u32,

    #[arg(about = "Dry run")]
    dry_run: bool,
}

// 2. Write your business logic
fn compress(app: &ImgCompress, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    ctx.emit(Progress::Started { total: Some(3), message: None });

    let data = std::fs::read(&app.input)?;
    let img = image::load_from_memory(&data)
        .map_err(|e| AppError::Runtime(format!("decode: {e}")))?;

    ctx.tick(1, Some(3), "Resizing...");
    let img = if app.width > 0 && app.width < img.width() {
        let ratio = app.width as f64 / img.width() as f64;
        let h = (img.height() as f64 * ratio) as u32;
        img.resize_exact(app.width, h.max(1), FilterType::Lanczos3)
    } else { img };

    ctx.tick(2, Some(3), "Encoding...");
    let out_path = app.input.with_file_name(format!("compressed.{}",
        if matches!(app.format, Format::Jpeg) { "jpg" } else { "png" }));
    img.save(&out_path).map_err(|e| AppError::Runtime(format!("save: {e}")))?;

    ctx.tick(3, Some(3), "Done");
    ctx.done(serde_json::json!({"output": out_path.to_string_lossy()}),
             start.elapsed().as_millis() as u64);
    Ok(serde_json::json!({"status": "ok"}))
}

// 3. Wire up — the framework handles everything else
fn main() {
    let schema = ImgCompress::schema();
    let cmd = lilyco_cli::CliRenderer::new().render(&schema);
    let matches = cmd.get_matches();

    if lilyco_cli::CliRenderer::handle_builtin_flags(&schema, &matches) { return; }

    let output_format = lilyco_cli::CliRenderer::output_format(&matches);
    let args = lilyco_cli::CliRenderer::extract_args(&schema, &matches);
    let app = ImgCompress::from_args(&args).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = Context::new(tx, Arc::new(false.into()), output_format.clone());
    std::thread::spawn(move || compress(&app, &ctx));

    for event in rx {
        match output_format {
            OutputFormat::JsonStream => println!("{}", serde_json::to_string(&event).unwrap()),
            _ => if let Progress::Log { message, .. } = &event { eprintln!("  {message}"); },
        }
        if matches!(event, Progress::Done { .. } | Progress::Error { .. }) { break; }
    }
}
```

Run it:

```bash
$ cargo run -- --input photo.jpg --quality 50 --format webp
$ cargo run -- --schema              # JSON Schema
$ cargo run -- --anthropic-tool      # AI tool definition
$ cargo run -- --json-stream         # Machine-readable progress
```

---

## Architecture

```
┌──────────────────────────────────────────────────┐
│                  Your Struct                      │
│         #[derive(App)]                           │
│         struct MyTool { ... }                    │
└────────┬──────────┬──────────┬──────────────────┘
         │          │          │
    ┌────▼───┐ ┌───▼────┐ ┌──▼──────────┐
    │  CLI   │ │  TUI   │ │   Web UI    │
    │ (clap) │ │(ratatui│ │(axum + HTML)│
    └────┬───┘ └───┬────┘ └──┬──────────┘
         │         │         │
    ┌────▼─────────▼─────────▼────┐
    │      lilyco-core          │
    │  CommandSchema → clap::Cmd  │
    │  Progress → TUI widgets     │
    │  Progress → SSE events      │
    └─────────────────────────────┘
```

### Design Principles

1. **Type-driven**: `bool` → checkbox, `u8` → number input, custom enum → dropdown. No manual widget mapping.
2. **CLI-first**: CLI is the most structured interface. TUI and Web are derived from the same schema.
3. **Progress as first-class citizen**: Every interface understands `Progress::Tick` / `Log` / `Done`.
4. **Zero-cost**: Feature flags gate TUI and Web dependencies. CLI-only builds need only `clap`.
5. **AI-native**: Every Lilyco app can export its interface as an LLM function-calling schema.

---

## Crate Reference

### lilyco-core

The foundation. No UI dependencies.

```rust
use lilyco_core::prelude::*;
```

#### Core Traits

| Trait | Method | Purpose |
|-------|--------|---------|
| `App` | `schema() -> CommandSchema` | Returns the full command schema |
| `App` | `from_args(&HashMap) -> Result<Self, AppError>` | Construct from parsed CLI/AI args |
| `App` | `run(&self, &Context) -> Result<Value, AppError>` | Execute business logic |
| `Renderer` | `render(&self, &CommandSchema) -> Output` | Convert schema to a UI representation |
| `ValueEnum` | `variants() -> Vec<&str>` | All possible string values |
| `ValueEnum` | `from_str(&str) -> Option<Self>` | Parse from string |

#### Core Types

| Type | Purpose |
|------|---------|
| `CommandSchema` | Full command description: name, about, args, subcommands |
| `ArgSchema` | Single argument: name, about, kind, required, default |
| `ArgKind` | `Flag \| Text \| Number {min,max} \| Enum {values} \| Path {must_exist} \| List {item}` |
| `Progress` | `Started \| Tick \| Log \| Done \| Error` |
| `LogLevel` | `Debug \| Info \| Warn \| Error` |
| `Context` | Runtime: progress channel, cancel signal, output format |
| `OutputFormat` | `Human \| Json \| JsonStream` |
| `AppError` | `InvalidArg \| InvalidInput \| Runtime \| Cancelled \| Io \| Serialize` |

#### CommandSchema JSON Export

```rust
schema.to_json_schema()       // JSON Schema (generic)
schema.to_openai_tool()       // OpenAI function calling format
schema.to_anthropic_tool()    // Anthropic tool use format
```

### lilyco-macros

Proc macros for deriving boilerplate.

```rust
use lilyco_macros::{App, ValueEnum};
```

#### `#[derive(App)]`

Generates `schema()`, `from_args()`, and `run()`. Reads these attributes:

**Struct-level:**
| Attribute | Example | Purpose |
|-----------|---------|---------|
| `#[app(about = "...")]` | `#[app(about = "Compress images")]` | Command description |
| `#[app(run = "fn")]` | `#[app(run = "compress")]` | Wire up `run()` to a business-logic function |

**Field-level:**
| Attribute | Example | Purpose |
|-----------|---------|---------|
| `#[arg(about = "...")]` | `#[arg(about = "Input file")]` | Argument description |
| `#[arg(default = expr)]` | `#[arg(default = 75)]` | Default value |
| `#[arg(range = lo..=hi)]` | `#[arg(range = 1..=100)]` | Number range |
| `#[arg(min = n)]` | `#[arg(min = 0)]` | Min value |
| `#[arg(max = n)]` | `#[arg(max = 255)]` | Max value |
| `#[arg(must_exist = bool)]` | `#[arg(must_exist = true)]` | Path existence check |

#### `#[derive(ValueEnum)]`

Auto-converts PascalCase variants to snake_case strings:

```rust
#[derive(ValueEnum)]
enum Codec { H264, H265, Av1 }
// → variants: ["h264", "h265", "av1"]
// → from_str("h265") → Some(Codec::H265)
```

#### Type Inference

| Rust Type | Inferred `ArgKind` | `required` |
|-----------|-------------------|------------|
| `bool` | `Flag` | `false` |
| `String` | `Text` | `true` |
| `u8`/`i32`/`f64`/... | `Number` | `true` |
| `PathBuf` | `Path` | `true` |
| `Option<T>` | same as `T` | `false` |
| `Vec<T>` | `List { item: infer(T) }` | `true` |
| Custom `enum` | `Enum` | `true` |

### lilyco-cli

Generates a `clap::Command` from `CommandSchema`. Adds built-in flags automatically.

```rust
let schema = MyTool::schema();
let renderer = lilyco_cli::CliRenderer::new();
let cmd = renderer.render(&schema);
let matches = cmd.get_matches();
```

#### Built-in Flags (auto-added to every command)

| Flag | Behavior |
|------|----------|
| `--schema` | Print JSON Schema and exit |
| `--openai-tool` | Print OpenAI function definition and exit |
| `--anthropic-tool` | Print Anthropic tool definition and exit |
| `--json` | OutputFormat::Json |
| `--json-stream` | OutputFormat::JsonStream (one JSON per line) |

#### Public API

```rust
impl CliRenderer {
    fn new() -> Self;
    fn render(&self, schema: &CommandSchema) -> clap::Command;
    fn handle_builtin_flags(schema: &CommandSchema, matches: &ArgMatches) -> bool;
    fn output_format(matches: &ArgMatches) -> OutputFormat;
    fn extract_args(schema: &CommandSchema, matches: &ArgMatches)
        -> HashMap<String, serde_json::Value>;
}
```

### lilyco-tui

Interactive terminal form built on ratatui.

```
 Transcode — Transcode video files
 $ transcode --input video.mp4 --codec h265 --quality 18
────────────────────────────────────────────────────────
          (*) input: [video.mp4________________________]
              codec: [h264] h265 [Av1]                  ←→
           quality: [18]                                ↑↓
           dry_run: [x]                                 Space
────────────────────────────────────────────────────────
 [Tab] Switch  [Enter] Confirm  [Esc] Quit  [F1] Help
```

#### Widget Behaviors

| ArgKind | Key | Behavior |
|---------|-----|----------|
| Flag | `Space` | Toggle on/off |
| Text | Type + `Backspace` | Edit text |
| Number | `↑` `↓` | ±1. Type digits to edit |
| Enum | `←` `→` | Cycle through options |
| Path | Type + `Backspace` | Edit path |
| List | `Enter` / `Delete` | Add/remove item |

#### State Machine

```
Form ──Enter──▶ Confirm ──Enter──▶ Running ──done──▶ Done
  ▲               │                   │               │
  │               Esc                 │               │
  └───────────────┘                   ▼               ▼
                                   Error ◀────────── Enter
```

#### CLI Preview

The bottom bar shows a live CLI command preview that updates as you edit values. It auto-omits:

- `false` flags (e.g., `--dry-run` only appears when checked)
- Values matching their defaults
- Empty optional fields
- Path values are auto-quoted if they contain spaces

### lilyco-gui

Web server with embedded HTML, similar to Gradio in spirit.

```rust
let gui = lilyco_gui::GuiRenderer::new(8080);
gui.serve(schema, Arc::new(|args| Box::pin(async move {
    // process args, return result
    Ok(serde_json::json!({"status": "ok"}))
}))).await;
```

```
┌─────────────────────────────────────┐
│  ImgCompress — Compress images      │
│                                     │
│        Input: [___________________] │
│      Quality: [75_______________]   │
│       Format: [jpeg ▾]             │
│         Width: [0________________]  │
│      Dry run: [☐]                  │
│                                     │
│        [▶ Run]    [📋 Copy CLI]    │
│                                     │
│  $ imgcompress --quality 75         │
├─────────────────────────────────────┤
│  Output                             │
│  ████████░░░░░░░ 50%               │
│  Encoding frame 50/100              │
│  Done in 1.2s                       │
└─────────────────────────────────────┘
```

**Flow:** Form POST → spawn task → SSE stream → progress bar + log

---

## Type → Widget Mapping

| Rust Type | CLI | TUI | Web |
|-----------|-----|-----|-----|
| `bool` | `--flag` | `[x]` Space toggle | `<input type=checkbox>` |
| `String` | `--name <val>` | text input | `<input type=text>` |
| `u8`/`i32`/`f64`/... | `--count <num>` | ↑↓ ±1 + digit input | `<input type=number>` |
| Custom enum | `--mode <choice>` | ←→ cycle | `<select>` |
| `PathBuf` | `--file <path>` | text input | `<input type=text>` |
| `Vec<T>` | `--tag a --tag b` | Enter/Delete multi-line | dynamic inputs |
| `Option<T>` | optional | optional (not required) | optional |

---

## AI Integration

Every Lilyco app is an AI tool:

```bash
$ imgpress --anthropic-tool
```

```json
{
  "name": "ImgCompress",
  "description": "Compress image files",
  "input_schema": {
    "type": "object",
    "properties": {
      "input": { "type": "string", "description": "Input image file" },
      "quality": { "type": "number", "minimum": 1, "maximum": 100, "description": "Quality" },
      "format": { "type": "string", "enum": ["jpeg", "png", "webp"], "description": "Format" },
      "dry_run": { "type": "boolean", "description": "Dry run" }
    },
    "required": ["input"]
  }
}
```

This is a valid Anthropic tool-use definition. Drop it into your Claude API call, and the model can invoke your Rust tool directly.

```bash
$ imgpress --openai-tool   # OpenAI format
$ imgpress --schema         # Generic JSON Schema (for other LLMs)
$ imgpress --json-stream    # Each Progress event as one JSON line — ideal for agent consumption
```

### AI Agent Consumption Pattern

```jsonl
{"type":"started","total":5,"message":"Loading photo.jpg..."}
{"type":"tick","current":1,"total":5,"message":"Reading input file","percent":0.2}
{"type":"tick","current":2,"total":5,"message":"Original: 4000×3000","percent":0.4}
{"type":"tick","current":3,"total":5,"message":"Encoding...","percent":0.6}
{"type":"tick","current":4,"total":5,"message":"Writing compressed.jpg","percent":0.8}
{"type":"done","result":{"output_size":142000,"compression_ratio":35.5},"duration_ms":1200}
```

---

## Progress Protocol

Every interface consumes the same `Progress` events:

```rust
ctx.emit(Progress::Started { total: Some(100), message: Some("Starting...".into()) });

for i in 0..=100 {
    if ctx.is_cancelled() { return Err(AppError::Cancelled); }
    ctx.tick(i, Some(100), format!("Processing frame {i}"));
}

ctx.log(LogLevel::Info, "Compression complete");
ctx.done(serde_json::json!({"size_mb": 4.2}), 3200);
```

| Interface | `Started` | `Tick` | `Log` | `Done` |
|-----------|-----------|--------|-------|--------|
| **CLI** (`--json-stream`) | JSON line | JSON line with percent | JSON line | JSON line + exit |
| **CLI** (Human) | — | `\r` progress line | `[INFO]` line | summary + exit |
| **TUI** | Progress bar at 0% | Bar fills + message | Scroll log | Result screen |
| **Web** | SSE: bar at 0% | SSE: bar fills | SSE: log append | SSE: result JSON |

---

## Examples

### Image Compressor (`lilyco-example`)

```bash
cd lilyco-example
cargo run -- --input photo.jpg --quality 50 --format webp
cargo run -- --input photo.jpg --dry-run --json
cargo run -- --schema
```

See `lilyco-example/src/main.rs` for the full source (~230 lines).

### Transcode (TUI demo)

```rust
use lilyco_macros::{App, ValueEnum};
use lilyco_core::prelude::*;
use std::path::PathBuf;

#[derive(ValueEnum)]
enum Codec { H264, H265, Av1 }

#[derive(App)]
#[app(about = "Transcode video files")]
struct Transcode {
    #[arg(about = "Input file", must_exist = true)]
    input: PathBuf,

    #[arg(about = "Codec", default = "h264")]
    codec: Codec,

    #[arg(about = "Quality 0-51", default = 23, range = 0..=51)]
    quality: u8,
}
```

---

## Testing

```bash
# Run all tests
cargo test --workspace

# Run a specific crate
cargo test -p lilyco-core
cargo test -p lilyco-cli
cargo test -p lilyco-tui
cargo test -p lilyco-macros
```

Current coverage: **58 tests** across all crates.

---

## Installation

### From crates.io (when published)

```toml
[dependencies]
lilyco-core = "0.1"
lilyco-macros = "0.1"
lilyco-cli = "0.1"
```

### From git

```toml
[dependencies]
lilyco-core = { git = "https://github.com/lilyco-42/lilyco" }
lilyco-macros = { git = "https://github.com/lilyco-42/lilyco" }
lilyco-cli = { git = "https://github.com/lilyco-42/lilyco" }
lilyco-tui = { git = "https://github.com/lilyco-42/lilyco" }
```

---

## Limitations & Roadmap

### Current Limitations

- **`#[derive(App)]`** only works on named-field structs (no tuple structs or enums)
- **`#[app(run = "fn")]`** requires the function to be in scope. Without this attribute, `run()` panics with a helpful message directing you to add it.
- **Number range validation** works at the CLI layer (clap) but not in TUI/Web widgets
- **Subcommands** are supported in CLI only — TUI and Web renderers do not handle them yet
- **Windows TUI** not yet tested (crossterm backend should work but hasn't been verified)

### Roadmap

- [x] ~~Real `run()` dispatch in Web GUI with progress streaming~~ — done via `GuiRenderer::serve_app::<A>()`
- [x] ~~`#[app(run = "fn")]` macro attribute~~ — wire business logic with zero boilerplate
- [x] ~~Integration tests that exercise all three interfaces end-to-end~~ — 12 tests in `lilyco-example`
- [ ] Subcommand navigation in TUI and Web GUI
- [ ] Input validation in TUI/Web widgets (range, required, enum)
- [ ] Path auto-complete in TUI (Tab triggers directory listing)
- [ ] `#[app(subcommands)]` macro support
- [ ] Publish to crates.io
- [ ] Performance benchmarks for schema generation

---

## License

MIT OR Apache-2.0, at your option.
