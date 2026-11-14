# Lilyco Manager 性能基线

管理器是本地回环应用，性能目标不是吞吐，而是**小体积、低常驻开销、可预测的本地响应**。本文件记录预算、测量方法和当前基线。

## 1. 预算

| 指标 | 预算 | 执行位置 |
| --- | --- | --- |
| 单文件前端 raw | 1 MiB | `scripts/perf-check.mjs`、CI |
| 单文件前端 gzip | 128 KiB | `scripts/perf-check.mjs`、CI |
| release 二进制 | 32 MiB | `scripts/perf-check.mjs`（存在时） |
| 运行历史 | 最近 500 条 | `HISTORY_LIMIT` + `prune_history` |
| 单应用日志 | 500 行 | `supervisor.rs` |
| 开发任务日志 | 500 行 / 任务 | `dev.rs` |
| 开发任务记录 | 32 个 | `dev.rs` |
| 外部适配器调用 | 4 秒超时、64 KiB stdout | `main.rs` |
| `--schema` 调用 | 5 秒超时、512 KiB stdout | `main.rs` |
| Release 下载 | 256 MiB | `release.rs` |
| Release 解压 | 512 MiB | `release.rs` |

任何预算调整都必须在 PR 中给出理由和新的测量结果。

## 2. Release 构建配置

`Cargo.toml`：

```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

- `lto = "thin"`：跨 crate 内联，构建时间远低于 fat LTO，体积收益明显。
- `codegen-units = 1`：更好的优化与更小的二进制。
- `strip = true`：去掉符号，管理器不需要线上调试符号。

不使用 `panic = "abort"`，保留 panic 展开以便未来接入崩溃上报或优雅关闭。

## 3. 测量

```bash
# 前端体积
cd frontend && bun run build
cd .. && bun scripts/perf-check.mjs

# 二进制体积
cargo build --release
ls -lh target/release/lilyco-manager

# 冷启动到监听（Linux）
LILYCO_MANAGER_NO_BROWSER=1 LILYCO_MANAGER_TOKEN=0123456789abcdef0123456789abcdef \
  LILYCO_MANAGER_DB=/tmp/lilyco-perf.sqlite target/release/lilyco-manager &
PID=$!
# 等日志出现 "listening on" 后观察常驻内存
grep -E 'VmRSS|VmHWM' /proc/$PID/status
kill $PID
```

`/usr/bin/time` 并非所有发行版都预装；Linux 上直接读 `/proc/<pid>/status` 更可靠。

## 4. 当前基线（2026-11，Linux x64）

| 项目 | 数值 |
| --- | --- |
| `frontend/dist/index.html` raw | 162,648 B |
| gzip | 58,064 B |
| release 二进制（Linux x64，strip + thin LTO） | 6,699,280 B（约 6.4 MiB） |
| 启动到打印监听地址（Linux x64） | 约 12 ms |
| 空闲常驻内存 RSS（Linux x64，18 线程） | 约 8.0 MiB |
| 运行历史上限 | 500 |
| 单应用日志上限 | 500 行 |
| 开发任务记录上限 | 32 |

## 5. 设计上的性能取舍

- **单文件前端**：`vite-plugin-singlefile` 内联所有 JS/CSS，换来零外部请求和离线可用；代价是 HTML 体积略大于拆分产物。当前仍远低于 1 MiB。
- **按需加载重依赖**：`@huggingface/transformers` 等可选 ML 运行时不得进入首屏同步路径；新增依赖前先跑 `perf-check`。
- **MCP 聚合并发**：`/api/mcp/tools` 用 `JoinSet` 并行查询各应用，避免串行启动多个 `--mcp` 进程；结果截断到 1000 个工具。
- **有界内存**：日志、任务、历史都有硬上限，长期运行不会因 UI 轮询持续增长。
- **本地 SQLite**：单文件、无服务端；写入历史失败不影响工具执行结果。
- **无缓存层**：本地单用户场景下，避免引入额外一致性成本；GitHub Release 查询直接走 API，可选 token 提升限额。

## 6. 回归排查顺序

1. `bun scripts/perf-check.mjs` 是否通过。
2. 新增前端依赖是否进入首屏；检查 `dist/index.html` 的 raw/gzip 变化。
3. 新增 Rust 依赖是否显著增加 release 二进制；优先使用已有依赖的 feature。
4. 日志/任务/历史是否仍受上限约束。
5. 启动是否仍在打印监听地址后立即进入服务循环；避免在启动路径做网络请求。
