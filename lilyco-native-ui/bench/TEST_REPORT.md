# Lilyco UI 双后端测试完成报告

**报告日期**：2026-11-15（Asia/Shanghai）  
**被测对象**：同一份 `UiSpec` JSON 契约驱动的 React/CDN Web 后端与 NSH/GTK4 原生后端  
**测试目标**：验证「类型即组件」契约在两条渲染链路上的功能覆盖、异常韧性、交互状态、启动/资源性能与稳定性  
**结论状态**：✅ 通过（功能/契约/异常/稳定性/性能采集 + 2 项缺陷已修复并回归）；⚠️ 余 3 项限制与未覆盖范围，详见第 8 节  
**最后一次复测**：2026-11-15 14:4x（Asia/Shanghai），缺陷修复后的全量重跑

---

## 1. 测试范围与产物

### 1.1 被测组件

| 层 | 路径/入口 | 职责 |
|---|---|---|
| 契约层 | `lilyco-ultra-ui/src/spec.rs` | `UiSpec` / `#[serde(tag="类型")] ElementSpec` 13 变体；`validate()` |
| Web 生成层 | `lilyco-ultra-ui/src/generator.rs` | 同一份 `UiSpec` → React HTML；固定 CDN + SRI + classic JSX runtime |
| Web 服务层 | `lilyco-ultra-ui/src/lib.rs` | `/`、`/api/render`、`/api/validate`、`/api/example`、`/api/example/calculator` |
| 原生渲染层 | `lilyco-native-ui/src/render.rs` | 穷尽 `match ElementSpec` → NSH/GTK4 原生控件树 |
| 原生状态层 | `lilyco-native-ui/src/lib.rs` | `Signal<BTreeMap>` + `Signal<Option<String>>`，局部信号更新 |
| 原生入口 | `lilyco-native-ui/src/main.rs` | 读取 UiSpec JSON、校验、注册 registry、启动 `AppSource::Rust` |

### 1.2 测试夹具与脚本

| 类型 | 文件 |
|---|---|
| 全变体契约 | `bench/fixtures/all-13-spec.json`（13 个元素 + 标题栏 = 14 项断言） |
| 修复版 Web 页 | `bench/fixtures/all-13-react-fixed.html`（9,134 B，sha256 `dff144b4…13d`） |
| 离线内联页 | `bench/fixtures/.generated/all-13-react-offline.html`（3,220,778 B，sha256 `04a7fd8c…2dc`） |
| 旧版白屏对照 | `bench/fixtures/all-13-react-old-unpinned.html`（8,718 B，sha256 `9bcd6a4a…677`） |
| Web 全变体矩阵 | `bench/cdp_matrix.js` + `bench/web_matrix.py` |
| 原生全变体矩阵 | `bench/native_matrix.py` + `bench/atspi_tree.py` |
| 契约/异常矩阵 | `bench/contract_matrix.py` |
| 启动/交互性能 | `bench/native_bench.py`、`bench/interaction_bench.py`、`bench/web_bench.py` |

### 1.3 原始结果

| 文件 | 内容 |
|---|---|
| `bench/results/contract-all13-2026-11-15.json` | 14 个契约/异常用例 |
| `bench/results/web-all13-2026-11-15.json` | Web 全变体 CDP 运行（四组，最新批次：3+3+3+1） |
| `bench/results/native-all13-2026-11-15.json` | 原生 14 项功能矩阵 + 3 组状态交互 + 截图/日志证据 |
| `bench/results/native-all13-startup-2026-11-15.json` | 原生全变体启动/资源 5 轮 |
| `bench/results/native-all13-interaction-2026-11-15.json` | 原生全变体「执行」按钮交互 5 轮 |
| `bench/results/native-all13-2026-11-15.png` | 原生全变体窗口截图证据 |
| `bench/results/web-2026-11-14.json`、`native-startup-2026-11-14.json`、`native-interaction-2026-11-14.json` | 上一轮 10 元素基准，作为纵向参考 |

