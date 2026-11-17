# 附录 D · 排错索引

> 按症状查。每条给「先查什么」和「一条命令」。
> 报错原文的完整解释见 [附录 B](B-errors.md)。

---

## 编译与环境

| 症状 | 先查 | 一条命令 |
|---|---|---|
| `unexpected argument '--gui'` 或参数互斥 | 依赖有没有开 `web` | `grep features Cargo.toml` |
| `unexpected argument '--mcp'` | 用的是不是 `run_cli_registry` | `grep -n "run_.*registry" src/main.rs` |
| `Vec<u32>` 编译不过 | 是否用了非 String 的列表 | 改成 `Vec<String>`，见 B.6 |
| 找不到 `target/debug/xxx` | 本机 `CARGO_TARGET_DIR` | `echo $CARGO_TARGET_DIR` |
| 改了代码但行为没变 | 进程没重启 / 旧二进制 | `cargo build && ls -l $CARGO_TARGET_DIR/debug/xxx` |

---

## 页面

| 症状 | 先查 | 一条命令 |
|---|---|---|
| 页面打不开 / 连接被拒 | 端口、进程是否活着 | `curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/` |
| 打开是空白 | 服务端渲染异常；看终端 stderr | 直接跑 `--gui` 看控制台 |
| 改了 CSS 没变化 | `GET /` 是否 `no-store`；是否硬刷新 | `curl -sI http://127.0.0.1:8080/ \| grep -i cache` |
| 点「运行」没反应 | 是不是少了 `ctx.done` | 搜 `ctx.done`；看日志区有没有「等待输出…」 |
| 日志区出现「组件装配失败」 | 某个 `initXxx` 抛错（TDZ 或 DOM 名变了） | 看 `#log` 里的红字，点名了哪个 init |
| 字段全空 / 名字对不上 | 字段名与 id 规则 | `snake_case` → `--kebab-case`；id 是 `field-<arg>` |
| 输入框比 40px 高 | `line-height` 被继承 | 检查 `input { line-height: 1.4 }` 是否还在 |
| 窄屏横向溢出 | 有没有 `min-width: 0` | 在 360px iframe 里看 `scrollWidth > clientWidth` |
| 按钮组在竖排时变很高 | `flex-basis` 在 column 下变成了高度 | 竖排分支改 `flex: 0 0 auto` |
| 深色下下拉箭头/滚动条是白的 | 少 `color-scheme` | 搜 `color-scheme: dark` |
| 暗色下卡片还有阴影 | `--shadow-card` 没置 `none` | 看 `[data-theme="dark"]` 块 |
| 对比度不够 | 是否把 hairline 当控件边用 | 控件边应是 `--control`，不是 `--hairline` |
| 关掉动效后看不出在跑 | reduce 的静态替代被删 | 搜 `prefers-reduced-motion` 的进度条与按钮规则 |
| 页面里出现外链图标/字体 | 违反零依赖 | 浏览器控制台看 CSP 拒绝 |

---

## 数据与参数

| 症状 | 先查 | 一条命令 |
|---|---|---|
| 数字上限提示不见了 | `range` 里有没有下划线 | `cargo run -- --schema \| grep -A4 '"max"'` |
| 必填项其实可以不填 | 字段有没有 `default` / 是否 `Option` | 看 schema 的 `required` |
| Web 提交数字参数报「需要数字」 | 列表 item 类型 / 宏取值分支 | 见 B.6 |
| `must_exist` 没生效 | 属性写没写、类型是不是 PathBuf | `cargo run -- --schema \| grep must_exist` |
| CLI 有值、页面没值 | 字段名转 kebab 后不一致 | 对比 schema 的 `name` 与 HTML 的 `id` |

---

## 进度与取消

| 症状 | 先查 | 一条命令 |
|---|---|---|
| 进度条一直动、永远不结束 | 少了 `done` / `error` 收尾 | `cargo run -- --json-stream` 看最后一行 |
| 进度条不动 | `total` 是否为 `None`（不确定态） | 看事件里有没有 `percent` |
| 取消按钮不出现 | 执行路是不是自定义 `RunnerFn` | 改用 `run` / `run_registry` |
| 取消后进程不退 | 检查点太少 / 在不可中断区 | 在循环里加 `ctx.is_cancelled()` |
| 页面「取消失败：no such running session」 | 同上 | 见 B.9 |

---

## MCP / agent

| 症状 | 先查 | 一条命令 |
|---|---|---|
| 客户端显示连接失败 | 进程能不能握手 | `printf '...' \| ./bin --mcp` |
| `tools/list` 里没有某命令 | 是否 `hidden`；是否单命令 | 检查 `#[app(name)]` 与注册 |
| 调用返回 `-32602` | 参数不合法（路径不存在等） | 用 CLI 同样参数先跑一次 |
| 调用返回「安全门」 | 命令声明了 T1+，MCP 面被拒 | 看 `#[app(safety = ...)]` |
| agent 拿到的结果难解析 | 返回了裸值 | 返回带字段名的 JSON 对象 |
| 工具名不是想要的 | 默认取 struct 名 | 加 `#[app(name = "xxx")]` |

---

## 多命令

| 症状 | 先查 | 一条命令 |
|---|---|---|
| 顶栏没有命令下拉 | 可见命令只有 1 个 | `cargo run -- --schema \| grep '"name"'` |
| `--json` 不识别 | 挂在了顶层 | `filecount count --file x --json` |
| 子命令顺序每次不同 | 注册表是映射结构 | 不要依赖顺序，用名字调用 |
| 别名不生效 | `.alias()` 加没加在注册时 | `grep alias src/main.rs` |

---

## 发布

| 症状 | 先查 | 一条命令 |
|---|---|---|
| release 体积没变小 | profile 写在了依赖仓库而不是自己项目 | 检查自己 `Cargo.toml` 的 `[profile.release]` |
| `cargo publish` 拒绝 | 还有 `path` 依赖 | `grep path Cargo.toml` |
| Android 编译失败 | 开了 `tui`（crossterm 不支持） | 依赖写法去掉 `tui`/`web` |

---

## 还是不行

按这个顺序收集信息，再提问：

1. `cargo run -- --schema` 的完整输出；
2. `cargo run -- --json-stream` 的最后一行；
3. 页面 `#log` 里的红字（如果有）；
4. `cargo --version` 与依赖的 `features` 行。

这四样能覆盖绝大多数「说不清哪里不对」的情况。