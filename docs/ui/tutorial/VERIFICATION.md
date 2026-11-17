# 验证记录

> 这套教程里的每个输出都来自本机实跑。本页记录**怎么验的、退出码是什么、哪些没验**。
> 跑验证用的 crate 在 `/tmp/lilyco-book`（临时目录，不随仓库提交），
> 四个 example 与第 1–4 章一一对应。

---

## 环境

| 项 | 值 |
|---|---|
| 平台 | Linux（CachyOS，kernel 6.18） |
| Rust / cargo | 1.99.1 |
| 目标目录 | `CARGO_TARGET_DIR=$HOME/.cache/lilyco-build/target` |
| 验证时间 | 2026-11-16 |

---

## 逐章构建

命令与退出码：

| 章 | 命令 | 退出码 | 输出摘要 |
|---|---|---|---|
| 1 | `cargo build --example ch01` | 0 | `Finished dev profile` |
| 2 | `cargo build --example ch02` | 0 | 1 条 `field quiet is never read`（教程已解释） |
| 3 | `cargo build --example ch03` | 0 | 警告消失（quiet 已用于日志） |
| 4 | `cargo build --example ch04` | 0 | 注册两个命令 |
| 全部 | `cargo build --examples` | 0 | `Finished dev profile` |

第 5–7 章不新增 Rust 代码：第 5 章只改令牌/样式，第 6–7 章复用 `ch04` 的二进制。
因此「每章结束可编译」在代码层由 ch01–ch04 一次 `cargo build --examples` 覆盖。

---

## 四形态（ch04）

| 形态 | 命令 | 退出码 / 结果 |
|---|---|---|
| CLI + JSON | `ch04 c --file Cargo.toml --json` | 0，输出 `{file,label,mode,tags,value}` |
| 契约 | `ch04 --schema` | 0，数组含 `count` 与 `echo` 两条 |
| 事件流 | `ch04 c --file Cargo.toml --json-stream` | 0，末行 `{"type":"done",...}` |
| Web | `LILYCO_PORT=18996 ch04 --gui` + `curl '?cmd=count'` | HTTP 200，45130 B |

### 页面六类控件（`?cmd=count`，按 `data-component` 计数）

```text
flag=1  text=1  number=1  enum=1  path=1  list=1
```

`?cmd=echo` 切换后：`card-title">echo`、`data-component="text"`。

### 真实浏览器渲染（Chromium headless）

```bash
chromium --headless=new --no-sandbox --disable-gpu --virtual-time-budget=4000 \
  --dump-dom 'http://127.0.0.1:18996/?cmd=count' > /tmp/dom.html
```

| 断言 | 结果 |
|---|---|
| 渲染后 DOM | 45,192 B |
| 六类 `data-component` | flag / text / number / enum / path / list 各 1 |
| 脚本已装配 | `#preview` 有内容：`count --mode lines --limit 0` |
| 装配失败行 | `class="line err"` 计数 0 |

headless 无法点击按钮，所以「点击运行」对应的链路单独验证（下一节）。

### 运行链路（POST /run + SSE）

```text
POST /run  {"args":{...},"cmd":"count"} → {"session_id":"14veMHiG…"}
GET  /progress/<sid>                    → started → tick×3 → log → done
```

实测末行：

```json
{"duration_ms":0,"result":{"file":"/tmp/lilyco-book/Cargo.toml","label":"","mode":"bytes","tags":[],"value":182},"type":"done"}
```

这一步证明「运行按钮按下后」的后端行为与第 3 章描述一致。

---

## MCP 三步

| 请求 | 结果 |
|---|---|
| `initialize` | `serverInfo: {"name":"lilyco","version":"0.3.0"}` |
| `tools/list` | 两条 tool：`count`、`echo`，含 `inputSchema` / `required` / `enum` / `minimum` |
| `tools/call`（合法） | `content[0].text` 为结果 JSON，`isError: false` |
| `tools/call`（坏路径） | JSON-RPC `error.code: -32602` |

---

## 视图组件协议（52 类）

