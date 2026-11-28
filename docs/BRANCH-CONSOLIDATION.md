# 分支合并状态（2026-11-28）

## 目标

在 Hub-and-Spoke 架构（见 `docs/THEME-REPOS.md`）下审查历史 feat 分支：将与主干兼容的改动合并回 `main`，应用类改动归主题仓，不把已迁出的 `apps/` 重新引入主干。

## 已合并

- `feat/mcp-negotiation-resources-images` → merge `74cd519`：MCP 协议版本协商、`resources/*` 与图片内容块；`lilyco-mcp` 38 项测试通过。
- `1167ba5`：修复 `crates/lilyco-dev/src/scaffold.rs` 的 rustfmt 漂移。
- CI run 46138593925 全平台通过（ubuntu / windows / android / musl / hygiene）。

## 分支研判

| 分支 | 状态 | 处理 |
|---|---|---|
| `feat/audit-export`、`feat/read-token` | 提交已在 main 历史中 | 无需处理 |
| `fix/deterministic-registry-order` | 提交已在 main 历史中 | 无需处理 |
| `feat/manager-api-docs`、`feat/smoke-projects` | 功能已在 `Lyco-team/lilyco-agent`（接口文档“试调用”、项目/方案生命周期冒烟） | 不再合入 main |
| `feat/lan-access` | 完全包含于 `feat/native-ui-slice` | 不单独处理 |
| `feat/rapid-kiri` | 完全包含于 `feat/lilyco-rapid` | 不单独处理 |
| `feat/native-ui-slice` | 核心 crate 源码与 main 一致；增量为 lain42-portal、chatgpt/web2cli | chatgpt/web2cli 已在 `lilyco-ai`；lain42 待独立仓 |
| `feat/lilyco-rapid` | 核心 crate 无未迁移源码 | `lilyco-rapid` / `lilyco-no_std` 待主题仓归属 |

## 架构约束

`18a8f30` 已移除 `apps/`，主干只保留 10 个核心 SDK crate。对旧分支做整支 merge 会产生 63 处“修改/删除”冲突并复活旧路径（已用 `git merge-tree` 与 `--no-commit` 试合并验证），因此不整支合并。

## 待决

1. `lain42-portal` 独立仓归属（含商业配置，建议私有仓）。
2. `lilyco-rapid` / `lilyco-no_std`：新建主题仓，或作为新核心 crate 合入 `crates/`。
3. 已完成分支的归档或删除。

## 规范检查

- MCP 新增生产代码零 `unwrap` / `expect` / `panic`，许可证 MIT OR Apache-2.0。
- lain42 新增代码的 `unwrap` 集中在测试；订阅节点表为硬编码，迁移独立仓时应外置为 TOML 配置。