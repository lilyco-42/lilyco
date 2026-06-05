# Triforge 工程规格文档 v0.1

> 给下一个 AI 的上下文：这是一个 Rust 框架的原型规格。
> 任务是实现 `triforge-core` crate 的骨架代码，包含下文定义的所有 trait 和类型。
> 不要改变接口设计，可以补充实现细节。

---

## 一句话定位

**一个让 Rust 软件天生 AI-callable 的框架。**
开发者写一次业务逻辑，框架自动生成：CLI（AI/脚本调用）、TUI（终端交互）、GUI（图形界面）三端接口，以及 AI Function Calling 所需的 JSON Schema。

---

## 设计原则

1. **CLI 优先**：CLI 是 AI 和脚本的接口，语义最严格，是其他两端的地基
2. **类型驱动**：从 Rust 类型推断 widget 形态，不让用户重复声明
3. **进度是一等公民**：长任务的进度上报有统一协议，三端自动处理
4. **零成本抽象**：不用的端不编译进去，feature flag 控制
5. **Schema 可导出**：任何用框架写的软件都能导出机器可读的接口描述

---

## Crate 结构

```
triforge/
├── triforge-core/      # 核心 trait、类型、derive 宏接口 ← 本次实现重点
├── triforge-cli/       # CLI 渲染器，依赖 clap
├── triforge-tui/       # TUI 渲染器，依赖 ratatui
├── triforge-gui/       # GUI 渲染器，依赖 egui（后续）
└── triforge-macros/    # #[derive(App)] 过程宏（后续）
```

本次只实现 `triforge-core`，其他 crate 留 stub。

---

## 核心数据类型

### 1. `ArgSchema` — 单个参数的完整描述

```rust
/// 一个参数的完整机器可读描述
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgSchema {
    pub name: &'static str,
    pub about: &'static str,
    pub kind: ArgKind,
    pub required: bool,
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ArgKind {
    /// bool → --flag
    Flag,
    /// String → --name <value>
    Text,
    /// 数字，带可选范围
    Number { min: Option<f64>, max: Option<f64> },
    /// 枚举，固定可选值
    Enum { values: Vec<&'static str> },
    /// 文件路径
    Path { must_exist: bool },
    /// Vec<T> → --tag a --tag b
    List { item: Box<ArgKind> },
}
```

### 2. `CommandSchema` — 命令树节点

```rust
/// 一个命令（或子命令）的完整描述
/// 这是整个框架的核心数据结构，CLI/TUI/GUI 都从它派生
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSchema {
    pub name: &'static str,
    pub about: &'static str,
    pub args: Vec<ArgSchema>,
    pub subcommands: Vec<CommandSchema>,
}

impl CommandSchema {
    /// 导出为 JSON Schema（用于 AI Function Calling）
    pub fn to_json_schema(&self) -> serde_json::Value { todo!() }

    /// 导出为 OpenAI tool 定义格式
    pub fn to_openai_tool(&self) -> serde_json::Value { todo!() }

    /// 导出为 Anthropic tool 定义格式
    pub fn to_anthropic_tool(&self) -> serde_json::Value { todo!() }
}
```

### 3. `Progress` — 统一进度协议

```rust
/// 长任务的进度事件，三端都消费这个类型
/// CLI → JSON stream 到 stdout
/// TUI → 进度条组件更新
/// GUI → 进度环 + 取消按钮
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Progress {
    Started {
        /// 总步数，None 表示不确定
        total: Option<u64>,
        message: Option<String>,
    },
    Tick {
        current: u64,
        total: Option<u64>,
        /// 人类可读的当前状态，如 "编码第 120 帧"
        message: Option<String>,
        /// 0.0 ~ 1.0，None 表示不确定
        percent: Option<f32>,
    },
    Log {
        level: LogLevel,
        message: String,
    },
    Done {
        /// 任务结果，序列化为 JSON
        result: serde_json::Value,
        duration_ms: u64,
    },
    Error {
        code: i32,
        message: String,
        /// 机器可读的错误类型
        kind: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel { Debug, Info, Warn, Error }
```

### 4. `Context` — 运行时上下文