| 检查 | 命令 / 方法 | 结果 |
|---|---|---|
| 协议单测 | `cargo test -p lilyco-core --offline view::` | rc=0，7 passed |
| 静态槽位 | `ch_views --schema` | `views` 含 `job`（progress）与 `summary`（table） |
| 事件流 | `ch_views views --steps 3 --json-stream` | 逐行 `{"type":"view",…}`，末行 done |
| 全量渲染 | 无头 Chromium 点击运行 `ch_all_views --gui` | slots `v00…v51`（52/52）；`data-view-kind` 覆盖 52 类 |
| 渲染闸门 | `cargo test -p lilyco-gui --offline` | rc=0，38 passed；逐条 OK：`every_css_color_is_in_the_design_table`、`assets_stay_within_the_documented_budget`、`every_js_component_init_is_bootted`、`boot_is_the_last_thing_in_the_script`、`no_data_attribute_goes_unread`、`inline_script_is_syntactically_valid`、`reduced_motion_keeps_a_non_animated_state_signal` |
| 资源预算 | 同上 | HTML 约 29 KB / CSS 约 28 KB，上限 44,000 / 40,000（DESIGN.md §9） |
| 静态槽位画壳 | `GET /?cmd=views` 的 HTML | 含 `data-view-id="job"/"summary"`、`data-view-kind-hint`、`data-view-placeholder="1"` |
| 槽位被填充 | CDP 点击运行后 | `slots` 7 个，`skeletons` 0（占位全部被原位替换） |
| 浮层关闭 | CDP 点 `[data-close]` | `{"dialogBefore":true,"dialogAfter":false}` |
| MCP 透传 | `initialize` + `tools/call(views)` 走 stdio | `isError:false`，stderr 为空；view 走进度通知 |
| TUI 降级（文本） | `cargo test -p lilyco --features tui` 的 `tui_view_falls_back_to_text` | ok；得到「视图 job: progress」 |
| TUI 降级（端到端） | 同上的 `drain_task_turns_view_into_log_line`：向 Task 发 View + Done，调用 `drain_task` | ok；`progress_log` 含降级行，状态变 `Done`（rc=0，20 passed） |
| 旧 crate 回归 | `lilyco-example --json-stream`（imgpress 真实图片） | rc=0，末行 `done` |
| 旧 crate 回归 | `lilyco-example --gui`（未开 web） | rc=1：`--gui` 不被识别，走 CLI 后因空 `--input` 报 IO 错误 |
| 旧 crate 回归 | `lilyco-dev doc --help` | rc=0，`--json-stream` / `--json` 标志仍在 |
| 旧 crate 回归 | `lilyco-dev --gui`（未开 web） | rc=2，`unexpected argument '--gui' found` |
| 旧 crate 回归 | `lilyco-dev build --help`、`lilyco-example --schema` | rc=0 |
| 教程示例回归 | `ch01 --gui`、`ch03 --json-stream`、`ch04 --gui` | HTTP 200 / rc=0 / HTTP 200 |
| 回归修复 | 上一轮给 `view_fallback_line` 加 cfg 时误吞 `drain_task` 的 `#[cfg(feature="tui")]` | 已修复；`cargo check -p lilyco-example -p lilyco-dev` rc=0 |
| tauri 定性 | `git status --short lilyco-tauri` + check `--exclude` | 0 改动；exclude 后 rc=0（失败在其自身 build script） |

### 等价覆盖与隔离说明

- **TUI 降级**：在伪终端里直接跑 `--tui` 只能截到 ratatui 的备用屏幕控制序列（无文本帧），
  因此改用端到端单测 `drain_task_turns_view_into_log_line` 等价覆盖真实路径：
  构造真实 `executor::Task`（channel 里放 `Progress::View` + `Done`）→ 调用 `drain_task` →
  断言 `app.form.progress_log` 含「视图 job: progress」且状态变 `Done`。它覆盖的是同一个
  `match` 分支，不是辅助函数。
- **`lilyco-example --gui` 基线**：该行为由 `detect_backend` 决定；本次 diff 中
  `detect_backend` / `fn detect` 的增删行数为 **0**（`git diff -U0 lilyco/src/lib.rs`
  过滤 `^[+-]` 后为空），且 `lilyco-example/Cargo.toml` 的 diff 为 0 行 —— 依赖特性未变，
  所以「未开 web 时 `--gui` 不被识别」不是本次引入。