---

## 2. 测试环境

| 项 | 值 |
|---|---|
| OS | CachyOS / Linux；DISPLAY=:0，WAYLAND_DISPLAY=wayland-1 |
| GTK | GTK4 4.24.1，Pango 1.58.2；GDK_BACKEND=x11 |
| Python | 3.14.10（PIL、requests 可用） |
| Node | v26.13.0（全局 WebSocket） |
| 浏览器 | Chrome/Chromium 157.0.8103.38，`--headless=new --no-sandbox --disable-gpu` |
| Rust/Cargo | 1.99.1；release profile `opt-level="z" / lto="fat" / panic="abort"` |
| NSH | 0.5.1；`nsh-platform-linux` 关闭 `js/webview`，纯 Rust + GTK4 |
| 原生二进制 | 36,382,288 B；`ldd` = `NO_WEBKIT_NO_QUICKJS_NO_MISSING` |
| 动态依赖 | GTK4 / Pango / GStreamer / Vulkan；**无 WebKit6、无 QuickJS、无缺失 so** |
| 测量方法 | Web：CDP `Page.navigate` → `#root .el` / MutationObserver；原生：xprop 窗口映射 + NSH `first frame` / AT-SPI `nsh.activate` |

**口径限制**：Web 的冷启动 = 浏览器进程 CDP 就绪 + 首屏渲染；原生 = 进程启动 → X11 窗口出现 + NSH 内部首帧。两者不是同一事件边界，不能直接换算「快 N 倍」；内存口径同样不同（Chrome 进程树 VmRSS 总和 vs 单原生进程 VmRSS）。

---

## 3. 单元测试 / 构建结果

```text
# 2026-11-15 01:39 首轮
BUILD_ALL_EXIT=0
cargo test -p lilyco-ultra-ui: 3 generator tests + 12 spec tests PASS
lilyco-native-ui cargo test: 1 arithmetic + 4 coverage tests PASS
cargo build --release: Finished release profile in 6m 19s
ldd: NO_WEBKIT_NO_QUICKJS_NO_MISSING

# 2026-11-15 14:4x 缺陷修复后复测
MAIN_TEST_EXIT=0   -> 4 generator tests + 12 spec tests PASS（新增 window_size_maps_to_css_class）
NATIVE_TEST_EXIT=0 -> 2 lib tests + 4 coverage tests PASS（新增 image_width_parses_px_and_falls_back_to_full）
NATIVE_RELEASE_EXIT=0 -> Finished release profile in 2m 57s
CONTRACT_EXIT=0    -> 14/14 PASS（render_window_class_defect 升级为 render_window_class_mapped）
合计单元测试 22/22 PASS
```

覆盖点：

- `spec_tests.rs`：解析、默认值、窗口尺寸 class、JSON roundtrip、空标题/空元素/重复变量/空 Select/非法 JSON/缺字段。
- `generator_tests.rs`：三条固定 CDN + 3 个 SRI、显式 classic runtime、计算器 case 不再漂移。
- `coverage.rs`：原生 `COVERED` 与 13 类型一致；默认示例与计算器示例同时喂给两个后端；JSON roundtrip。
- `render::tests::arithmetic`：`1+2*3=7`、`(1+2)*3=9`、`10/4=2.5`、`1+ → 表达式无效`。
- `render::tests::image_width_parses_px_and_falls_back_to_full`：`48px→Some(48.0)`、` 48px →Some(48.0)`、`100%→None`、`""→None`。
- `generator_tests::window_size_maps_to_css_class`：断言 `SIZE[w.大小]`、`大: 'large'`、`小: 'small'`、`中等: 'medium'`、`全屏: 'fullscreen'`，并断言不再出现 `'window ' + w.大小`。

### 3.1 本轮修复回归（2026-11-15 14:4x）