```rust
/// 传入 App::run() 的上下文
/// 负责进度上报、取消信号、输出格式控制
pub struct Context {
    /// 发送进度事件的 channel sender
    progress_tx: std::sync::mpsc::Sender<Progress>,
    /// 取消信号
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// 输出格式：human / json / json-stream
    pub output_format: OutputFormat,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    /// 人类友好，彩色，有格式
    Human,
    /// 单个 JSON 对象，任务完成后输出
    Json,
    /// 每个 Progress 事件一行 JSON，适合 AI/脚本消费
    JsonStream,
}

impl Context {
    /// 上报进度，三端各自处理显示
    pub fn emit(&self, progress: Progress) {
        let _ = self.progress_tx.send(progress);
    }

    /// 检查是否已被取消
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 便捷方法
    pub fn tick(&self, current: u64, total: Option<u64>, message: impl Into<String>) {
        self.emit(Progress::Tick {
            current,
            total,
            message: Some(message.into()),
            percent: total.map(|t| current as f32 / t as f32),
        });
    }

    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        self.emit(Progress::Log { level, message: message.into() });
    }

    pub fn done(&self, result: serde_json::Value, duration_ms: u64) {
        self.emit(Progress::Done { result, duration_ms });
    }
}
```

---

## 核心 Trait

### `App` trait — 用户实现这一个就够了

```rust
/// 用户需要实现的唯一 trait
/// #[derive(App)] 宏会自动实现，手动实现也支持
pub trait App: Sized {
    /// 返回这个命令的完整 schema（用于 CLI 生成、AI schema 导出）
    fn schema() -> CommandSchema;

    /// 从解析后的参数 map 构造自身（CLI 调用路径）
    fn from_args(args: &std::collections::HashMap<String, serde_json::Value>)
        -> Result<Self, AppError>;

    /// 从 AI tool call JSON 构造自身
    fn from_tool_call(call: &serde_json::Value) -> Result<Self, AppError> {
        // 默认实现：tool call 的 input 格式和 args 格式相同
        let args = call["input"]
            .as_object()
            .ok_or(AppError::InvalidInput("missing input".into()))?
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Self::from_args(&args)
    }

    /// 执行业务逻辑，通过 ctx 上报进度
    fn run(&self, ctx: &Context) -> Result<serde_json::Value, AppError>;

    // ---- 框架提供默认实现，用户一般不需要覆盖 ----

    /// CLI 入口：解析 std::env::args()，执行，处理输出
    fn run_cli() -> ! { todo!() }

    /// TUI 入口：启动 ratatui 界面
    #[cfg(feature = "tui")]
    fn run_tui() -> Result<(), AppError> { todo!() }

    /// GUI 入口：启动 egui 窗口
    #[cfg(feature = "gui")]
    fn run_gui() -> Result<(), AppError> { todo!() }
}
```

### `Renderer` trait — 各端实现

```rust
/// 渲染器：把 CommandSchema 转换成各端的表示
pub trait Renderer {
    type Output;
    fn render(&self, schema: &CommandSchema) -> Self::Output;
}
```

---

## 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("参数错误: {0}")]
    InvalidArg(String),

    #[error("输入无效: {0}")]
    InvalidInput(String),

    #[error("执行失败: {0}")]
    Runtime(String),

    #[error("已取消")]
    Cancelled,

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("序列化错误: {0}")]
    Serialize(#[from] serde_json::Error),
}
```

---

## 用法示例（目标 API）

下面是框架成熟后，用户代码的样子。实现时以此为北极星。

```rust
use triforge::prelude::*;

/// 视频转码命令
#[derive(App)]
#[app(about = "转码视频文件")]
struct Transcode {
    #[arg(about = "输入文件", must_exist = true)]
    input: PathBuf,

    #[arg(about = "输出文件")]
    output: PathBuf,

    #[arg(about = "视频编码", default = "h264")]
    codec: Codec,

    #[arg(about = "质量 0-51，越小越好", default = 23, range = 0..=51)]
    quality: u8,

    #[arg(about = "仅预览，不实际执行")]
    dry_run: bool,
}

