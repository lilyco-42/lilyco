# lilyco-native-ui

把 **同一份 `UiSpec` JSON** 渲染成 **NSH / GTK4 原生视图**的垂直切片，用来替换
`lilyco-ultra-ui` 的 React + WebView 整页重建路径。

## 为什么独立成一个 workspace

`nsh-base 0.5.1` 要 `unicode-width ^0.2.2`，主 workspace 里 `ratatui 0.29` 钉死
`=0.2.0`，Cargo 不允许同一 minor 区间共存两版。因此根目录 `Cargo.toml` 用
`exclude = ["lilyco-native-ui"]` 把它移出主依赖图，这里自带 `Cargo.lock`。

## 跑起来

```bash
cd lilyco-native-ui
cargo run                          # 用 default_example_json()（图片压缩工具）
cargo run -- ./my-spec.json        # 用自定义 UiSpec JSON
cargo test                         # 契约覆盖 + 计算器四则运算
```

也可直接跑构建产物：
`timeout 8 env GDK_BACKEND=x11 /home/lilyco/.cache/lilyco-build/target/debug/lilyco-native-ui ./my-spec.json`
（`124` = 窗口一直活到超时，符合预期。）

## 三层结构（类型即契约）

1. **契约层** `../lilyco-ultra-ui/src/spec.rs` — `#[serde(tag = "类型")] enum ElementSpec`
   的 13 个变体是唯一数据源；`UiSpec::validate()` 做语义校验。
2. **状态层** `src/lib.rs` — `Signal<BTreeMap<String, String>>` 存表单值、
   `Signal<Option<String>>` 存提示。改一个输入框只触发读它的文本节点更新，
   不重跑组件、不重建整页。
3. **渲染层** `src/render.rs` — `match ElementSpec` 穷尽 13 变体到 `AnyElement`。
   往 `ElementSpec` 加变体，这里会直接编译报错，杜绝漂移。

`tests/coverage.rs` 把 `COVERED` 与 `type_name()` 全量对账，并验证同一份 spec
同时喂得动 React 与原生两个后端。

## 依赖面（这是「提速与稳定」的落点）

`ldd` 无 `libwebkitgtk-6.0.so.4`、无 `libjavascriptcoregtk-6.0.so.1`、无 `not found`；
只链 `libgtk-4` / `pango` / `gstreamer` / `vulkan`。
Cargo.toml 里 `nsh-platform-linux` 用 `default-features = false`，
不启用 `js` / `webview`，所以不链 QuickJS、不链 WebKit。

相比旧路径，架构上去掉了：浏览器进程、JS/Babel 运行时、iframe `srcdoc` 整页替换；
改为信号驱动的局部绑定。端到端 A/B 数字见 `bench/results/` 与下一节；
结论只适用于本机、同一份 `bench/fixtures/bench-spec.json`、Chrome headless 与 GTK4/XWayland 环境。
## A/B 基准（2026-11-14，release 二进制）

环境：CachyOS / rustc 1.99.1 / GTK 4.24.1 / Pango 1.58.2 / glibc 2.44 /
Chrome 157.0.8103.38 / X11 on Wayland (`GDK_BACKEND=x11`)。原生用
`cargo build --release`，Web 用 `chromium --headless=new` + CDP；同一份 10 元素
`UiSpec`（图片压缩工具）分别生成 `bench-react-fixed.html` 与原生窗口。

| 指标 | React/CDN 固定版 | React 离线内联 | NSH/GTK4 原生 |
| --- | ---: | ---: | ---: |
| 启动到首次渲染（中位数，口径见下） | 4964.69 ms | 650.93 ms | 276.22 ms 窗口出现 + 32.18 ms 首帧 |
| 常驻内存（中位数） | 1114.61 MB Chrome 进程树 | 1113.31 MB | 175.92 MB 进程 / 184.40 MB FPS 监视值 |
| 稳态 CPU（中位数） | 未纳入同口径 | 未纳入同口径 | 0%（日志采样均值 0.26%） |
| 交互到状态提交（中位数） | 13.10 ms click→toast | 13.80 ms | 2.15 ms AT-SPI→on_press→turn |
| 外部网络请求 | 3 | 0 | 0 |
| 断网可用性 | rendered 0/3 | rendered 3/3 | 不受影响 |

启动指标也不是严格同口径：Web 是 Chrome headless CDP 的 `browser_ready + firstRenderMs`；
原生是 `xprop` 观察到窗口映射的时间加 NSH 首帧内部耗时。本轮没有测 `lycomc` 的
`wry`/WebView 桌面壳，因此不能直接据此宣称替换 `lycomc` 后的具体加速倍数；
这里比较的是同一份 `UiSpec` 的两条渲染链路。

交互口径不完全等价：Web 是 CDP `button.click()` 到 `.el-toast` MutationObserver；
原生是 AT-SPI `nsh.activate` 到 Rust `on_press` + `nsh::perf` commit/turn。
XWayland 下 XTest 合成按钮事件不投递到 GTK 子窗口，因此用 NSH 官方 AT-SPI 通路；
这不是真实鼠标事件到物理呈现的端到端延迟，不能当成同口径逐毫秒比较。

稳定性硬证据：未锁版本的旧 React 页 `rendered 0/1`，异常 1；Babel 8.1.2 默认
automatic runtime 生成 `import ... from "react/jsx-runtime"` 导致白屏。生产修复把
React/react-dom 锁到 18.3.1、Babel 锁到 7.28.4，加 SRI 与显式 classic runtime 后
`rendered 3/3`、异常 0。原生 5 轮交互全部 accepted、5 个业务 press、0 crash、
0 critical。

原始数字与逐轮明细：

- `bench/results/web-2026-11-14.json`
- `bench/results/native-startup-2026-11-14.json`
- `bench/results/native-interaction-2026-11-14.json`

复现脚本在 `bench/`：`make_fixtures.py` 生成的离线页面与实测 fixture 逐字节一致
（sha256 `e83dfeb2bb4584da8e11fc59b9cf6263beeee45fd601a02034281ac88bdee545`）、
`web_bench.py`（Web 四档）、`native_bench.py`（启动/RSS/CPU）、
`interaction_bench.py` + `atspi_activate.py`（原生交互）。需要 `chromium`、`node`、
`libatspi.so.0`、`X11/XTest` 开发库；`x11bench.c` 用 `gcc -O2 -lX11 -lXtst` 编译。

## 顺带修掉的漂移

- `lilyco-ultra-ui/src/generator.rs`：React 后端补 `case '计算器'`（原来落到
  `default: 未知元素类型`），加 `.el-calc` 样式。
- `lilyco-ultra-ui/src/lib.rs`：激活原本没挂路由的 `/api/example/calculator`。

## 已知噪声（不影响功能）

含「文本域 / 计算器」等长内容时，GTK4 主题会打印
`GtkGizmo (slider) reported min width -2`。它来自 multiline input 内部的
GtkScrolledWindow，是上游主题层的布局 WARNING：无 `critical`、无 panic、
窗口稳定存活。应用侧已对 `scroll()` 与 multiline `input()` 都设
`.scrollbar(false)` 表达意图。
