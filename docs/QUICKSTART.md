## Quick Start

Create a new project and add the dependencies:

```bash
cargo new imgpress && cd imgpress
cargo add lilyco serde serde_json image      # 一个框架依赖即可（宏经 facade 解析路径）
```

Paste this into `src/main.rs`:

```rust
use std::path::PathBuf;
use std::time::Instant;
use image::{DynamicImage, GenericImageView};
use image::imageops::FilterType;
use lilyco::prelude::*;

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

// 3. Wire up — one line, four interfaces
fn main() {
    lilyco::run::<ImgCompress>();
}
```

Run it:

```bash
$ cargo run -- --input photo.jpg --quality 50 --format webp
$ cargo run -- --schema              # JSON Schema
$ cargo run -- --anthropic-tool      # AI tool definition
$ cargo run -- --json-stream         # Machine-readable progress
$ cargo run -- --gui                 # Web GUI (SSE progress)
$ cargo run -- --mcp                 # MCP stdio server (Agent-ready)
```

`lilyco::run::<A>()` 按环境自动选端（借鉴 mininterface 的接口工厂）：
交互终端 → TUI；管道/脚本 → CLI；`--gui` → Web；`--mcp` → MCP。
TUI 起不来时自动回退 CLI，绝不裸崩。

---

