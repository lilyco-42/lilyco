# 经典游戏逐帧压测报告 — Conway's Game of Life

> 批次：2026-11-15 16:05 +08:00（同一主机、顺序执行、3 轮/后端）  
> 被测对象：lilyco 双后端——**NSH 原生 GTK**（`lilyco-native-ui`）与 **React/Web 生成器**（`lilyco-ultra-ui` 的固定版 React/Babel 栈）。  
> 结论：**功能等价 PASS；稳态均跑满约 60 FPS；内存量级原生约为 Chromium 进程树的 1/6.27，但口径不等价；本轮没有发现崩溃、JS 异常或掉帧。**

## 1. 测试范围与产物

### 1.1 为什么选生命游戏

康威生命游戏（Conway's Game of Life，1970）是经典零玩家/细胞自动机游戏：确定性、无需按键注入、负载可由网格尺寸精确调节，适合做逐帧压力载体。两端不比"谁画得好看"，而比同一规则、同一种子、同一画布口径下的稳态帧率、单步/单帧成本、内存与异常韧性。

本测试是**独立 benchmark 资产**，不扩展 `ElementSpec` 13 变体契约，不修改 `render::COVERED`、不修改上一份 `TEST_REPORT.md` 的功能矩阵。新增的 `src/bin/life.rs` 由 Cargo 自动发现为独立二进制，不影响既有 `lilyco-native-ui` 主程序。

### 1.2 同一游戏、同一配置

| 项目 | 值 |
|---|---|
| 网格 | 120 × 80 = 9,600 格，单元格 6 px，画布 720 × 480 |
| 随机数 | xorshift32，种子 `0x4C494C59` = 1279872089；`rand() % 100 < 28` 播种 |
| 规则 | 有界 8 邻域，B3/S23；每 240 代重新播种以保持活细胞负载 |
| 节奏 | `tick_us=16667`，即 60 tick/s；两端每 tick 推进一代并触发一次重绘 |
| 功能摘要 | FNV-1a：初始网格 + 300 代逐步哈希，输出 8 位十六进制 |
| 共同 CONFIG | `cols=120,rows=80,cell=6,seed=1279872089,density=28,restart=240,digest_steps=300,tick_us=16667` |
| 共同 digest | `3ecf46af` |

JS 侧用 `>>>0`、`Math.imul(h ^ cell, 0x01000193)` 对齐 Rust 的 wrapping 位运算；Rust 侧用 `wrapping_mul`。两边 `config` 字符串与 `digest` 完全相同，`functional_match=true`。这是功能等价的硬证据，而不是靠肉眼看截图。

### 1.3 实现要点

- 原生：`#[component] fn Game()` 用 `signal_with(Life::new(), SignalEquals::Never)` 保证每 tick 都通知；`effect` 内 `set_interval(Duration::from_micros(16_667), ...)`，并把 `Spawned` 任务移入 `cleanup` 常驻（直接丢弃会因 NSH `Task` 的 Drop 语义被取消）；`canvas(move || life.with(draw))` 在闭包内读信号订阅重绘。无 WebView、无 QuickJS、无 JS 运行时。
- Web：复用 `lilyco-ultra-ui` 的 React 18.3.1 UMD + ReactDOM 18.3.1 + Babel Standalone 7.28.4 classic preset 栈；`requestAnimationFrame` 驱动，canvas 2D 按同一网格绘制；`window.__LIFE` 暴露 `digest/config/snapshot()/errors`。
- 稳态测量使用 **内联 vendor 的离线页**：`fixtures/.generated/life-offline.html`。`fixtures/life-fixed.html` 是同样的外链固定版 + SRI 页，仅作为可复现资产/冷启动参考，**不参与本批次稳态数字**。此前 `TEST_REPORT.md` 已记录在线 CDN 冷启动抖动极大（1163 ms 与 6906 ms 两个批次），故禁止把在线冷启动与本批次混算加速比。
- 3 轮/后端，顺序执行以避免互相抢 CPU；每轮 warmup 2 s（Web 另丢弃前 120 个 rAF 样本），稳态保持 9 s。

### 1.4 产物与哈希

