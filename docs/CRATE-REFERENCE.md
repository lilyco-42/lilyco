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
| `ArgKind` | `Flag | Text | Number {min,max} | Enum {values} | Path {must_exist} | List {item}` |
| `Progress` | `Started | Tick | Log | Done | Error` |
| `LogLevel` | `Debug | Info | Warn | Error` |
| `Context` | Runtime: progress channel, cancel signal, output format |
| `OutputFormat` | `Human | Json | JsonStream` |
| `AppError` | `InvalidArg | InvalidInput | Runtime | Cancelled | Io | Serialize` |
| `Registry` | 运行期命令注册表：注册 / 别名 / 隐藏 / JSON 声明式加载 |
| `RegisteredCommand` | `name + aliases + hidden + schema + handler` |
| `Handler` | `Fn(&Context, &Value) -> Result<Value, AppError>`（统一执行入口） |
| `executor` | 共享执行宿主：`spawn`（流式）/ `execute`（同步收集），保证事件流以 Done/Error 结尾 |

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
// -> variants: ["h264", "h265", "av1"]
// -> from_str("h265") -> Some(Codec::H265)
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

#### 多命令（Registry → clap 子命令）

一个二进制挂多个命令，子命令名 / 别名 / 隐藏语义全部来自 `Registry`：

```rust
let mut registry = Registry::new();
registry.register(RegisteredCommand::from_app::<Compress>())?;
registry.register(RegisteredCommand::from_app::<Resize>())?;

lilyco_cli::run_registry("imgtool", registry);   // crate 入口
lilyco::run_cli_registry("imgtool", registry);   // 门面入口
```

```bash
imgtool compress --input a.png    # 子命令
imgtool resize --width 800        # 子命令
imgtool --schema                  # 注册表清单（全部命令 schema，Agent 可消费）
```

> `#[derive(App)]` 默认用结构体名做命令名；多命令场景建议
> `#[app(name = "img-compress")]` 指定 kebab-case 名。

### lilyco-tui

Interactive terminal form built on ratatui.

```
 Transcode -- Transcode video files
 $ transcode --input video.mp4 --codec h265 --quality 18
---------------------------------------------------------
          (*) input: [video.mp4________________________]
              codec: [h264] h265 [Av1]                  <->
           quality: [18]                                ^v
           dry_run: [x]                                 Space
---------------------------------------------------------
 [Tab] Switch  [Enter] Confirm  [Esc] Quit  [F1] Help
```

#### Widget Behaviors

| ArgKind | Key | Behavior |
|---------|-----|----------|
| Flag | `Space` | Toggle on/off |
| Text | Type + `Backspace` | Edit text |
| Number | `^` `v` | +/-1（自动夹在 schema 范围内）. Type digits to edit（越界由提交校验拦截） |
| Enum | `<` `>` | Cycle through options |
| Path | Type + `Tab` | **目录补全**：循环候选（目录带 `/` 后缀）；空值时 `Tab` 切换字段 |
| List | `Enter` / `Delete` | Add/remove item |

#### State Machine

```
[多命令] CommandSelect --Enter--> Form --Enter--> Confirm --Enter--> Running --done--> Done
                                          ^                   |               |
                                          |                   |               |
                                          +------<--- Done/Error 任意键返回（单命令退出）
  ^               |                   |               |
  |               Esc                 |               |
  +---------------+                   v               v
                                   Error <---------- Enter
```

#### CLI Preview

The bottom bar shows a live CLI command preview that updates as you edit values. It auto-omits:

- `false` flags (e.g., `--dry-run` only appears when checked)
- Values matching their defaults
- Empty optional fields
- Path values are auto-quoted if they contain spaces

### lilyco-gui

Web server with embedded HTML, similar to Gradio in spirit. Zero external UI dependencies: one
self-contained page (design tokens, component catalogue and the seven interaction states live in
`lilyco-gui/DESIGN.md`), light + dark, usable from 320 px to 1920 px.

```rust
// 单命令：把 App 类型交给它，进度与取消都接好线（内部就是一份单命令注册表）
let gui = lilyco_gui::GuiRenderer::new(8080);
gui.serve_app::<ImgCompress>(ImgCompress::schema()).await;
```

多命令用 `serve_registry(registry)`（页面顶部出现命令下拉，`?cmd=` 切换）。自己实现执行体的逃生口是
`serve(schema, runner)`：`RunnerFn` 拿到参数和一个 SSE 发送端，代价是 GUI 手里没有取消句柄，
页面因此不画「取消」按钮 —— 要让 Web 端能取消就走前两个入口。

```
+-------------------------------------+
|  ◆ lilyco Web 控制台     [imgcompress v] ◐ |
|  ImgCompress                        |
|  Compress images                    |
|  Input *  [_____________________]   |
|           [ ⇪ 拖拽区 · 选择文件 · 本机 ] |
|  Quality  [75]  取值 1 – 100        |
|  Format   [jpeg v]                  |
|  Dry run  [ ]  只算不写             |
|  [▶ 运行] [■ 取消] [复制 CLI]        |
|  $ imgcompress --quality 75 ...     |
|  输出  ▓▓▓▓▓▓░░░░ 60%                |
|  > Encoding frame 60/100            |
|  结果  { "saved_bytes": 10240 }  [复制] |
+-------------------------------------+
```

