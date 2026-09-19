# lfiles — 文件整理 / 查找 / 去重使用文档

`lilyco-files` 是 **lilyco 框架的「一域一二进制 × 四端」样板**：一个二进制挂一整个「文件域」
的 4 条命令，天然获得 **CLI / TUI / Web / MCP** 四种界面 + **AI 可调用**，
全程报告实时进度、支持取消，跨平台（含 Android/Termux）。

它同时是**新 app 的抄写模板** —— `main.rs` 的 registry 装配（含安全策略按调用面注入）
就是每个新域该有的形状。

- 仓库：`https://github.com/lilyco-42/lilyco`
- 二进制名：`lfiles`
- 依赖：**无外部依赖**（纯 `std::fs`，不调用任何外部进程 → 无命令注入面）

---

## 安装

### 1. cargo binstall（推荐，免编译）

```bash
cargo install cargo-binstall   # 首次
cargo binstall lilyco-files
```

### 2. cargo install（源码编译）

```bash
cargo install --git https://github.com/lilyco-42/lilyco lilyco-files
```

### 3. 从仓库直接跑（开发）

```bash
git clone https://github.com/lilyco-42/lilyco
cd lilyco
cargo run -p lilyco-files -- find --root . --pattern '*.jpg'
```

---

## 命令一览

| 命令 | 安全级 | 干什么 |
|---|---|---|
| `find` | **T0** 只读 | 递归查找文件（glob / 扩展名 / 体积区间 / 深度） |
| `stats` | **T0** 只读 | 磁盘占用汇总（按扩展名、按一层子目录、最大 N 文件） |
| `dedup` | **T0** 只读 | 找重复文件（三段式：体积分桶 → 头尾指纹 → 全量哈希），**只报告不删** |
| `rename` | **T1 需确认** | 批量重命名（前缀/后缀 或 查找替换），**默认 dry-run** |

---

## 快速上手

```bash
cd ~/Pictures
lfiles find --pattern '*.jpg'              # 找出来（人读格式）
lfiles find --pattern '*.jpg' --json       # 找出来（结构化，给脚本/AI）
lfiles stats                               # 谁占了空间
lfiles dedup --min-size 1024               # 有没有重复
lfiles rename --prefix 2026_              # 预览改名（不动盘）
lfiles rename --prefix 2026_ --apply      # 真改
```

---

## 全部参数

### `find`

| 参数 | 类型 | 说明 |
|---|---|---|
| `root` | Path（必填，须存在） | 扫描起点 |
| `pattern` | Text | glob，**只匹配文件名**（`*` 任意串 / `?` 单字符 / `**`） |
| `ext` | Text | 扩展名过滤，逗号分隔（如 `jpg,png`），大小写不敏感 |
| `min-size` / `max-size` | Number (u64) | 字节区间 |
| `max-depth` | Number (u32) | `0` = 只看根一层；省略 = 不限 |
| `ignore-case` | Flag | 忽略大小写 |
| `limit` | Number (u64) | 最多返回多少条 |
| `json` | Flag | 输出结构化 JSON |

返回：`{ root, count, total_size, total_size_human, by_ext: {ext: count}, files: [{path,size,size_human,ext}], duration_ms }`，按 path 排序。

### `stats`

| 参数 | 类型 | 说明 |
|---|---|---|
| `root` | Path（必填） | 统计起点 |
| `largest` | Number (u64) | 最大文件列几条（默认 10） |

返回：`{ root, file_count, total_size, total_size_human, by_ext: [{ext,count,size,size_human,percent}], by_dir: [{dir,count,size,size_human}], largest: [{path,size,size_human}], duration_ms }`，各维度按体积降序。

### `dedup`

| 参数 | 类型 | 说明 |
|---|---|---|
| `root` | Path（必填） | 扫描起点 |
| `min-size` | Number (u64) | 小于此体积跳过（默认 1 字节） |
| `ext` | Text | 只查这些扩展名 |

返回：`{ root, groups: [{size, size_human, wasted, hash, files: [path…]}], group_count, duplicate_files, wasted_bytes, wasted_human, scanned, duration_ms }`。

**只报告，绝不删除。** 三段式检测保证只对"已经体积相同 + 头尾 4KB 指纹相同"的文件做全量哈希。

### `rename`

| 参数 | 类型 | 说明 |
|---|---|---|
| `root` | Path（必填） | 操作目录 |
| `pattern` | Text | 只动匹配的文件（glob，文件名） |
| `prefix` / `suffix` | Text | 加前缀 / 后缀（`a.jpg` → `IMG_a.jpg` / `a_bak.jpg`） |
| `replace-from` / `replace-to` | Text | 子串替换（替换**全部**出现） |
| `ignore-case` | Flag | 替换时忽略大小写 |
| `apply` | Flag | **不加就是 dry-run**；加上才真改 |
| `overwrite` | Flag | 目标已存在时也继续（默认中止） |
| `allow-reapply` | Flag | 允许重复叠加前缀/后缀（默认跳过已带的） |

返回：`{ root, dry_run, matched, count, renames: [{from,to}], skipped: [{from,reason}], hint, duration_ms }`。

---

## 示例

