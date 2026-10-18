# WASM 工具宿主

> 原则：**grep / glob / 结构搜索 / 重写这类能力，不自己写**。
> 别人已经做得更好（ast-grep、ripgrep、formatter、language server），
> 框架只负责**加载和调用**它们。

## 现状

| 层 | 谁实现 | 说明 |
| :--- | :--- | :--- |
| 工具宿主 | 我们，约 100 行 | `agent/wasm_tool.rs`：extism 加载 `.wasm`，调用 `run(input) -> output` |
| 工具本体 | **别人** | 任何 extism-pdk 插件；ast-grep 是目标 |
| 沙箱层 | 我们，三个小工具 | `read_file` / `list_dir` / `edit_file` —— 它们不该被"外包"，因为**路径越权与唯一锚点**是 agent 的安全边界 |

## 用法

```bash
tiny-agent <workspace> --apply \
  --wasm-tool ast_grep=/path/to/ast_grep.wasm \
  --call "ast_grep:pattern=console.log($A);lang=ts"
```

插件名由 `--wasm-tool name=path` 指定，调用时用同一个名字。
插件默认被视为 **mutating**，因此需要 `--apply`（它们可能改写文件）。

## 插件 ABI

遵循 [Extism PDK](https://extism.org)：导出 `run`，输入输出都是字符串。

```rust
use extism_pdk::*;

#[plugin_fn]
pub fn run(input: String) -> FnResult<String> {
    Ok(do_the_work(&input))
}
```

仓库里的 [`tests/fixtures/wasm_tool`](../tiny_lilyco/tests/fixtures/wasm_tool) 就是最小样例，
CI 会真的编译成 `wasm32-unknown-unknown` 并调用它。

## 接 ast-grep 的实际障碍（诚实记录）

ast-grep 本体是 Rust，但它的解析器依赖 **tree-sitter 的 C 代码**。因此：

| 目标 | 可行性 |
| :--- | :--- |
| `ast-grep-core` → `wasm32-unknown-unknown` | ❌ 卡在 C 依赖 |
| `ast-grep-core` → `wasm32-wasi` + 带 C 的交叉工具链 | ⚠️ 需要预编译 tree-sitter 语法 |
| 用 gh 上第三方 [`nounder/ast-grep-wasm`](https://github.com/nounder/ast-grep-wasm) 产物 | ⚠️ 非官方，需自行审计 |
| 先用 CLI 形态：宿主执行 `ast-grep` 可执行文件 | ✅ 立即可用（`run_command` 白名单加 `ast-grep` 即可） |

**短期最省事的路线**：把 `ast-grep` 加进 `run_command` 的白名单，让 agent 直接调 CLI；
等有人产出可信的 wasm 构建再切过去。宿主接口已经就位，切换不需要改调用方。

## 为什么不把 `grep`/`glob` 删掉

它们是**离线兜底**：没有网络、没有插件文件时，agent 仍要能读文件。
但它们的定位是"最小可用"，不是"最终形态"——有插件时优先插件。
