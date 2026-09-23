# 集成指南 —— 把 lilyco 接进你的项目

面向**仓库之外**的人：你已经有一个 Rust 项目（或只有一个「想让 AI agent 调我的工具」的需求），
想知道从哪儿进来、要付多大代价。仓库内部的导航入口是 [CODEGRAPH.md](CODEGRAPH.md)，
架构取舍是 [ARCHITECTURE.md](ARCHITECTURE.md)；这两份不重复它们，只给「怎么选路 + 怎么落地」的清单。

## 0. 一分钟选路

| 你想要什么 | 走哪条 | 要引入的东西 |
|---|---|---|
| 有个能直接被 agent 调的现成工具（不写 Rust） | [A](#路径-a用现成的域二进制零-rust-依赖) | 一个二进制 + `--mcp` |
| 自己的项目里写命令，CLI/TUI/Web/MCP 四端白拿 | [B](#路径-b在自己的-crate-里写命令推荐) | `lilyco` 一个包 |
| 只想要参数 schema + 执行宿主，界面自己管 | [C](#路径-c不引-facade只要-core--宏) | `lilyco-core` + `lilyco-macros` |
| 多命令一行启动的现成样板 | 看 `lilyco-binfmt/src/main.rs`（`lbin`，4 条全 T0） | —— |

## 路径 A：用现成的域二进制（零 Rust 依赖）

每个域二进制本身就是一台 MCP 服务器：`<bin> --mcp` 起 stdio，`tools/list` 一次返回该域全部命令，
`--schema` 打印整张注册表清单（给 agent 的第一份可读文档）。

在 agent 客户端里挂一个二进制，配置形如：

```json
{ "mcpServers": { "lbin": { "command": "lbin", "args": ["--mcp"] } } }
```

**发布现状（2026-09-23 对 crates.io 实查，别照着 README 猜）**：

| 包 | crates.io | 预编译资产 |
|---|---|---|
| `lilyco` / `lilyco-core` | 0.2.3 / 0.2.4（**仓库已是 0.3.0，尚未发布**） | —— |
| `lilyco-ffmpeg` | 0.1.0 | release 里有 `lffmpeg-<target triple>` |
| `lilyco-files`（`lfiles`）、`lilyco-binfmt`（`lbin`）、`lilyco-brush` 等域 crate | **未发布** | 只有 `lbrush` / `lvision` / `lffmpeg` / `lplc` 在 CI 产物清单里 |

所以：域 crate 目前一律 `cargo install --path lilyco-<域>` 或 `cargo build -p lilyco-<域> --release`；
`cargo binstall` 只对已发布且带资产的 crate 有效（`[package.metadata.binstall]` 的资产名是
`{ name }-{ target }{ binary-ext}`，`pkg-fmt = "bin"`）。要把某个域补进产物清单，改
`.github/workflows/ci.yml` 的 `release-build` / `android` 两处（那是人工决策，见 §4 第 11 步）。

## 路径 B：在自己的 crate 里写命令（推荐）

一个 `[dependencies]` 就够 —— **应用 crate 不要直接依赖 `lilyco-core`**（AGENTS 硬规则 3）：

```toml
[dependencies]
lilyco = { version = "0.3", features = ["full"] }   # 只要 CLI+MCP 就别开 full
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

命令 = 一个 struct + `#[derive(App)]`，字段 doc comment 就是四端共享的描述。
`run` 写成自由函数并用 `run = "函数名"` 指过去，返回 `Value`（终态事件由 `ctx.done(...)` /
executor 合成，不要自己 return 一个 `Progress`）：

```rust
use lilyco::prelude::*;
use serde_json::{json, Value};
use std::path::PathBuf;

/// 把一张图压小
#[derive(App)]
#[app(name = "squeeze", about = "压缩图片", run = "run_squeeze", safety = "t1")]
pub struct Squeeze {
    /// 要压的文件
    #[arg(about = "File to squeeze", must_exist = true)]
    input: PathBuf,

    /// 质量 0-51
    #[arg(about = "Quality", default = 23, range = 0..=51)]
    quality: f64,
}

fn run_squeeze(app: &Squeeze, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    ctx.log(LogLevel::Info, "开工");
    let result = json!({ "input": app.input.display().to_string(), "quality": app.quality });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

fn main() {
    lilyco::run::<Squeeze>();   // 四端自动选：管道→CLI，终端→TUI，--gui→Web，--mcp→MCP
}
```

可用的属性（`lilyco-macros/src/app_derive.rs` 里就这些）：
`#[app(name / about / run / safety / crate)]`；字段级 `#[arg(about / default / range / min / max / must_exist)]`。
`safety` 取 `t0/read_only`（缺省）| `t1/confirm` | `t2/token` | `t3/never_auto`。
类型推参：`bool`→Flag，数值→Number，`PathBuf`→Path，`Vec<T>`→List，`#[derive(ValueEnum)]`→Enum。

**多命令**（一个域一个二进制）：把每条命令 `RegisteredCommand::from_app::<T>()` 塞进 `Registry`，
再 `lilyco::run_registry("名字", reg)` —— 照 `lilyco-binfmt/src/main.rs` 抄，含按调用面注入安全策略那一段。

Facade 的公开入口（`lilyco/src/lib.rs`，行号见 CODEGRAPH §4）：
`detect` / `detect_backend` / `run` / `run_with` / `run_registry` / `run_registry_with` /
`run_registry_with_policy` / `run_cli_registry` / `run_tui_registry` / `run_web_registry` / `serve_mcp`。

## 路径 C：不引 facade，只要 core + 宏

适合：你自己的项目已有 CLI 框架（clap/argh），只想要「一份 schema、四端一致」的模型层，
或者要把命令暴露给别的进程而不带 lilyco 的后端。

```toml
[dependencies]
lilyco-core = "0.3"
lilyco-macros = "=0.3.0"
```

`derive(App)` 默认展开成 `::lilyco::__core::…`（即硬绑 facade）。不引 facade 时用属性覆盖展开路径：

```rust
#[derive(App)]
#[app(crate = "lilyco_core")]
pub struct Stats { /* … */ }
```

两条路径下的条目一一对应（facade 里就是 `pub use lilyco_core as __core`），所以同一份命令代码
在「嵌进 facade」和「只用 core」之间搬动不需要改字段。执行宿主仍是 `lilyco_core::executor`
（`execute` / `spawn`，`Progress` 流恒以 `Done`/`Error` 结尾）—— 这是全仓库唯一的执行宿主，
后端 crate（`lilyco-cli` / `-tui` / `-gui` / `-mcp`）也都只依赖 core，可以按需用单个后端而不是整个 facade。

## 1. 特性开关：只有一处真相

| 位置 | 特性 | 含义 |
|---|---|---|
| `lilyco-core` | **没有特性** | 后端无关的领域层；原先的 `cli/tui/gui` 三个空壳特性已删（全仓库零引用） |
| `lilyco`（facade） | `default = []` | headless：只有 CLI + MCP，纯 Rust，任何平台可编 |
| | `tui` / `web` / `ultra` | 各自 `dep:` 门控平台重依赖（crossterm / ratatui / tokio+axum） |
| | `full = ["tui","web","ultra"]` | 全功能别名 |
| 域 crate | `default = ["full"]` | 面向使用者的缺省 |
| | `full = ["dep:lilyco","lilyco/full"]` | 四端 |
| | `android = ["dep:lilyco"]` | Termux/无 crossterm 平台：headless（CLI+MCP） |
| `lilyco-gui` | `pick` | 只管它自己的原生文件选择器（`rfd` + `windows-sys`），关掉后 `/pick` 返回 501 并给可读提示 |

新增域 crate **照这套命名**，别再造 `headless` / `no-tui` / `cli` 这类同义词。
`rfd` 要 `default-features = false, features = ["xdg-portal"]`：它的 `wayland` 缺省会拉
`wayland-sys` 的 pkg-config，把 Linux 交叉检查弄挂。

## 2. 新增一个域二进制的 12 步

按 `lilyco-binfmt`（提交 `959f5e6` + `c6e2cb2`）真实改过的文件排出来的，一步对应一处：

1. `Cargo.toml` `[workspace] members` 加 `"lilyco-<域>"`
2. `lilyco-<域>/Cargo.toml`：`version.workspace = true` + §1 那三个特性 + `[[bin]] name = "l<域>"` + `[package.metadata.binstall]`
3. `src/main.rs`：`build_registry_with_policy()` + `policy_for(backend)` + `main()`（抄 lbin）
4. 每条命令一个模块：struct + `#[derive(App)]` + `run(&self, ctx)`，**命令侧带 `#[cfg(test)]` 单测**
5. 安全分级逐条写进 `#[app(safety = ...)]`（T0 只读 / T1 可逆 / T2 破坏性 / T3 提权）
6. 数值参数把 0 当「用缺省值」看：`#[arg(default = N)]` 只对 CLI 生效，Web/MCP 省略数字参数时送的是 0
7. `cargo fmt -p lilyco-<域> && cargo clippy -p lilyco-<域> --all-targets && cargo test -p lilyco-<域>`
   —— 用 `-p`，别用 `--all`（本机 rustfmt 版本可能比仓库新，会顺手改掉别人的文件）
8. `docs/<域>.md`：命令表 + 每列语义 + 一条真实输出
9. `readme.md`：该域小节（安装、四条典型调用、边界声明）
10. `docs/CODEGRAPH.md`：§1 版本表、§3/§7 该域的符号行、§9 测试地图
11. **`.github/workflows/ci.yml`：把 crate 加进 `apps` job 的两处枚举**（clippy 与 test 各一处）
    —— 漏掉这一步 = 该 crate 在 CI 上等于不存在（`lbin` 就这么漏过一次）
    要发预编译产物，再另外加进 `release-build` / `build android` 的构建与上传步骤
12. `scripts/acceptance/<域>_probe.py`：四端逐字比对（CLI 出基准 → Web SSE → MCP stdio → TUI 真 PTY），
    并在 `scripts/acceptance/README.md` 记一行用法

## 3. 集成方最常踩的六条（都来自实测）

1. **数字参数的 0 不等于「没填」**：Web/MCP 对省略的数字参数送 0，把它当「读 0 行 / 读 0 字节」用
   会让四端给出长度不同的表。参照 `lilyco-binfmt/src/{entries,symbols}.rs` 的 `DEFAULT_LIMIT` 写法。
2. **截断了要说**：读到上限就报「只读了 x / y 字节，加大 `--max-bytes` 再看」，别把截断后的空结果当结论。
3. **错误消息得点名说话的人**：多读者链式嗅探共用一个 `last_error` 时，最后一个写槽的那个的解释
   不是「这个文件为什么不行」的权威答案。
4. **Web 端只信任本机**：`/run` `/upload` `/cancel` `/pick` 全部要求 token + 回环 Origin
   （`lilyco-gui/src/security.rs`）；新增会改状态的端点，先加进 `PROTECTED_POST` 再写 handler。
5. **拖拽上传拿的是副本路径，`/pick` 拿的是原始路径**：命令侧只看到一个 `path`，但对「改回去」的
   命令来说这两者含义不同，选哪种要在该域文档里写清。
6. **四端都要真跑一遍**：`--json` 逐字比对（CLI/Web/MCP）+ 真 PTY（TUI）。只看 CLI 会漏掉 1/2/3 全部；
   模板见 `scripts/acceptance/binfmt_probe.py`。

## 4. 版本与发布

- 各 crate 版本走 `version.workspace = true`（一处改全仓），改动的 crate 在 CODEGRAPH §1 同步
- `lilyco-macros` 在 facade 里是 `=0.3.0` **精确锁**：宏展开引用 `::lilyco::__core`，
  这条 semver 看不见的边必须靠显式锁定保证同版本发布
- 发布：`bash scripts/publish.sh`（顺序 core → macros → cli → tui → gui → mcp → facade，
  用临时 `CARGO_HOME` 绕开镜像滞后；域 crate 不在该清单里，要发得另说）—— 需要 crates.io 凭据，人工执行
- GitHub Release 只在打 `v*` tag 时创建：推 `main` 只跑 CI，不会发布

## 5. 想直接看代码的话

| 目的 | 读这个 |
|---|---|
| 最小完整域二进制 | `lilyco-binfmt/src/main.rs`（4 命令 / 全 T0） |
| 命令 + 校验 + 进度事件 | `lilyco-binfmt/src/identify.rs`、`lilyco-grep/src/lib.rs` |
| 只用 core 的嵌入方 | `lilyco-macros/tests/derive_tests.rs` 的 `crate = "lilyco_core"` 那一例 |
| 后端怎么写 | `lilyco-mcp/src/lib.rs`（最小后端样板） |
| 四端契约怎么验 | `scripts/acceptance/binfmt_probe.py` |
