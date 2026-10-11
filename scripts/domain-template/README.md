# 域模板（domain-template）

`lilyco new <name>` 与 `scripts/new-domain.sh` 的**单一事实源**：一个「一域一二进制 × 四端
（CLI / TUI / Web / MCP）」的最小骨架，含 `Cargo.toml`、`src/main.rs`、`src/show.rs`。

本文件不会被生成到新 crate 里 —— `scaffold_files()` 只写那三个文件（有断言）。

## ⚠️ 清单为什么叫 `Cargo.toml.tmpl` 而不是 `Cargo.toml`

**不要改回去。** 这不是笔误。

cargo 在解析 git 依赖时，会**递归读取检出目录里的每一个 `Cargo.toml`**。
出处：`cargo/src/sources/path.rs` 的 `read_packages()` / `read_nested_packages()`，
源码注释原话是：

> git source try to find and read all manifests from the repository
> but since it's not possible to exclude folders from this search
> it's safer to ignore malformed manifests to avoid …

也就是说 —— **`[workspace] exclude` 对这条遍历无效**（cargo 自己在 TODO 里也写了
「Add a way to exclude folders?」）。只要目录里存在一个名为 `Cargo.toml` 的文件，
它就会被读到并解析。

而模板清单里的 `name = "@CRATE@"` 是占位符，不是合法包名。后果是：**任何把 lilyco
当 git 依赖的仓库**（例如 `lilyco-42/lgame`）都会在 `cargo` 起步阶段看到一行红字：

```
error: invalid character `@` in package name: `@CRATE@`, the first character must be a Unicode XID start character
  --> …/.cargo/git/checkouts/lilyco-…/scripts/domain-template/Cargo.toml
 2 | name = "@CRATE@"
```

它**不致命**（cargo 会跳过这份畸形清单，随后照常 `Locking N packages`），但对使用者
是纯粹的惊吓 —— 每次拉依赖都以为构建坏了。

改名的效果：目录里不再有 `Cargo.toml`，cargo 的遍历就不会碰它。占位符照旧保留，
`include_str!` 与 `scripts/new-domain.sh` 里的 `cp` 已同步改到 `.tmpl`。

## 谁在用它

| 使用方 | 位置 |
|---|---|
| `lilyco new <name>`（推荐路径） | `lilyco-dev/src/scaffold.rs` 的 `include_str!` |
| 手抄脚本 | `scripts/new-domain.sh` 的 `cp` |

改模板后记得两边都能拿到新内容（前者编译期内嵌，改完要重编 `lilyco-dev`；
由 CI 验证）。完整的新增域清单见 [`docs/INTEGRATION.md`](../../docs/INTEGRATION.md)。