**Flow:** `POST /run` (JSON args + token header) → spawn on `core::executor` → `GET /progress/{sid}`
SSE (`started` / `tick` / `log` / `telemetry` / `done` / `error`) → progress bar + log + result.

**路径字段带「本机」选择按钮**：`#[arg(must_exist = true)]` 渲染出的输入框旁边有一个按钮，点了由**本机进程**弹一次系统文件选择框（`POST /pick`，走 `rfd`，可用 `pick` 特性关掉），选完把真实路径回填进输入框。
浏览器的 `<input type=file>` 办不到这件事——它出于安全永远不给出真实路径（只有 `File.name`），而四端共用的 handler 收的正是 `path`、自己从盘上读。
`/pick` 与 `/run` 共用同一道回环 + Origin + Token 闸（无令牌 401，挡在弹框之前）；对话框在 blocking 线程里等，弹着的时候页面照常响应。
> 弹框可见性：Windows 的前台锁定不让后台进程抢焦点，所以 `pick_handler` 不硬抢——它按标题轮询
> `FindWindowW`（标题就是 `set_title` 用的那个常量，两者不会各说各话），`SetForegroundWindow` 试一次、
> `FlashWindowEx` 让任务栏那一格闪起来；页面上同时挂一条「系统选择框已弹出，可能被压在后面，看任务栏」的提示，
> 请求一结束就撤。实测：弹着的时候 `tasklist /V` 的窗口标题正是 `选择文件`，且页面其他请求仍然 200。

### lilyco-tauri (桌面端)

Tauri 2 桌面壳，装的是**同一个读者**：应用不自己解析办公文件，它把随包带出来的 `lbin` 当 sidecar 跑，
拿它的 JSON 原样铺开。理由很直白——`lbin` 那批账在 CI 上与一份标准库实现逐格对过（语料 302 份件，
每次 run 都整本重跑一遍），桌面端再造一个读者等于多一处会各自漂移的说法，两边还都自称「按文件写的交」。

命令层只有两个：`office_commands` 交「能问什么」的协议（十个问题，名字在 Rust 侧白名单，
测的就是它们与 `lbin` 源码里的字符串一字不差），`office_run` 收 `命令 + 路径`、验过是文件之后
跑 `lbin <命令> --path <文件> --json`。退出码与 stderr 原样进错误信息，不折成「读取失败」四个字。
权限面收成三样：`core:default`、`dialog:allow-open`、`shell:allow-execute` 且只允许那一个 sidecar。
页面 CSP 收到 `'self'`，不引任何外部脚本，也不用 npm 构建（走 `withGlobalTauri` 的全局 API）。

产物由 GitHub Actions 出（`.github/workflows/desktop.yml`，本机不编）：三个平台各自
`cargo build --release -p lilyco-binfmt --bin lbin` → 按 target triple 放进
`lilyco-tauri/binaries/` → `tauri-action` 出 bundle → 收进 `dist/` 再上传可直接下载的工件
（工件名 `lilyco-office-<windows|macos|linux>`，里面是 `.msi` / `.dmg` / `.deb` / `.AppImage`），
打了 `v*` 标签才顺手挂到 GitHub Release。上传那步留着 `if-no-files-found: error`：真没产物时要它明着红。

> **`tauri-action` 的两处反直觉**（第一版三条流水线各红 100 毫秒，就是从这两条读出来的）：
> `includeRelease` 为假时它**什么都不做**——不编、不打 bundle，只打一句
> 「No artifacts were found」然后 success 收工，所以「每次 run 都要有工件」就必须一直传
> `includeRelease: true`（不配 `tagName` 它就不碰 release，见源码里
> 「No releaseId or tagName provided, skipping all uploads...」那条分支）。
> 而前端零 npm 依赖（本仓库故意没有 `package.json`）时，它找不到 `@tauri-apps/cli` 会退回
> `npm i -g @tauri-apps/cli@v1`——v1 的 CLI 读不了 v2 配置，所以 `tauriScript` 要显式写成
> `npx --yes @tauri-apps/cli@2`。bundle 落在 `target/release/bundle` 还是
> `target/<triple>/release/bundle` 不由我们猜，收工件那一步用 `find` 量出来再收。

> **签名的实话**：没有配 Apple 证书与 notary 凭据时，macOS 那份就是**未签名未公证**——CI 照样绿，
> 但别人下载后会被 Gatekeeper 拦住；Windows 的 `.msi` 同理未签名。工件能装不等工件好装，
> 这一段不替凭据的存在说谎。

**第一次三平台同绿的实物**（run 36454410386，`5831609`；每次推分支都会在 Actions 那次 run 的
Artifacts 里留一份同名的，点开就能下载）：

