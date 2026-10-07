## Testing

本地：

```bash
# Run all tests
cargo test --workspace

# Run a specific crate
cargo test -p lilyco-core
cargo test -p lilyco-cli
cargo test -p lilyco-tui
cargo test -p lilyco-macros
cargo test -p lilyco-ultra-ui
cargo test -p lilyco-mcp
cargo test -p lilyco-vision
```

CI（GitHub Actions）：push / PR 自动跑 **ubuntu + windows 双矩阵** —
`cargo fmt --check` + `cargo clippy --workspace --all-targets` + `cargo test --workspace` + `cargo doc`；
打 tag `v*` 自动构建 5 个二进制（Windows + Android-arm64：lbrush / lvision / lffmpeg / **laic** / lbin）并发布 GitHub Release。
见 `.github/workflows/ci.yml`。Windows TUI 从此由 CI 持续验证编译与单元测试。

Current coverage: **984 tests, all passing** + 端到端冒烟（`examples/multi.rs`）+ 性能基准（`cargo bench -p lilyco-example`）.

口径：`cargo test --workspace --no-fail-fast`，windows-latest + Git Bash，2026-10-05 实测
**984 passed / 0 failed**（58 个测试目标）。

两个前置条件，缺任何一个都会看到假红：

- `lilyco-brush` 的 8 个 shell 集成测试需要 `BRUSH_PATH` 指向一个可用的 shell。该变量覆盖路径时，
  shell 种类按**文件名**判定（brush 独有 `--no-config`，拿它去喂 bash 会直接以非 0 退出）：
  `BRUSH_PATH="C:/Program Files/Git/bin/bash.exe" cargo test -p lilyco-brush` 即全绿。
- `lilyco-tauri` 的 sidecar 要先在位（桌面端跑的就是 CI 上那本对账认的 `lbin`）：
  `cargo build -p lilyco-binfmt --bin lbin` 后把 `lbin.exe` 复制成
  `lilyco-tauri/binaries/lbin-x86_64-pc-windows-msvc.exe`（`binaries/` 已被 git 忽略）。
  `desktop.yml` 里就是这么串的；单独 `cargo build -p lilyco-tauri` 必然报
  `resource path binaries\lbin-...exe doesn't exist`。

`lilyco-tauri` 被排除是因为它的 build script 在 Windows 上编不过（同样与测试无关）。

---