| 缺陷 | 修复前 | 修复后 | 回归证据 |
|---|---|---|---|
| Web 窗口尺寸 class 用中文 | `'window ' + w.大小` → 实测 `"window 大"`，`.window.large` 不命中 | 生成器注入 `const SIZE = { 小:'small', 中等:'medium', 大:'large', 全屏:'fullscreen' }`，输出 `'window ' + (SIZE[w.大小] ‖ 'medium')` | CDP 实测 `windowClasses = ["window large"]`（fixed_online/offline 各 3 轮）；契约用例改名 `render_window_class_mapped` 并 PASS |
| 原生图片把「宽度」当高度 | `.w_full()` + `.h(px(48))` | `image_width()` 纯函数；`48px` → `.w(px(48.0))`，`100%`/非像素 → `.w_full()` | `image_width_*` 单测；原生矩阵仍 14/14；新截图 117,699 B / 868 色 |

---

## 4. 契约与异常矩阵（14/14 PASS）

服务入口：`http://127.0.0.1:9090`，测试脚本 `bench/contract_matrix.py`。

| # | 用例 | 期望 | 结果 |
|---:|---|---|---|
| 1 | `/api/example` | 默认 10 元素合法示例 | PASS |
| 2 | `/api/example/calculator` | 计算器示例路由已挂载 | PASS |
| 3 | `validate(all-13)` | `valid=true, errors=[]` | PASS |
| 4 | `render(all-13)` | 3 条固定 SRI、classic preset、13 个变体 case | PASS |
| 5 | 窗口 class 映射 | 生成器含 `SIZE[w.大小]`、`大: 'large'`，且不再出现 `'window ' + w.大小` | PASS（原缺陷探针已升级为回归断言） |
| 6 | 非法 JSON → `validate` | HTTP 400，axum 拒绝请求体 | PASS |
| 7 | 非法 JSON → `render` | HTTP 400，不进入生成器 | PASS |
| 8 | 空标题 | `标题不能为空` | PASS |
| 9 | 空元素列表 | `窗口元素列表为空` | PASS |
| 10 | 重复变量名 | `重复` | PASS |
| 11 | 空 Select 选项 | `选项列表不能为空` | PASS |
| 12 | Heading level 9 | `标题级别必须在 1~6` | PASS |
| 13 | 未知 serde tag | 契约层拒绝，列出 13 个合法变体 | PASS |
| 14 | 校验失败 → `render` | HTTP 400，不生成 HTML | PASS |

> 发现并修正测试脚本自身的误判：非法 JSON 由 axum JSON extractor 直接返回 `text/plain` 400，不是 `validate_handler` 的 JSON 业务错误；报告与脚本按真实响应记录。

---

## 5. 全变体功能矩阵

### 5.1 Web（React/CDN，14/14 PASS × 3 轮）

夹具 `all-13-spec.json` 经修复后的 `/api/render` 生成页面，在 headless Chrome CDP 中逐项断言 DOM。

| 变体/能力 | 断言要点 | 结果 |
|---|---|---|
| 标题栏 | `.wtitle = Lilyco 原生全变体烟测` | PASS |
| 标题 | `H1.el-heading.h1 = 十三变体` | PASS |
| 文本 | `P.el-text` 含「纯 Rust」 | PASS |
| 输入框 | `type=text, value=abc, placeholder=输入` | PASS |
| 数字框 | `type=number, value=7, min=0, max=100` | PASS |
| 选择框 | `png/webp`，默认 `png` | PASS |
| 复选框 | `checked=true` | PASS |
| 文本域 | `hello`，`rows=3` | PASS |
| 图片 | `document-share.png`，`width=48px` | PASS |
| 分隔线 | `HR.el-divider` | PASS |
| 进度条 | `.el-pb style.width = 42%` | PASS |
| 链接 | `https://nsh.dev`，文本「NSH 官网」 | PASS |
| 计算器 | `.el-calc` 含 `scientific` | PASS |
| 按钮 | `执行`、`el-btn primary` | PASS |

