# lffmpeg — 媒体转码使用文档

`lilyco-ffmpeg` 是 lilyco 框架的一个示例工具：用 `#[derive(App)]` 声明式描述
一个 ffmpeg 包装命令，天然获得 **CLI / TUI / Web / MCP** 四种界面 + **AI 可调用**，
全程报告实时进度、支持取消，跨平台（含 Android/Termux）。

- 仓库：`https://github.com/lilyco-42/lilyco`
- 二进制名：`lffmpeg`
- 依赖：系统 `ffmpeg`（必须在 PATH 上）；`ffprobe` 仅用于计算完成百分比，缺失时自动降级为不确定进度。

---

## 安装

### 1. cargo binstall（推荐，免编译）

```bash
cargo install cargo-binstall   # 首次
cargo binstall lilyco-ffmpeg
```

从 GitHub Release 拉单个预编译二进制，装到 `~/.cargo/bin/lffmpeg`（Windows 为 `lffmpeg.exe`）。

### 2. cargo install（源码编译）

```bash
cargo install --git https://github.com/lilyco-42/lilyco lilyco-ffmpeg
```

### 3. 从仓库直接跑（开发）

```bash
git clone https://github.com/lilyco-42/lilyco
cd lilyco
cargo run -p lilyco-ffmpeg -- --input a.mp4 --output b.mp4
```

CI 为 Windows 与 Android（aarch64）各产出一个 headless 二进制，随 tag 发布。

---

## 快速上手

最简单的转码（拷贝音频、重新编码视频为 h265、CRF 28）：

```bash
lffmpeg --input a.mp4 --output b.mp4 --codec h265 --crf 28
```

转码自带实时进度（百分比、当前 out_time、帧数）。跑完打印结果 JSON：
输入、输出、完整 ffmpeg 命令、退出码、耗时、成功与否、stderr 摘要。

---

## 全部参数

| 参数 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `--input` | path（须存在） | — | 输入媒体文件 |
| `--output` | path | — | 输出媒体文件 |
| `--codec` | enum | `h264` | `h264` / `h265` / `vp9` / `av1` / `copy`（见下） |
| `--crf` | number 0-51 | `23` | CRF 质量（越低越好；`copy` 时忽略） |
| `--preset` | enum | `medium` | `ultrafast`…`veryslow`（vp9/av1 映射为 quality 档） |
| `--width` | number | 缺省按高度等比 | 输出宽度（`-2` 保持宽高比） |
| `--height` | number | 缺省按宽度等比 | 输出高度（`-2` 保持宽高比） |
| `--start` | number（秒） | — | 裁剪：从第 N 秒开始（`-ss`） |
| `--duration` | number（秒） | — | 裁剪：只取 N 秒（`-t`） |
| `--audio` | enum | `copy` | `copy` / `aac` / `opus` / `none`（`-an` 去掉音轨） |
| `--overwrite` | bool | `false` | `-y` 强制覆盖；否则 `-n` 拒绝覆盖 |

每个工具还自动带框架内置旗标：`--json` / `--json-stream` / `--schema` / `--mcp` / `--tui` / `--web` / `--help`。

### 参数映射到 ffmpeg

- `codec` → `-c:v`（`copy` → 流复制，`-c:v copy`）
- `crf` → `-crf`（0-51）
- `preset` → `-preset`（vp9/av1 时 ffmpeg 自行解释 quality 档）
- `width`/`height` → `-vf scale=W:H`，缺省的一侧用 `-2` 保持宽高比
- `start`/`duration` → `-ss` / `-t`（秒）
- `audio` → `-c:a <..>`（`none` → `-an`）
- `overwrite` → `-y` / `-n`
- 固定追加 `-movflags +faststart`、`-progress pipe:1`（供进度解析）

---

## 示例

```bash
# 转 h265 / 低码率（质量高、文件小）
lffmpeg --input a.mp4 --output b.mp4 --codec h265 --crf 26

# 缩放：只给宽度 1280，高度自动等比
lffmpeg --input a.mp4 --output b.mp4 --width 1280

# 只给高度 720，宽度自动等比
lffmpeg --input a.mp4 --output b.mp4 --height 720

# 裁剪：从第 10 秒开始，取 5 秒
lffmpeg --input a.mp4 --output clip.mp4 --start 10 --duration 5

# VP9 + Opus（w/ 音轨转码）
lffmpeg --input a.mp4 --output b.webm --codec vp9 --audio opus

# 去掉音轨、强制覆盖
lffmpeg --input a.mp4 --output b.mp4 --audio none --overwrite

# 流复制（不重新编码，仅改容器/复刻）
lffmpeg --input a.mov --output a.mp4 --codec copy
```

---

## 四种界面

```bash
lffmpeg --input a.mp4 --output b.mp4     # CLI：实时进度条 + 结果 JSON
lffmpeg --input a.mp4 --output b.mp4 --tui    # TUI：交互式表单 + 运行视图（Ctrl-C 取消）
lffmpeg --input a.mp4 --output b.mp4 --web    # 浏览器 GUI
lffmpeg --mcp                               # MCP stdio 服务器（供 AI / DSH）
```

### AI / 脚本消费（`--json-stream`）

每个事件一行 JSONL：`started` → `tick*` → `done` 或 `error`。

```bash
lffmpeg --input a.mp4 --output b.mp4 --codec h265 --json-stream
# {"type":"started",  "total":100,  "message":"ffmpeg a.mp4 -> b.mp4"}
# {"type":"tick",     "current":97, "total":100, "percent":0.97, "message":"2.92s frame 72 b.mp4"}
# {"type":"done",     "result":{...}, "duration_ms":355}
```

`done.result` 字段：`input` / `output` / `command` / `exit_code` / `duration_ms` / `duration_hms` / `success` / `stderr` / `stderr_truncated`。
ffmpeg 非零退出不是工具崩溃，而是 `error` 事件携带 stderr 末行摘要，便于排查。

---

## 行为约定

- **取消**：CLI 按 `Ctrl-C`，TUI 按 `Ctrl-C`/`c`/`q`/`Esc`。取消会 **kill 正在运行的 ffmpeg** 并终止。
- **错误**：输入不存在在解析期即拒绝（`must_exist`）；ffmpeg 失败返回携带 stderr 摘要的 `AppError::Runtime`。
- **Android/Termux**：headless 构建（CLI + MCP），关掉 TUI/Web 后端；需要 Termux 里装 `ffmpeg` / `ffprobe`（`pkg install ffmpeg`），无 ffprobe 时进度降级为不确定进度。

---

## 开发 / 测试

```bash
cargo test -p lilyco-ffmpeg          # 14 个单元测试（参数映射 / 进度解析 / 剪辑校验）
cargo clippy -p lilyco-ffmpeg --all-targets
cargo fmt -p lilyco-ffmpeg -- --check
```

参数如何映射到 ffmpeg、`-progress pipe:1` 如何解析、音频/剪辑/覆盖如何处理，均由测试覆盖。
