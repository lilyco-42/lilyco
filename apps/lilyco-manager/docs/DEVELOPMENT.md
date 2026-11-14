# Lilyco Manager 开发与维护

本文档是 `apps/lilyco-manager` 的开发闭环说明：本地命令、测试分层、CI 矩阵、跨平台约束和相邻仓库的维护边界。

## 1. 前置条件

| 工具 | 用途 | 版本基线 |
| --- | --- | --- |
| Rust stable | 管理器后端 | `rustfmt`、`clippy` 组件 |
| Bun | 前端依赖与脚本 | 1.x |
| Node.js | `scripts/*.mjs`、Playwright | 18+ |
| Chromium / Chrome | 浏览器冒烟测试 | Playwright `channel: chrome` |

前端依赖使用 `frontend/bun.lock`；Rust 依赖使用 `Cargo.lock`。两者都必须提交。

## 2. 本地开发命令

```bash
# 后端
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release

# 前端
cd frontend
bun install --frozen-lockfile
bun run typecheck
bun run build
bun run smoke

# 仓库一致性 / 性能预算
bun scripts/dev-check.mjs
bun scripts/perf-check.mjs
```

`dev-check` 校验仓库结构、文档、脚本、CI 关键触发器和 catalog；`perf-check` 校验单文件前端体积、离线约束和可选的 release 二进制体积。

## 3. 测试分层

| 层 | 位置 | 覆盖 |
| --- | --- | --- |
| 单元测试 | `src/*.rs` 的 `#[cfg(test)]` | 清单校验、路径穿越、平台资产别名、日志环形缓冲、token |
| API 契约测试 | `src/main.rs` | 401、工具列表、执行/历史、同源校验、应用列表、历史上限 |
| 前端类型检查 | `frontend/src` | Vue SFC 与 API 类型 |
| 浏览器冒烟 | `frontend/tests/commerce.spec.ts` | 离线商品图导出、WASM 内核 |
| 管理面板交互 | `frontend/tests/apps.spec.ts` | 应用列表、Release 安装、开发任务、MCP、主题切换 |

新增 API 时必须同时补契约测试；新增 UI 交互必须可在 mock API 下离线复现，不依赖真实 GitHub 或本机应用。

## 4. CI 矩阵

`.github/workflows/lilyco-manager.yml`：

- `push` 到 `main` 且路径命中 `apps/lilyco-manager/**` 或 workflow 本身。
- `pull_request` 使用相同路径过滤。
- `concurrency` 按 workflow + ref 取消旧任务。
- Rust 校验与 release 构建运行在 Ubuntu、macOS、Windows。
- 浏览器冒烟仅在 Linux 执行，避免 macOS/Windows 的 Chrome 安装差异造成假失败；性能预算在三平台都执行。
- Rust 依赖使用 `Swatinem/rust-cache`；WASM 源固定到审核过的 commit。

跨平台测试不能依赖宿主路径分隔符、宿主平台资产名或本地绝对路径。平台相关逻辑必须通过纯函数 + 显式平台键测试。

## 5. 跨平台约束

- 清单中的 `binary_path`、`release.assets` 资产名和 `dev.tasks` 参数不得包含绝对路径、盘符或 `..`。
- 平台资产键固定为 `linux-x64`、`linux-aarch64`、`windows-x64`、`windows-arm64`、`macos-universal`；别名由 `release.rs` 的 `platform_aliases` 统一处理。
- 子进程不经过 shell；argv 来自清单，HTTP 只能传应用 id / 任务名。
- 日志与开发任务输出都限制为 500 行，任务记录最多 32 个。
- 卸载只删除 `$XDG_DATA_HOME/lilyco/apps/<id>`，不删除源码或 PATH 中的版本。

## 6. 相邻仓库

| 仓库 | 关系 | 维护方式 |
| --- | --- | --- |
| `lycodl` | Release 资产与开发任务的真实样例 | 带 `.github/workflows/release.yml`；另加正确性 CI |
| `lycomc` | 本机 Minecraft 启动器 | 依赖绝对路径 `../lilyco/lilyco`，当前定位为本机开发，不承诺可移植 CI |
| `ui-spec` | 设计令牌来源 | 令牌 vendored 到 `frontend/src/design/` |

`lycomc` 若要在 CI 中构建，需要先把 `Cargo.toml` 的 `lilyco` 依赖改为可复现的 git rev 或发布包，并检出对应仓库；在此之前不要把 `lycomc` 加入公共 CI 必需矩阵。

## 7. 环境变量

| 变量 | 说明 |
| --- | --- |
| `LILYCO_MANAGER_TOKEN` | 固定会话 token；至少 24 字符。缺省随机生成 |
| `LILYCO_MANAGER_DB` | SQLite 路径；缺省 `lilyco-manager.sqlite` |
| `LILYCO_MANAGER_APPS` | 清单目录列表，使用平台路径分隔符 |
| `LILYCO_MANAGER_NO_BROWSER` | 设置后不自动打开浏览器 |
| `LILYCO_MANAGER_BIN_<ID>` | 覆盖单个应用的二进制绝对路径 |
| `LILYCO_TOOLHUB` | `hub.py` 绝对路径，用于导出 MCP 客户端配置 |
| `LILYCO_DESKTOP_SWITCHER_EXE` / `LILYCO_CLI_EXE` | 只读适配器可执行文件覆盖 |
| `GITHUB_TOKEN` / `GH_TOKEN` | 可选，提升 GitHub API 限额 |

## 8. 常见问题

- **前端构建后 Rust 仍显示旧页面**：`include_str!` 在编译期嵌入，改完前端必须重新 `cargo build`。
- **端口冲突**：多个应用默认都想用 `8766`，在各自清单的 `run.env` 分配不同端口。
- **安装被拒绝**：只接受带 `sha256:` digest 的正式 Release 资产；检查资产名与平台键。
- **Playwright 找不到浏览器**：本地安装 Chrome 或使用 `channel: chrome` 对应的浏览器；CI 只在 Linux 跑浏览器冒烟。
- **历史记录过多**：后端只保留最近 500 条；`/api/history` 默认返回 50 条。