| 文件 | 大小 | sha256 |
|---|---:|---|
| `src/bin/life.rs` | 8,442 B | `1bc58eb4e4fcec4648af5e6abc945d6c42c698aca8f881636c97bdebb652755e` |
| `bench/make_life_fixtures.py` | 7,203 B | `f43ed630441d88cb9882f62dab0eb22d6a5109bf130bef119256039eaaae1680` |
| `bench/fixtures/life-fixed.html` | 5,634 B | `afe17f636f148e7efa2301ec6f5fbfeb1a8f49feb5d3b0037224380b6fca7498` |
| `bench/fixtures/.generated/life-offline.html` | 3,217,278 B | `c1cb09024865a40329c465e73b9370085767e5291dc123ab4045212f73bc3149` |
| `bench/life_cdp.js` | 5,893 B | `c973d773595aa08ab768ea0f9f84b6d684264828711fb4a91bea82a62d72ba2d` |
| `bench/life_perf.py` | 16,556 B | `24773ea413a23f0f976791967ec8dc6e0c7e422add653247249dc187b90fbbc5` |
| `bench/results/game-perf-2026-11-15.json` | 36,975 B | `c1e759d9e029183f7f850ca1b72516cf147efc7f9ce376ed9f29f8d1f5f02ce6` |
| `bench/results/life-web-2026-11-15.png` | 7,592 B | `75b12e6414cf9145fe17e5eb3d42847a070150053729d5edd66153520b7048a4` |

## 2. 测试环境

| 项目 | 值 |
|---|---|
| 内核 | Linux 6.18.60-2-cachyos-lts |
| CPU | 16 逻辑核 |
| 显示 | `DISPLAY=:0`（XWayland），`WAYLAND_DISPLAY=wayland-1`；原生强制 `GDK_BACKEND=x11` |
| Chromium | Chromium 157.0.8103.38 Arch Linux；`--headless=new --no-sandbox --disable-gpu --disable-dev-shm-usage --window-size=760,560` |
| Node | v26.13.0（CDP 采集） |
| 原生二进制 | `/home/lilyco/.cache/lilyco-build/target/release/life`，sha256 `534803472f2ad64b3db8c16ff9cd4ee39b8669b9e055cf086c905556a6f4f45d`，release / LTO |
| Web fixture | 内联三脚本离线页，3,217,278 B；三条 CDN 请求数为 0 |
| 批次参数 | `LIFE_HOLD_MS=9000`, `LIFE_WARMUP_S=2`, `LIFE_WARMUP_SAMPLES=120`, `LIFE_RUNS=3`, `LIFE_CDP_PORT=9441` |

> NSH `nsh::fps` 本批次实测分母是 **60/60**（`fps 60/60`），不按 `xrandr` 报告的 119.96 Hz 假设 120。

## 3. 功能等价与异常韧性

| 检查 | 原生 NSH | React/Web | 判定 |
|---|---|---|---|
| 配置串 | 3/3 轮一致 | 3/3 轮一致 | PASS |
| FNV digest | `3ecf46af` 3/3 | `3ecf46af` 3/3 | PASS |
| 60 tick/s | 660 ticks / 11,000 ms = 60.00/s | 59.86–59.95 steps/s | PASS（rAF/定时器抖动内） |
| 60 draw/s | 660 draws / 11,000 ms = 60.00/s | 59.69–59.84 draws/s | PASS |
| 崩溃/异常 | 3 轮 `panics=0`，无 `CRITICAL` | `errors=[]`、`exceptions=0`、`consoleErrors=0` | PASS |
| 网络 | 不适用 | `netFail=0`、`externalRequests=0` | PASS（离线页） |
| 截图 | 窗口内 720×480 canvas | `results/life-web-2026-11-15.png` | 已归档 |

`game-perf-2026-11-15.json` 的 `functional_match=true`、`native_digest=web_digest="3ecf46af"`、`config` 单值。

补充回归证据：2026-11-15 16:13 完成 `cargo test --release --lib --tests`（`lilyco-native-ui`，release）：`src/lib.rs` 2/2、`tests/coverage.rs` 4/4，新增 `src/bin/life.rs` 与既有主程序各 0 测试，合计 **6 passed / 0 failed**。`native_renderer_declares_every_variant`、`one_spec_feeds_both_backends` 均 PASS，说明独立游戏二进制没有破坏 13 变体契约。日志：`/tmp/lilyco_native_tests.log`。

## 4. 稳态性能对比（同一批次，3 轮中位数）

### 4.1 帧率与吞吐

