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
- **`imgpress --mcp`** — a standard MCP server any Agent can call (2024-11-05)

Same binary, four interfaces — the backend is chosen automatically by the environment.

---