```bash
# 只找根一层的大文件
lfiles find --root . --min-size 104857600 --max-depth 0

# 按扩展名（多种）
lfiles find --root . --ext jpg,png --ignore-case

# 谁占空间
lfiles stats --root /var/log --largest 20

# 找重复（忽略小文件）
lfiles dedup --root ~/Downloads --min-size 1048576 --json

# 加日期前缀（先预览）
lfiles rename --root ./photos --prefix 2026_0901_
lfiles rename --root ./photos --prefix 2026_0901_ --apply

# 查找替换（把所有 "副本" 去掉）
lfiles rename --root . --replace-from 副本 --replace-to "" --apply

# 只改 jpg
lfiles rename --root . --pattern '*.jpg' --suffix _bak --apply
```

---

## 四种界面

```bash
lfiles find --root . --pattern '*.jpg'          # CLI：直接出结果
lfiles --tui                                    # TUI：命令选择页 → 表单 → 运行视图
lfiles --gui                                    # Web：浏览器控制台（?cmd= 下拉切换命令）
lfiles --mcp                                    # MCP stdio 服务器（供 AI / DSH）
```

> **注意**：多命令形态下**裸跑 `lfiles` 会降级成 CLI**（不会把你拽进交互界面），
> 进 TUI 必须显式 `--tui`。这是 `detect_registry_backend()` 的约定 —— 多命令 app 裸跑
> 通常是脚本调用，不该被交互界面拦住。

### AI / 脚本消费（`--json-stream`）

每个事件一行 JSONL：`started` → `tick*` → `done` 或 `error`。

```bash
lfiles find --root . --pattern '*.jpg' --json-stream
# {"type":"started","message":"..."}
# {"type":"done","result":{...},"duration_ms":3}
```

### MCP 工具形状

`tools/list` 一次返回全部 4 个工具，**safety tier 写进 description**（agent 能看到门槛）：

```
find     [safety: T0]   params: root, pattern, ext, min-size, max-size, max-depth, ignore-case, limit
stats    [safety: T0]   params: root, largest
dedup    [safety: T0]   params: root, min-size, ext
rename   [safety: T1]   params: root, pattern, prefix, suffix, replace-from, replace-to, ...
```

`tools/call rename` 在 MCP 面**会被安全门拒绝**（自动化面 fail-closed），
提示 `请在本地交互面执行，或注册时换用允许该分级的 SafetyPolicy`。
同一份 handler 在 CLI/TUI/Web 面（人类在环）则放行 —— 这是「安全策略按调用面注入」的体现。

若模型只吃 OpenAI tool 形状，用 `--schema` 导出后做一次 MCP→OpenAI 的形状转换即可。

---

## 行为约定

- **dry-run 默认**：`rename` 不加 `--apply` 绝不改盘。
- **冲突先检后改**：多个文件改到同一目标、或目标已存在 → **在任何改名之前**整体中止，
  不产生半改状态。报错信息给出具体路径与 `--overwrite` 提示。
- **幂等**：`--prefix IMG_` 跑两次**不会**变成 `IMG_IMG_a.jpg`（已带前缀的进 `skipped`）；
  要显式叠加得加 `--allow-reapply`。`--replace-from` 模式天然幂等。
- **跳过目录**：`.git` / `node_modules` / `target` / `__pycache__` / `dist` / `build` /
  `.venv` 等构建与 VCS 目录一律不进扫描；**不跟随符号链接**。
- **绝不删文件**：`dedup` 只出报告。要回收空间需人工确认后自行删除。
- **无 shell**：全部用 `std::fs`，没有 `Command::new`，不存在参数注入面。

---

## 作为「新域」的抄写模板

`lilyco-files/src/main.rs` 就是每个新 app 该有的形状：

```rust
pub fn build_registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);   // ← 必须在 register 之前
    for c in [RegisteredCommand::from_app::<find::Find>(),
              RegisteredCommand::from_app::<rename::Rename>(),
              RegisteredCommand::from_app::<dedup::Dedup>(),
              RegisteredCommand::from_app::<stats::Stats>()] {
        reg.register(c).expect("命令名冲突");
    }
    reg
}

fn main() {
    let backend = lilyco::detect_registry_backend();
    let reg = build_registry_with_policy(policy_for(backend));
    lilyco::run_registry_with("lfiles", reg, backend);   // 四端一行分发
}
```

四端**不共享业务逻辑，只共享 `CommandSchema`** —— 所以加一个新域 = 写命令 + 装配 registry，
四端自动获得，没有四份工作量的说法。

---

## 开发 / 测试

```bash
cargo test -p lilyco-files        # 69 个单元测试
cargo clippy -p lilyco-files --all-targets
cargo fmt -p lilyco-files -- --check
```

测试分布：`util` 10（walk / glob / 体积格式化）、`find` 9、`rename` 11+（含冲突中止、
幂等、`allow-reapply`）、`dedup` 10、`stats` 7、`main` 8（registry 装配 + 安全策略映射）。

> 沙箱内跑测试若遇 `lbrush` 假失败，是 `BASH_ENV` 被注入所致（与代码无关）：
> 用 `env -u BASH_ENV cargo test --workspace`。