#[derive(ValueEnum)]
enum Codec { H264, H265, Av1 }

impl Run for Transcode {
    fn run(&self, ctx: &Context) -> Result<serde_json::Value, AppError> {
        if self.dry_run {
            ctx.log(LogLevel::Info, format!(
                "dry-run: ffmpeg -i {} --codec {:?} -crf {}",
                self.input.display(), self.codec, self.quality
            ));
            return Ok(serde_json::json!({ "dry_run": true }));
        }

        ctx.emit(Progress::Started {
            total: Some(1000), // 总帧数
            message: Some("开始转码".into()),
        });

        // 实际调用 ffmpeg...
        for frame in 0..1000u64 {
            if ctx.is_cancelled() { return Err(AppError::Cancelled); }
            ctx.tick(frame, Some(1000), format!("编码第 {} 帧", frame));
        }

        ctx.done(
            serde_json::json!({ "output": self.output, "size_mb": 142 }),
            43200,
        );
        Ok(serde_json::json!({ "status": "done" }))
    }
}

fn main() {
    // 根据编译 feature 和运行参数自动选择端
    Transcode::run_auto()
}
```

**三端调用方式：**

```bash
# CLI（AI/脚本）
$ mytool -i a.mp4 -o b.mp4 --codec h265 --json-stream

# CLI Schema 导出（AI function calling 用）
$ mytool --schema
$ mytool --openai-tool
$ mytool --anthropic-tool

# TUI
$ mytool --tui

# GUI
$ mytool --gui
```

---

## 本次实现任务

**目标**：实现 `triforge-core` crate，能通过下面的测试。

**文件结构**：

```
triforge-core/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── schema.rs     # CommandSchema, ArgSchema, ArgKind
    ├── progress.rs   # Progress, LogLevel
    ├── context.rs    # Context, OutputFormat
    ├── error.rs      # AppError
    └── app.rs        # App trait, Renderer trait
```

**Cargo.toml 依赖**：

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"

[features]
default = ["cli"]
cli = []
tui = []
gui = []
```

**验收测试**：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_serializes_to_json_stream() {
        let p = Progress::Tick {
            current: 120,
            total: Some(1000),
            message: Some("编码第 120 帧".into()),
            percent: Some(0.12),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"type\":\"tick\""));
        assert!(json.contains("\"current\":120"));
    }

    #[test]
    fn command_schema_exports_anthropic_tool() {
        let schema = CommandSchema {
            name: "transcode",
            about: "转码视频",
            args: vec![
                ArgSchema {
                    name: "input",
                    about: "输入文件",
                    kind: ArgKind::Path { must_exist: true },
                    required: true,
                    default: None,
                },
                ArgSchema {
                    name: "codec",
                    about: "编码格式",
                    kind: ArgKind::Enum {
                        values: vec!["h264", "h265", "av1"]
                    },
                    required: false,
                    default: Some(serde_json::json!("h264")),
                },
            ],
            subcommands: vec![],
        };

        let tool = schema.to_anthropic_tool();
        assert_eq!(tool["name"], "transcode");
        assert!(tool["input_schema"]["properties"]["input"].is_object());
    }

    #[test]
    fn context_emits_progress() {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = Context::new_test(tx);
        ctx.tick(1, Some(10), "step 1");
        let event = rx.recv().unwrap();
        matches!(event, Progress::Tick { current: 1, .. });
    }
}
```

---

## 关键约束

- 所有公开类型必须实现 `Debug + Clone + Serialize + Deserialize`
- `Context` 必须是 `Send`（支持多线程任务）
- 不要在 core crate 里引入 clap、ratatui、egui 的依赖
- `to_anthropic_tool()` 输出格式参考：
  ```json
  {
    "name": "transcode",
    "description": "转码视频",
    "input_schema": {
      "type": "object",
      "properties": {
        "input": { "type": "string", "description": "输入文件" },
        "codec": { "type": "string", "enum": ["h264","h265","av1"] }
      },
      "required": ["input"]
    }
  }
  ```

---

*文档版本：v0.1 | 框架代号：Triforge | 核心理念：Write once, callable everywhere*