| 指标 | 原生 NSH GTK | React/Web | 说明 |
|---|---:|---:|---|
| 稳态 FPS | **60.00**（`nsh::fps 60/60`） | **59.88**（rAF 中位间隔 16,700 µs） | 两端都到约 60 Hz 上限 |
| frame 间隔 p50 | 16.63 ms（`gap_p50_ms`） | 16.70 ms（rAF） | 口径不同，见 5.1 |
| steps/s | **60.00** | **59.90** | 同 16,667 µs tick |
| draws/s | **60.00** | **59.75** | Web rAF 少 1–2 帧属调度抖动 |
| rAF/tick 间隔中位 | 16,666 µs（进程内 `Instant`） | 16,700 µs | 原生 tick gap 稳定 ±1 µs |

**读数**：在本负载（9,600 格 × 60 Hz）下两端都没有掉帧，`nsh::fps` 连续 9 个稳态采样均为 60/60，Web rAF 中位 16.70 ms 且 p95 16.70–16.80 ms。这个结果证明"能稳定跑满当前 60 Hz 游戏循环"，**不证明哪边极限吞吐更高**——两边都被 60 Hz 节奏封顶。

### 4.2 帧内成本（各自内部口径，禁止直接相减）

| 指标 | 原生 NSH GTK | React/Web |
|---|---:|---:|
| 规则推进 `step_us` 中位 | **72 µs** | **100 µs**（`performance.now()`，观测值被量化到 0/100/200 µs） |
| 绘制调用 `draw_us` 中位 | **47 µs**（构建 `Painter` 的 Rust CPU 时间） | **200 µs**（JS `fillRect` 调用耗时，非 GPU/合成耗时） |
| `frame_mean_ms` 中位 | **5.89 ms**（mean 6.08 / p95 6.51） | 不适用（rAF 间隔不是帧内耗时） |
| `paint_mean_ms` 中位 | **5.79 ms** | 不适用 |
| `snapshot_mean_ms` 中位 | **1.91 ms** | 不适用 |
| `canvas_mean_ms` 中位 | **1.87 ms**（NSH 画布光栅化） | 不适用 |
| JS heap | 不适用 | **9.44 MB**（`JSHeapUsedSize`） |

原生的 `draw_us=47 µs` 只是把 9,600 格展开成 `Painter` 指令；真正画布光栅化由 `nsh::fps` 的 `canvas=1.87 ms` 记录，`snapshot=1.91 ms` 包含 canvas。Web 的 `draw_us=200 µs` 只是 JS 调用 `ctx.fillRect` 的 `performance.now()` 差值，不含浏览器合成/光栅化；且本批次 Chrome 的 `performance.now()` 观测值呈 100 µs 量化（0 / 100 / 200 …），所以 **72 vs 100、47 vs 200 不能当成"Rust 比 JS 快多少倍"**。

### 4.3 内存与 CPU（口径不等价）

| 指标 | 原生 NSH GTK | React/Web | 备注 |
|---|---:|---:|---|
| 进程 RSS 中位 | **181.07 MB**（单进程 `/proc/<pid>/status`） | **1,134.89 MB**（Chromium 整棵进程树 `/proc` 求和） | 约 **6.27×** 量级差，但单进程 ≠ 多进程浏览器 |
| NSH 自报 RSS | 190.20 MB（`nsh::fps` 行内 `rss`） | 不适用 | 仅作原生内部对照，不与 Chrome 混算 |
| CPU 中位 | **37%**（`nsh::fps` 的 `cpu`） | **20.75%**（Chrome 进程树 `/proc` jiffies 增量） | 两种不同 CPU 账本，**不能据此判定谁更省 CPU** |
| JS heap 中位 | 不适用 | **9.44 MB** | 只是 JS 对象堆，不含浏览器进程开销 |

内存结论只应表述为：在本机本轮，**NSH 单进程常驻约 181 MB，完整 headless Chromium 进程树约 1,135 MB**；后者包含浏览器多进程、渲染器、GPU/合成与 Babel/React 运行时，不是同一抽象层。

### 4.4 启动数字（仅各自内部参考）

| 指标 | 原生 NSH GTK | React/Web |
|---|---:|---:|
| 首帧/`__LIFE` 就绪 | `first frame total` **0.47 ms**（median；mean 0.58 ms） | CDP `firstRenderMs` **570 ms**（mean 579.67 ms） |

这两个数**不是同一事件**：前者是 NSH 内部首帧耗时，不含窗口出现/进程启动；后者是从 `Page.navigate` 到 `window.__LIFE.digest` 可读，包含加载 3.2 MB 内联页、Babel 编译、React 挂载。禁止拿 0.47 与 570 算"快 1200 倍"。本轮 Web 固定版 CDN 页未参与测量，也不与更早批次的 1163 / 6906 ms 冷启动数字混算。

