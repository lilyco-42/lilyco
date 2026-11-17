# 第 7 章 · 发布与验收

> **本章你会学到**
> 1. release 构建与「profile 只在根 crate 生效」这条规则；
> 2. 怎么在不带 TUI/Web 的平台（Android / Termux）上构建；
> 3. 交付前必须自己跑一遍的验收清单；
> 4. 版本号与发布路径。

---

## 7.1 release 构建

```bash
cargo build --release
```

产物在 `target/release/filecount`（本机因 `CARGO_TARGET_DIR` 在 `~/.cache/lilyco-build/target/release/`）。

一个容易误解的点：**`[profile.release]` 只在你自己的 workspace 根生效**。
lilyco 仓库根的那套优化（`opt-level="z"`、`lto="fat"`、`strip`、`panic="abort"`）
不会自动应用到你的独立项目 —— 你的依赖会按你项目里的 profile 编。

想复刻那套体积优化，在自己的 `Cargo.toml` 里写：

```toml
[profile.release]
opt-level = "z"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = "symbols"
```

代价：`lto="fat"` + `codegen-units=1` 会让 release 编译显著变慢；
`panic="abort"` 意味着没有 unwind，某些依赖可能不兼容。先量体积，再决定要不要全开。

---

## 7.2 体积从哪来

三个杠杆，按收益排序：

| 杠杆 | 做法 | 注意 |
|---|---|---|
| 去掉重依赖 | 默认 `features = []`（headless）只留 CLI + MCP | 没有 `--gui` / TUI |
| LTO + strip | 上面的 profile | 编译慢，符号没了 |
| 裁剪特性 | 依赖的 `default-features = false` | 逐包确认能力没丢 |

`--gui` 的成本主要是 `axum` + `tokio`（Web 服务）与 `crossterm` + `ratatui`（TUI）。
一个纯 CLI 工具不需要它们。

验证体积变化时，用同一个 profile 对比前后两次构建，不要拿 debug 和 release 比。

---

## 7.3 跨平台

| 目标 | 依赖写法 | 结果 |
|---|---|---|
| Linux x86_64 | `features = ["tui", "web"]` | 四端齐全 |
| Windows x86_64 | 同上 | 四端齐全（crossterm 支持） |
| Android / Termux | `lilyco = { ... }`（不开 tui/web） | CLI + MCP；**crossterm 不支持 android** |
| 任意 headless | `--no-default-features` | CLI + MCP，纯 Rust |

Android 构建时不要把 `tui` 打开 —— 它会让编译在 crossterm 处失败。
`lilyco` facade 的 `default = []` 就是为了这个：默认就是可移植的那一档。

---

## 7.4 交付前验收清单

按顺序过一遍，前四项能自动化，后四项必须人看。

| # | 检查 | 命令 / 方法 | 通过标准 |
|---|---|---|---|
| 1 | 构建 | `cargo build --release` | 退出码 0 |
| 2 | 单元测试 | `cargo test` | 全绿 |
| 3 | 契约 | `cargo run -- --schema` | JSON 合法，参数与预期一致 |
| 4 | 事件流 | `cargo run -- --file X --json-stream` | 最后一行是 `done` |
| 5 | Web | `cargo run -- --gui` | HTTP 200，页面能运行一次 |
| 6 | 窄容器 | 把页面塞进 360px iframe | 无横向滚动条，按钮不塌 |
| 7 | 动效偏好 | 系统开「减少动态效果」 | 进度条仍有「在跑」的静态信号 |
| 8 | 对比度 | `getComputedStyle` 取色后按 WCAG 算 | 正文 ≥7:1、次要 ≥4.5:1、控件边 ≥3:1 |

第 5 项的自动化形态（本机实测）：

```bash
LILYCO_PORT=18999 cargo run -- --gui &
sleep 4
curl -s -o /tmp/page.html -w '%{http_code} %{size_download}B\n' http://127.0.0.1:18999/
grep -c 'data-component=' /tmp/page.html     # 组件自报家门的数量
kill %1
```

第 6 / 7 / 8 项没有捷径：`cargo test` 看不见对比度，也看不见 360px 里的挤压。
细则与实测方法见 [设计语言](../03-design-language.md) §6–§9。

---

## 7.5 版本与发布

三条约定（仓库内）：

1. 各 crate 独立版本号；改动的 crate patch +1；
2. 提交信息用 Conventional Commits（`feat(lilyco-mcp): ...`）；
3. 发布走 `bash scripts/publish.sh`（按依赖序全链，需先 `cargo login`）。

你自己的应用只需要：

```toml
[package]
version = "0.1.0"
```

```bash
cargo publish          # 发布到 crates.io（需要 token）
# 或
cargo binstall filecount   # 用户侧安装（若已发布）
```

发布前把 `lilyco = { path = "..." }` 换成版本号，否则 `cargo publish` 会拒绝 path 依赖。

---

## 7.6 现在你应该看到

| 命令 | 应该看到 |
|---|---|
| `cargo build --release` | 通过；产物在 `target/release/`（或 `CARGO_TARGET_DIR`） |
| `cargo test` | 全绿 |
| `--schema` / `--json-stream` / `--gui` | 与 debug 版行为一致 |
| 换一台机器（或 Termux） | headless 构建能跑 `--help` 与 `--mcp`，没有 `--gui` |

---

## 小结

- release 优化要写在你自己的 `Cargo.toml` 里；仓库根的 profile 只管仓库自身。
- 体积三个杠杆：去重依赖 → LTO + strip → 裁特性。
- Android 等平台不要开 `tui`；默认 `features = []` 就是可移植档。
- 交付前 8 项验收：构建、测试、契约、事件流、页面、窄容器、动效偏好、对比度。
- 发布前把 path 依赖换成版本号。

---

## 练习

1. **量一次体积**：分别用 `features = []` 与 `["tui","web"]` 构建 release，记录两个产物大小。
   提示：同一台机器、同一个 profile，才有可比性。
2. **跑第 6 项**：把页面塞进 360px 宽的 iframe，截图或读 DOM 几何，确认没有横向溢出。
3. **补一条 CI**：在项目里加一个最小 workflow，跑 `cargo test` 与 `--schema`（JSON 能被 `jq` 解析）。
   提示：契约值得在 CI 里守住，它是 agent 侧唯一的入口。

---

## 下一章

[第 8 章 · 声明组件，而不是画组件](08-view-components.md)：把展示层从业务里拆出去。