**交互状态**：点击 `执行` 后 toast 出现，3/3 轮成功；用 React 原生 setter 将首个输入框改为 `matrix-ok` 并触发 `input/change`，再次点击按钮，toast 携带 `matrix-ok`，3/3 轮成功。  
**窗口尺寸（本轮已修复）**：修复前生成 `"window 大"`，CSS 只定义 `.window.large` 等英文 class；修复后 CDP 实测 `windowClasses = ["window large"]`，3 轮一致。

### 5.2 原生（NSH/GTK4，14/14 PASS）

通过 AT-SPI 树 `atspi_tree.py dump`（55 节点）+ `import -window` 截图验证。截图：1920×1684，117,699 B，868 种颜色，说明窗口不是空白；归档为 `bench/results/native-all13-2026-11-15.png`。

| 变体/能力 | AT-SPI / 日志证据 | 结果 |
|---|---|---|
| 标题栏 | frame `Lilyco 原生全变体烟测` | PASS |
| 标题 | label `十三变体` | PASS |
| 文本 | label `纯 Rust / NSH GTK 原生视图` | PASS |
| 输入框 | label `名称` + 空 panel | PASS（见第 8 节第 1 项） |
| 数字框 | label `数量` + 空 panel | PASS（见第 8 节第 1 项） |
| 选择框 | combo box `nsh.activate` + label `png` | PASS |
| 复选框 | check box `启用` + `nsh.activate` | PASS |
| 文本域 | role `text`，text=`hello`，editable=true | PASS |
| 图片/分隔线 | 两个空 panel | PASS（见第 8 节第 1 项） |
| 进度条 | progress bar value=42.0 + label `压缩进度` | PASS |
| 链接 | button `NSH 官网` + `nsh.activate` | PASS |
| 计算器 | label `计算器 · 科学` + button `计算` | PASS |
| 按钮 | button `执行` + `nsh.activate` | PASS |
| 状态栏初始 | `状态: （空）` | PASS |

**交互状态**（全部 PASS）：

1. AT-SPI 向可编辑文本域写入 `matrix-ok`，激活 `计算`：状态栏变成 `v_calc=表达式无效, v_textarea=matrix-ok`，结果标签 `结果: 表达式无效`。
2. 激活复选框 `启用`：状态栏增加 `v_check=false`，证明 `on_change → values` 写回。
3. 激活按钮 `执行`：notice 标签变为 `动作 run 已触发 · v_calc=表达式无效, v_check=false, v_textarea=matrix-ok`，证明 `on_press → notice` 与状态快照同时工作。

日志统计：`perf_events=8`、`press_events=3`、`change_events=1`、`LILYCO_NATIVE_PRESS … run`、`Gtk-WARNING=4`、`CRITICAL=0`、`panic=0`。4 条 warning 为已知 GTK 主题噪声，不影响状态更新。修复后在最新 release 上重跑，55 节点、14/14、三段交互仍全部 PASS。

---

## 6. 性能对比（同一全变体夹具）

> 样本：Web `fixed_online` / `offline_vendored` / `cdn_blocked` 各 3 轮，`old_unpinned` 1 轮；原生 release 启动 5 轮、交互 5 轮。表格数字为 **median（mean）**，完整 n / min / max / mean 见 `bench/results/*.json`。

### 6.1 全变体同批次（2026-11-15 14:4x，缺陷修复后复测）