- **lilyco-tauri**：`git diff --stat lilyco-tauri` 为 0 行；失败点是它自己的 `build-script-build`
  （Tauri 需要前端产物），`cargo check --workspace --exclude lilyco-tauri` rc=0。

### 复现路径（仓库内）

```bash
# 契约与事件流（无 web 特性也能跑）
cargo run -p lilyco-example --example views -- --schema
cargo run -p lilyco-example --example views -- views --steps 3 --json-stream

# 页面渲染（template 已开 tui + web）
cd ../lilyco-template
cargo run --example views -- --gui        # 静态槽位 + 7 个组件
cargo run --example all_views -- --gui    # 52 类全量
```

页面断言用无头 Chromium 的 CDP：导航 → 点 `#run-btn` → 读
`[...document.querySelectorAll('[data-view-kind]')]`。它与 `--dump-dom` 是同一套渲染结果，
区别只是 dump 拿不到「点击之后」的 DOM 快照，所以用 CDP `Runtime.evaluate` 取。

---

## 附录 B 的复现记录

| 条目 | 复现命令 | 结果 |
|---|---|---|
| B.1 `--gui` 互斥 | `cargo run -p lilyco-example -- --input ... --gui`（依赖未开 web） | 退出码 2，`cannot be used with...` |
| B.2 `--mcp` 不存在 | `cargo run -p lilyco-example --example multi -- --mcp` | 退出码 2，`unexpected argument '--mcp' found` |
| B.3 顶层 `--json` | `ch04 --json` | 退出码 2，`unexpected argument '--json' found` |
| B.4 路径不存在 | `ch02 --file /nope` | 退出码 2，`path does not exist: /nope` |
| B.5 range 下划线 | `range = 0..=1_000_000` 后 `--schema` | 退出码 0，但 `"max": null` |
| B.6 `Vec<u32>` | `#[derive(App)]` + `Vec<u32>` | 编译失败，`error[E0277]` |
| B.7 MCP 参数错误 | `tools/call` 传 `file: /nope` | `-32602` |
| B.8 未使用字段 | `cargo build` | `warning: field quiet is never read` |
| B.9 取消失败 | `POST /cancel/does-not-exist` | HTTP 404，body `no such running session` |

---

## 文档健康

| 检查 | 命令 | 结果 |
|---|---|---|
| 相对链接 + 锚点 | `python3 docs/ui/check-links.py` | 28 个文件、91 条 md 链接、问题 0（含锚点解析） |
| 设计闸门 | `cargo test -p lilyco-gui --offline` | 退出码 0，35 passed / 0 failed |

---

## 未验证项（如实标注）

| 项 | 为什么没验 | 已验到哪一层 |
|---|---|---|
| TUI 表单交互 | 需要真实交互终端；本环境只能走管道 | 后端探测逻辑与回退路径（管道 → CLI） |
| `--gui` 自动拉起宿主浏览器 | 无头环境无法观察桌面行为 | 已用 Chromium headless 渲染并断言 DOM；HTTP 链路已验 |
| release 产物体积数字 | 本次未做 release 构建 | 给了命令与三个杠杆，未给具体字节 |
| reduce 动效的浏览器实测 | 无法在自动化环境切换系统「减少动态效果」 | 规则级：`@media (prefers-reduced-motion)` 块存在，且有测试盯住 |
| 320px 无横向溢出 | 未做 iframe 逐档测量 | 设计文档记录的是 2026-09-23 的 16 档实测结论，本次未复跑 |

---

## 本次改动的边界

- 仓库内改动：`docs/ui/`（新建）与 `readme.md` 一行入口。
- 未修改任何 `.rs` 文件；验证用的 crate 在 `/tmp`，不属于仓库。
- `git status` 中另有 5 个 `.rs` 处于已修改状态，其 mtime 为 11-15 14:35–18:58，
  早于本次工作（11-16 22:30 之后），不是本次产生。