## 5. 已知限制与诚实保留

1. **FPS 口径不同**：原生是 GTK frame clock / `nsh::fps`（本批次 60/60），Web 是 `requestAnimationFrame` 间隔换算；两者都受各自调度/刷新封顶，不能直接当同一 profiler。
2. **都没有测到极限**：9,600 格 × 60 Hz 下两端均满帧。要分出上限，需要提高网格/ tick 频率（例如 240×160、120/240 tick/s）或解除 vsync/rAF 节奏，做"掉帧临界点"曲线。
3. **计时精度不同**：原生 `Instant` 微秒；Web `performance.now()` 本批次观测到 100 µs 量化，且 JS 绘制耗时不含浏览器合成/光栅化。`step_us`/`draw_us` 只用于观察量级，不做加速比。
4. **内存/CPU 抽象层不同**：单原生进程 vs 多进程 Chromium；`nsh::fps cpu` vs `/proc` jiffies；`JSHeapUsedSize` 只是 V8 堆。
5. **平台范围有限**：仅 Linux 6.18 / XWayland / headless Chromium `--disable-gpu`，单主机顺序 3 轮、每轮稳态 9 s；未覆盖 Windows/macOS/移动端、物理键鼠、长时间 soak、AT-SPI 可访问性、GPU 后端。
6. **游戏是逐帧压测夹具，不是产品功能**：没有改 13 变体契约、没有改 `lilyco-manager`、没有引入 lycomc 双后端开关；`Patch{id,node}` 与 AI JSON Schema 仍不在本轮范围。
7. **CDN 冷启动排除**：稳态只用内联 vendor 页；`life-fixed.html` 的固定版 + SRI 外链页保留作复现资产，但不与在线抖动批次比较。

## 6. 复现步骤

```bash
cd /home/lilyco/code/lilyco/lilyco-native-ui
cargo build --release --bin life
# 产物按 .cargo/config.toml 落在 /home/lilyco/.cache/lilyco-build/target/release/life

cd bench
python3 make_life_fixtures.py
LIFE_CDP_PORT=9441 LIFE_HOLD_MS=9000 LIFE_WARMUP_S=2 \
LIFE_WARMUP_SAMPLES=120 LIFE_RUNS=3 CHROMIUM=chromium \
python3 life_perf.py
```

驱动按 `native_run(i) -> web_run(i)` 顺序跑 3 轮；原生日志留在 `/tmp/life-native-<i>.log`，Web 原始 CDP 结果在 `/tmp/life-web-<i>.json`；汇总写入 `bench/results/game-perf-2026-11-15.json`，第 2 轮 Web 截图写入 `bench/results/life-web-2026-11-15.png`。

独立检查功能摘要：

```bash
GDK_BACKEND=x11 /home/lilyco/.cache/lilyco-build/target/release/life 2>&1 | head -2
# LILYCO_GAME_DIGEST 3ecf46af
# LILYCO_GAME_CONFIG cols=120,...,tick_us=16667
```

## 7. 总评

- **等价性**：同一规则/种子/画布，两端 `digest=3ecf46af` 且配置串一致，3/3 轮 PASS。
- **稳态高性能**：原生 `60/60 FPS`、Web `59.88 FPS`，各 3 轮无掉帧、无 panic/异常、无外部请求。
- **成本画像**：NSH 的规则推进 72 µs、Painter 构建 47 µs、画布光栅化 1.87 ms、单进程 RSS 181.07 MB；Web 的 JS heap 9.44 MB、Chrome 进程树 RSS 1,134.89 MB、rAF 16.70 ms。
- **不越界结论**：本轮没有证据支持"NSH 极限帧率高于 Web × 倍"或"NSH CPU 一定更省"。两端都跑在当前 60 Hz 节奏上限，且 FPS/内存/CPU 口径不等价。
- **建议下一步**：把网格放大到 240×160 或把 tick 提到 120/240 Hz 做掉帧临界曲线；再在 lycomc 可开关双后端 PoC 里用同一夹具做真实业务集成 A/B。

---
原始结果：`bench/results/game-perf-2026-11-15.json`。所有数字均来自 2026-11-15 16:05 +08:00 这一批次；**禁止与 2026-11-14 或 2026-11-15 14:4x 批次跨批次算加速比**。