| 指标 | Web fixed_online | Web offline_vendored | NSH/GTK4 原生 | 口径说明 |
|---|---:|---:|---:|---|
| 渲染成功 | 3/3 | 3/3 | 1/1 矩阵 + 5/5 基准 | 原生矩阵单窗口，启动/交互各 5 轮 |
| 浏览器 CDP 就绪 / X11 窗口出现 | 225.62 (253.21) ms | 242.50 (270.00) ms | 343.71 (369.48) ms | 不同事件边界 |
| 首屏 DOM / NSH 内部首帧 | 6553 (6145) ms | 521 (553.67) ms | 69.17 (70.27) ms | 不同事件边界 |
| 冷启动（脚本口径） | 6906.28 (6398.21) ms | 853.97 (823.67) ms | 窗口出现 + 首帧约 412.88 ms（仅量级参照） | Web = browserReady + firstRender；原生两项相加**不等价** |
| 进程树 / 单进程 RSS | 1108.30 (1107.67) MB | 1112.87 (1113.91) MB | 169.53 (169.61) MB | Chrome 全进程树 VmRSS 总和 vs 原生单进程 VmRSS |
| NSH FPS monitor RSS | — | — | 177.65 (177.51) MB | NSH `nsh::fps` 自带口径 |
| JS heap | 8.45 (9.04) MB | 8.51 (8.54) MB | N/A | CDP `Performance.getMetrics` |
| 稳态 CPU | 未采集 | 未采集 | 2.5% (2.55%) | NSH `nsh::fps` 日志 |
| 空闲 turn | — | — | 0.44 (8.14) ms | NSH `nsh::perf` turn |
| 点击/激活延迟 | 13.60 (14.03) ms | 11.50 (14.20) ms | press_handler 0.10 (0.11) ms；state_commit 2.09 (1.91) ms | 见 6.3，边界不同 |
| AT-SPI `do_action` | — | — | 89 (102.6) µs | 仅原生激活调用开销 |
| 外部请求 | 3 | 0 | 0 | `unpkg.com` 请求数 |
| 窗口尺寸 class | `window large` | `window large` | — | CDP 实测，验证缺陷 1 已修 |
| 异常 / CRITICAL / panic | 0 / — / — | 0 / — / — | 0 / 0 / 0 | CDP `Runtime.exceptionThrown` / GTK 日志 |

**内存量级**：同批次 Chrome 进程树约 1.11 GB，原生单进程约 170 MB，约 **6.5×** 差距；这是「浏览器多进程树」与「单原生进程」的口径差异，不是同一分配器下的严格等价比较。

**网络与波动**：online 与 offline 的差距在同一批次内是 6906 vs 854 ms（≈6 000 ms），但**线上 CDN 抖动极大**——同一份夹具上一批次 online 冷启动中位数仅 1163 ms、再上一批 10 元素夹具为 4964 ms。三批数据说明：Web 链路的启动成本取决于 CDN 往返与 Babel 编译时机，而原生链路（窗口 ≈344 ms + 首帧 ≈69 ms）没有这一项。因此：

- 可复现的结论是「Web 多一条外部依赖链、多一个 JS/Babel 运行时、内存高一个量级」，**不是**某个固定倍数的提速承诺；
- **禁止跨批次计算加速比**，也禁止把 online 的 6906 ms 与原生 413 ms 直接相除。
### 6.2 10 元素纵向参考（**旧批次：2026-11-14 归档，勿与 6.1 混算**）

| 后端/场景 | 冷启动 median (mean) | 首屏/首帧 | 点击/状态提交 | RSS | 稳定性 |
|---|---:|---:|---:|---:|---|
| Web fixed_online | 4964.69 (4261.55) ms | nav_to_render 4766 (3853.67) ms | 13.10 (13.57) ms | 1114.61 (1114.43) MB | 3/3 rendered，0 异常，3 外部请求 |
| Web offline_vendored | 650.93 (640.63) ms | nav_to_render 415 (426.67) ms | 13.80 ms | 1113.31 MB | 3/3 rendered，0 异常，0 外部请求 |
| Web old_unpinned | 不可用 | 白屏 | 不可用 | 969.60 MB | 0/1 rendered，1 异常，6 外部请求 |
| Web cdn_blocked | 不可用 | 白屏 | 不可用 | — | 0/3 rendered，3 异常，3 网络失败 |
| 原生启动 | 窗口出现 245.78 (276.22) ms，n=4 | first_frame 32.18 (39.71) ms | turn 0.79 (0.82) ms | 175.92 (176.03) MB；FPS monitor 184.40 MB | 0 crash / 0 critical，CPU 0% (0.26%) |
| 原生交互 | — | — | press_handler 0.16 ms；event_to_turn 2.00 ms；state_commit 2.15 ms；do_action 129 µs | — | 5/5 press / event / accepted，0 crash / 0 critical |