| 工件（zip 里） | 大小 | 备注 |
|---|---|---|
| `lilyco office_0.3.0_x64_en-US.msi` | 7,426,048 B | 用只读方式翻了 MSI 的 File 表：`lbin.exe` 在里面（sidecar 真的随包） |
| `lilyco office_0.3.0_x64-setup.exe` | 5,076,792 B | NSIS，正文压过所以字符串搜不到 lbin，不代表没带 |
| `lilyco office_0.3.0_aarch64.dmg` | 7,233,904 B | Apple Silicon；未签名未公证 |
| `lilyco office_0.3.0_amd64.deb` | 8,052,986 B | `data.tar` 是 zst 压缩，同上 |
| `lilyco office-0.3.0-1.x86_64.rpm` | 8,053,569 B | |
| `lilyco office_0.3.0_amd64.AppImage` | 85,850,616 B | 自带头与库路径，最大的一份 |

### lilyco (facade)

**一个依赖搞定四端**。用户代码只依赖这一个 crate，后端按环境自动选择。

```rust
use lilyco::prelude::*;

fn main() {
    lilyco::run::<ImgCompress>();          // 自动选端
    // lilyco::run_with::<ImgCompress>(Backend::Mcp);  // 显式指定
}
```

| 触发方式 | 后端 |
|---------|------|
| `--mcp` | MCP stdio 服务器（Agent 直接调用） |
| `--gui` / `--web` | Web GUI |
| `LILYCO_UI=cli\|tui\|web\|mcp` | 环境变量强制 |
| 交互终端 + `TERM` | TUI 表单（起不来自动回退 CLI） |
| 其余（管道 / CI / 脚本） | CLI（`--json-stream` 供 AI 消费） |

多命令场景（Registry → 四端导航）：

```rust
lilyco::serve_mcp(registry);         // MCP：整个注册表 = tools/list
lilyco::run_cli_registry("app", registry);   // CLI：注册表 → clap 子命令
lilyco::run_tui_registry("app", registry);   // TUI：命令选择页 → 表单（回退 CLI）
```

| 触发方式（单命令） | 后端 |
|---------|------|
| `--mcp` | MCP stdio 服务器（Agent 直接调用） |
| `--gui` / `--web` | Web GUI |
| `LILYCO_UI=cli\|tui\|web\|mcp` | 环境变量强制 |
| 交互终端 + `TERM` | TUI 表单（起不来自动回退 CLI） |
| 其余（管道 / CI / 脚本） | CLI（`--json-stream` 供 AI 消费） |

### lilyco-mcp

把命令注册表暴露为标准 **Model Context Protocol** 服务器（2024-11-05），
实现 `initialize` / `ping` / `tools/list` / `tools/call`，零额外依赖。
`tools/call` 携带 `_meta.progressToken` 时，执行期间流式返回
`notifications/progress`（Progress::Started/Tick → 通知），长任务对 Agent 不再是黑盒。

```rust
let mut registry = Registry::new();
registry.register(RegisteredCommand::from_app::<MyTool>())?;
lilyco_mcp::McpServer::new(registry).serve_stdio()?;
```

核心是纯函数 `handle_line`（一行请求 → 一行响应），`serve` 可挂任意 `Read + Write`，
协议逻辑全部可单元测试。

### lilyco-ultra-ui

Experimental **JSON-to-React** declarative UI generator. Write a Chinese-language JSON spec; get a full React frontend — no Rust code required.

```rust
use lilyco_ultra_ui::UltraUiServer;

#[tokio::main]
async fn main() {
    UltraUiServer::new(9090).serve().await;
}
```

The JSON spec uses Chinese field names for an Excel-like feel:

```json
{
  "窗口": {
    "标题": "My App",
    "大小": "中等",
    "元素": [
      { "类型": "标题", "内容": "Welcome" },
      { "类型": "文本输入", "标签": "Name", "占位符": "Enter name..." },
      { "类型": "数字", "标签": "Quantity", "最小值": 0, "最大值": 100 },
      { "类型": "按钮", "文本": "Submit", "样式": "primary" },
      { "类型": "进度", "标签": "Progress" }
    ]
  }
}
```

#### Supported Element Types

| Type (Chinese) | English | Description |
|----------------|---------|-------------|
| `文本` | Text | Static text block |
| `标题` | Heading | H1-H4 heading |
| `按钮` | Button | Clickable button with style variants |
| `文本输入` | Text Input | Single-line text input |
| `数字` | Number | Numeric input with min/max |
| `下拉` | Select | Dropdown select |
| `复选框` | Checkbox | Boolean toggle |
| `多行文本` | Textarea | Multi-line text input |
| `图片` | Image | Image display |
| `分割线` | Divider | Visual separator |
| `进度` | Progress | Progress bar |
| `链接` | Link | Hyperlink |
| `计算器` | Calculator | Built-in calculator widget |

---

