# 分支合并状态（2026-11-28）

## 目标

在 Hub-and-Spoke 架构（见 `docs/THEME-REPOS.md`）下审查历史 feat 分支：将与主干兼容的改动合并回 `main`，应用类改动归主题仓，不把已迁出的 `apps/` 重新引入主干。

## 已合并

- `feat/mcp-negotiation-resources-images` → merge `74cd519`：MCP 协议版本协商、`resources/*` 与图片内容块；`lilyco-mcp` 38 项测试通过。
- `1167ba5`：修复 `crates/lilyco-dev/src/scaffold.rs` 的 rustfmt 漂移。
- `5a97212`：`feat/lilyco-rapid` 的 `rapid-core` / `rapid-server` / `lilyco-rapid` / `lilyco-no_std` 扁平化迁入 `crates/`；生成模板随 CLI 内嵌，产物可独立 `cargo metadata`。
- `lain42-portal` 拆出私有仓 [`Lyco-team/lain42-portal`](https://github.com/Lyco-team/lain42-portal)，保留 107 个提交历史；追加 `.gitignore`（`58cd101`）。
- CI run 46138593925 全平台通过（ubuntu / windows / android / musl / hygiene）。

## 分支研判

| 分支 | 状态 | 处理 |
|---|---|---|
| `feat/audit-export`、`feat/read-token` | 提交已在 main 历史中 | 删除本地分支 |
| `fix/deterministic-registry-order` | 提交已在 main 历史中 | 删除本地分支 |
| `feat/manager-api-docs`、`feat/smoke-projects` | 功能已在 `Lyco-team/lilyco-agent`（接口文档“试调用”、项目/方案生命周期冒烟） | 删除本地分支 |
| `feat/lan-access` | 完全包含于 `feat/native-ui-slice` | 删除本地分支 |
| `feat/rapid-kiri` | 完全包含于 `feat/lilyco-rapid` | 删除本地分支 |
| `feat/native-ui-slice` | 核心 crate 源码与 main 一致；增量为 lain42-portal、chatgpt/web2cli | chatgpt/web2cli 已在 `lilyco-ai`；lain42 已拆到私有仓 |
| `feat/lilyco-rapid` | 核心 crate 无未迁移源码 | 已迁入 `crates/`，删除本地分支 |

## 架构约束

`18a8f30` 已移除 `apps/`，主干只保留核心 SDK crate。对旧分支做整支 merge 会产生 63 处“修改/删除”冲突并复活旧路径（已用 `git merge-tree` 与 `--no-commit` 试合并验证），因此不整支合并。

## 后续

1. ✅ `lain42-portal` 已拆到私有仓 `Lyco-team/lain42-portal`。
2. ✅ `lilyco-rapid` / `lilyco-no_std` 已作为核心 crate 迁入 `crates/`。
3. 分支清理：删除已合并/已迁移分支，保留 `main`、`feat/native-ui-slice` 与最多两个活跃分支。

## 规范检查

- MCP 新增生产代码零 `unwrap` / `expect` / `panic`，许可证 MIT OR Apache-2.0。
- lain42 新增代码的 `unwrap` 集中在测试；订阅节点表为硬编码，迁移独立仓时应外置为 TOML 配置。
- rapid 系列许可证 MIT / MIT OR Apache-2.0，依赖为 crates.io 固定版本，符合商业合规要求。