### 6.3 交互延迟的测量边界

- **Web**：CDP 内 `button.click()` → `MutationObserver` 等 `.el-toast` 出现，包含 React state 更新与 DOM commit。
- **原生**：AT-SPI `nsh.activate` → Rust `on_press` → `LILYCO_NATIVE_PRESS` 打点 → NSH `event Press` / `commit` / `turn`；`press_handler` 是注入端到 Rust handler 的墙钟差，`state_commit` 是 event 到 commit/turn 的日志时差。
- XWayland 下 XTest 键鼠事件不投递 GTK 子控件，已弃用；这里不是物理键盘/鼠标端到端延迟。
- 因此点击/激活两列只用于各自链路内部的稳定性观察，**不宣称原生交互比 Web 快 N 倍**，也不宣称替换 `lycomc` 的 `wry` 壳会得到同样数字。

---

## 7. 稳定性 / 异常韧性对比（2026-11-15 14:4x 缺陷修复后批次）

| 场景 | 修复版 React online | React offline vendored | 旧版 unpinned CDN | CDN 屏蔽 | NSH/GTK4 原生 |
|---|---:|---:|---:|---:|---:|
| 全变体渲染成功 | **3/3** | **3/3** | **0/1** | **0/3** | **1/1 矩阵 + 5/5 启动/交互** |
| 14 项矩阵通过 | 14/14 × 3 | 14/14 × 3 | 0/14 | 0/14 | 14/14 |
| JS/Rust 异常 | 0 | 0 | 1 | 3 | 0 |
| 外部网络请求 | 3 | 0 | 6 | 3 | **0** |
| 网络失败 | 0 | 0 | 0 | 3 | **0** |
| 状态交互 | 3/3 | 3/3 | 不可用 | 不可用 | 3/3 场景 + 5/5 按钮事件 |
| CRITICAL / panic | — | — | — | — | **0 / 0** |

**两组稳定性硬证据**：

- 旧版未锁 CDN：`SyntaxError: Failed to execute 'appendChild' on 'Node': Cannot use import statement outside a module`。根因是 `@babel/standalone` 浮到 8.1.2 后默认 automatic runtime，生成 `import … from "react/jsx-runtime"`，普通浏览器 script 白屏。
- CDN 屏蔽：`ReferenceError: Babel is not defined`，3/3 白屏。原生后端零外部请求，不受断网/CDN 漂移影响。

修复版生产生成器已固定 React 18.3.1 / ReactDOM 18.3.1 / Babel 7.28.4，三条 `sha384` SRI，并显式注册 `lilyco-react` classic preset；本次 3+3 轮 online/offline 全部 0 异常。

---

## 8. 已知限制、缺陷与风险

> 本轮（2026-11-15 14:4x）已修复 2 项缺陷并补回归测试（见 3.1）；余下 3 项是限制与未覆盖范围，不是待修缺陷。

### 8.1 已修复（2026-11-15 14:4x，含回归测试）

| # | 原缺陷 | 修复 | 回归证据 |
|---:|---|---|---|
| 1 | Web 窗口尺寸 class 用中文字段（`"window 大"`，CSS 只定义 `.window.large`） | 生成器注入 `SIZE` 映射，输出 `'window ' + (SIZE[w.大小] || 'medium')` | `window_size_maps_to_css_class` 单测；契约用例 `render_window_class_mapped`；CDP 实测 `windowClasses = ["window large"]` |
| 2 | 原生图片把契约的「宽度」当高度（`.w_full()` + `.h(px(48))`） | 新增 `image_width()`：像素值 → `.w(px(w))`，其余 → `.w_full()` | `image_width_parses_px_and_falls_back_to_full` 单测；原生矩阵仍 14/14 |

### 8.2 余下限制与未覆盖范围

1. **AT-SPI 可观测性限制**：输入框/数字框/图片/分隔线在 AT-SPI 树里只暴露为空 panel，计算器表达式框暴露为非 editable label。因此自动化用「label + panel/截图」证明存在，不能等同于逐控件物理键盘输入；XWayland 下 XTest 键鼠事件不投递 GTK 子控件，故交互改用 NSH 官方 AT-SPI `nsh.activate`。有效四则运算由 Rust 单测覆盖。
2. **口径不等价**：Web 与原生启动/内存/交互数字的采集边界不同；本报告只给各自可复现数值与量级差异，不宣称「替换 lycomc 的 wry 壳会快 N 倍」。
3. **未覆盖范围**：未做 lycomc/wry 桌面壳 A/B、移动端/Windows/macOS 平台、长时间 soak、真实物理键鼠全链路、AI 生成 JSON Schema 校验、`Patch{id,node}` 增量更新协议。

---

## 9. 复现步骤

```bash
# 0) 依赖/环境
cd /home/lilyco/code/lilyco/lilyco-native-ui
cargo build --release
cd bench
python3 fetch_vendor.py             # 固定版本 CDN 脚本，哈希钉死
python3 make_all13_fixtures.py      # 生成 offline / old-unpinned 对照页

# 1) 单元测试
cd /home/lilyco/code/lilyco
cargo test -p lilyco-ultra-ui
cd lilyco-native-ui && cargo test

# 2) 契约/异常矩阵
BROWSER=/bin/true /home/lilyco/.cache/lilyco-build/target/debug/lilyco-ultra-ui-example &
python3 bench/contract_matrix.py bench/results/contract-all13-2026-11-15.json

# 3) Web 全变体矩阵（四组，含断网/旧 CDN）
MATRIX_RUNS=3 python3 bench/web_matrix.py

# 4) 原生全变体功能 + 状态矩阵
MATRIX_OUT=bench/results/native-all13-2026-11-15.json python3 bench/native_matrix.py

# 5) 性能：原生全变体启动与交互
python3 bench/native_bench.py /home/lilyco/.cache/lilyco-build/target/release/lilyco-native-ui \
  bench/fixtures/all-13-spec.json /tmp/bench-native-all13.json
BENCH_SPEC=bench/fixtures/all-13-spec.json BENCH_TARGET=执行 \
  BENCH_ARCHIVE=bench/results/native-all13-interaction-2026-11-15.json \
  python3 bench/interaction_bench.py
```

> 原生矩阵依赖 `DISPLAY=:0` 与 AT-SPI；XWayland 下请勿用 XTest 键鼠事件代替 `atspi_activate.py`。

---

## 10. 总评

- **功能**：同一份 `UiSpec` 的 13 个变体在 React 与 NSH/GTK4 两端全部渲染；Web 14/14 × 3、原生 14/14；契约 14/14；单元测试 22/22（含 2 项本轮新增回归）。
- **状态**：React 输入→toast 状态传递 3/3；原生 `values/notice` 信号经文本域、复选框、按钮三段交互全部写回，日志有真实 `Press` / `ChangeText` 事件。
- **稳定性**：修复版 Web 在线/离线 6/6 无异常；旧版 CDN 与断网分别因 Babel runtime / 缺 Babel 白屏；原生零网络、0 CRITICAL、0 panic。
- **性能**：Web 与原生启动/资源口径不同，但原生不依赖浏览器进程与 JS/Babel 运行时，RSS 与冷启动路径显著更短；这是架构证据与实测趋势，不是对 lycomc 替换收益的承诺。
- **缺陷**：第 8 节原列的窗口尺寸 class 与原生图片宽度两项缺陷**已修复并回归**（3.1）；余下 3 项属可观测性限制、口径差异与未覆盖范围。
- **放行建议**：**可直接推进「lycomc 可开关双后端 PoC」**——本轮无阻塞缺陷；PoC 期间继续沿用本报告的采集脚本与口径说明。
