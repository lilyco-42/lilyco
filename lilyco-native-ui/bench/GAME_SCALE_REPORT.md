# Lilyco 双后端负载放大 / 60 FPS 掉帧临界报告

生成时间：2026-11-15 17:45 +08:00。本报告是 `bench/GAME_PERF_REPORT.md`（16:05 基线）的后续独立批次，不改那份报告、不改 `bench/TEST_REPORT.md`、不改 13 变体 ElementSpec 契约、不触碰 `apps/lilyco-manager`。

---

## 1. 测试范围与产物

### 1.1 目的

16:05 的康威生命游戏基线在 120×80×6（9,600 格）下双端都跑满 60 FPS，属于「被垂直同步封顶、没测到极限」。本轮分两批放大负载，找双端第一个守不住 60 FPS 的点：

- **批次 A（网格放大）**：固定 720×480 画布，把网格从 120×80×6 提到 360×240×2（9,600 → 86,400 格），只增细胞/绘制指令数，画布像素面积不变。结果：四档双端全部守住 60 FPS，仍未测到临界点。
- **批次 B（绘制重复）**：固定 360×240×2 网格与 720×480 画布，把同一批 alive-cell `fill_rect` 重复 1/2/4/8/16 遍（86,400 → 1,382,400 cell-visits/frame），模拟状态、种子、FNV-1a digest 全程不变，只放大 CPU / 光栅化负载。结果：找到临界点。

### 1.2 判定口径

每档每后端顺序跑 2 轮（warmup 2 s + hold 7 s，去前 60 个 rAF 样本）。「守住 60 FPS」= 两轮同时满足 `fps >= 58` 且 `steps_per_s >= 55` 且 `draws_per_s >= 55`。加上后两个条件是因为 Web 的 `effective_fps = 1e6 / median(rAF)` 在半数帧被拉长时仍报 59.88（见 §5.2），单看中位 FPS 会漏判掉帧。

### 1.3 产物清单（SHA-256 已实测）

| 文件 | 字节 | SHA-256 |
| --- | ---: | --- |
| `bench/GAME_SCALE_REPORT.md`（本文） | 见落盘 | 见落盘 |
| `src/bin/life_scale.rs` | 9,297 | `58bcfe2745a46bc28629bbc73bf96463e29a490e625ab85454a84bcab699c67c` |
| `src/bin/life_draw.rs` | 9,521 | `12db91df88276b65919d389a3fc63a7b9725bad8b904f1a683fc48a2bf91a525` |
| `bench/make_life_scale_fixtures.py` | 3,328 | `176247e7d7bc48e0e860ce51b3ae8bea668955239e87a6caa00fc080084ea196` |
| `bench/make_life_draw_fixtures.py` | 3,877 | `9e7e99b801737bfc44cb35a4d6141c0ef77e9f8e080657f704d833be0146fd64` |
| `bench/life_scale.py` | 8,070 | `fa5e0182201e3781aef41741471fbc2f05b6abab07bedc17d5e591546262e02c` |
| `bench/life_scale_draw.py` | 6,773 | `113e21adcc2ebccb6c531a41fdcc0935f58b25046947739716c23ce15d7d868f` |
| `bench/results/game-scale-2026-11-15.json` | 31,325 | `519f2553dc0b9dbf8bfa5c1c562b443ba15ff81cbed5cb6ed5dba58f040965cb` |
| `bench/results/game-scale-draw-2026-11-15.json` | 40,041 | `17cff055b4103bc548d59979cf2e21c59fc008af818f95f7cde246b9b96bd9ee` |

离线 Web 夹具（`bench/fixtures/.generated/`，均内联 react-18.3.1 / react-dom-18.3.1 / babel-7.28.4 vendor 脚本，0 个 unpkg 外链）：

| 夹具 | 字节 | SHA-256 |
| --- | ---: | --- |
| `life-scale-120x80x6.html` | 3,217,275 | `316cb8ab8ae52ee0d12e108239840a78f716c218d836dab2914c26516c573696` |
| `life-scale-180x120x4.html` | 3,217,278 | `821b9d09aedefd487ce682b966ae682842586b4cae6eb3120f3c97ca46a2395a` |
| `life-scale-240x160x3.html` | 3,217,278 | `5b0358a066cad614559809819a77c9fa6d0241309b926f3dc0a8949a561196f2` |
| `life-scale-360x240x2.html` | 3,217,278 | `5511362067d0a4f69ddd878810e60c476893161ceb8ddf8f1c47dbb55b5ae025` |
| `life-draw-1.html` | 3,217,389 | `36f1f09ac3402f1bd5618cc6c6b194f27b6ad91e89588dbc28326c7f2dbdadb6` |
| `life-draw-2.html` | 3,217,389 | `79463ba21a383f30a1e8d8880b5f77eecb802e0f248735c0a4fa03a2e467b7e3` |
| `life-draw-4.html` | 3,217,389 | `229039e9fa4ac49aa47d0e0da6fae5e488554a6abcf001ff89f4cffc4e64a8c7` |
| `life-draw-8.html` | 3,217,389 | `5535eca5bb86e40335feafd841d5464ebe9eea82ebbb8f75c68c304b0f0a9614` |
| `life-draw-16.html` | 3,217,392 | `06201d8a087942e491c2ac2369c13a2ce8a7ed172a4495b1cb7a7e0d89e7c695` |

二进制（独立 workspace target 目录）：`/home/lilyco/.cache/lilyco-build/target/release/life_scale` 36,201,808 B sha `4bc2bb761544571b27b9f2f5517146516e10a80925d2199a5825cdbfe8a899a4`；`.../life_draw` 36,201,872 B sha `02d1d013450c26075ebbf42e427d55d01bcbbcdc68a8d6db3303659732fc9fb7`。

---

## 2. 测试环境

| 项 | 值 |
| --- | --- |
| 内核 | Linux 6.18.60-2-cachyos-lts |
| CPU | 16 逻辑核 |
| 显示 | `DISPLAY=:0`（XWayland），`WAYLAND_DISPLAY=wayland-1`，原生侧 `GDK_BACKEND=x11` |
| 浏览器 | Chromium 157.0.8103.38 Arch Linux，`--headless=new --no-sandbox --disable-gpu --disable-dev-shm-usage` |
| Node | v26.13.0（CDP 驱动） |
| 批次 A | 2026-11-15T17:20:27+0800，CDP 端口 9442，4 档 × 2 轮 / 后端 |
| 批次 B | 2026-11-15T17:39:23+0800，CDP 端口 9443，5 档 × 2 轮 / 后端 |
| 轮次参数 | warmup 2.0 s、稳态 7,000 ms、每档 2 轮、同一主机顺序执行 |

**批次隔离**：A / B 与 16:05 基线是三台不同二进制、三个时间点、两次独立构建，禁止跨批次算加速比或把不同批次的数字混在同一结论里。

---

## 3. 功能等价与异常韧性

| 检查 | 结果 |
| --- | --- |
| 批次 A digest | 四档 Rust 与 JS 逐档一致：`8a705904` / `e53804f3` / `79cca185` / `32e242a6`；每档 `functional_match=true` |
| 批次 B digest | 五个 repeat 全部 `32e242a6`（同一模拟状态），每档 `functional_match=true` |
| 配置串 | `cols=360,rows=240,cell=2,seed=1279872089,density=28,restart=240,digest_steps=60,tick_us=16667` + `draw_repeat=<rep>`，Rust/JS 完全一致 |
| Web 异常 | 10 个 Web 轮次 `errors=0` / `exceptions=0` / `netFail=0` / `externalRequests=0`（稳态只用内联 vendor 页，CDN 冷启动不参与） |
| 原生异常 | 10 个原生轮次 `panics=0`；高负载下无进程退出、`steps_per_s` 最低 57.67（draw-16）仍接近 60 tick/s |
| 契约回归 | `cargo test --release --lib --tests` → **6 passed / 0 failed**（2 个 lib 单测 + 4 个 coverage 契约测试；3 个 bin 各 0 tests），日志 `/tmp/lilyco_native_tests_scale.log` |

等价性由同一 xorshift32 种子、B3/S23 有界八邻域规则、240 代重播种、FNV-1a 哈希保证；`draw_repeat` 只重复绘制，不触碰 `cells` 状态，故 digest 不变是预期结果。

---

## 4. 批次 A：网格放大曲线（未测到临界点）

全部为 2 轮中位数。单位：`nFrame/nCanvas` ms、`nStep/nDraw/wDraw` µs、`nCPU/wCPU` %、`nRSS/wRSS` MB。

| 档位 | 格数 | digest | 原生 FPS | nFrame | nCanvas | nCPU | nDraw | nRSS | Web FPS | wRafP95 | wDraw | wCPU | wRSS | 双端守住 60 |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 120×80×6 | 9,600 | `8a705904` | 60.0 | 6.48 | 2.52 | 40.0 | 36.0 | 181.49 | 59.88 | 16,700 | 200 | 25.05 | 1,249.17 | ✅ / ✅ |
| 180×120×4 | 21,600 | `e53804f3` | 60.0 | 7.19 | 2.99 | 42.5 | 58.5 | 182.16 | 59.88 | 16,700 | 300 | 25.05 | 1,244.48 | ✅ / ✅ |
| 240×160×3 | 38,400 | `79cca185` | 60.0 | 6.68 | 2.71 | 43.0 | 98.5 | 183.86 | 59.88 | 16,800 | 650 | 33.85 | 1,240.96 | ✅ / ✅ |
| 360×240×2 | 86,400 | `32e242a6` | 60.0 | 9.55 | 5.16 | 61.5 | 193.0 | 189.94 | 59.88 | 16,750 | 1,150 | 44.35 | 1,138.43 | ✅ / ✅ |

顶层字段 `largest_native_holding_60fps = largest_web_holding_60fps = 360x240x2`，`first_native_drop = first_web_drop = null`。成本确实随格数上升（原生 CPU 40→61.5%，Web `draw_us` 200→1,150 µs），但 60 Hz 帧预算 16.67 ms 远未用尽，所以单纯增格数不构成有效掉帧压力；这才引出批次 B。

---

## 5. 批次 B：绘制重复掉帧临界曲线

### 5.1 聚合结果（2 轮中位数）

| 档位 | cell-visits/frame | 原生 FPS | nFrame | nCanvas | nCPU | nDraw | nRSS | Web FPS | wRafP95 | wDraw | wCPU | wRSS | wDraws/s | 守住 60 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| draw-1 | 86,400 | 60.0 | 8.33 | 4.73 | 56.0 | 190.75 | 187.92 | 59.88 | 16,700 | 900 | 36.9 | 1,145.91 | 59.34 | ✅ / ✅ |
| draw-2 | 172,800 | 60.0 | 10.53 | 6.89 | 68.0 | 336.0 | 198.81 | 59.88 | 16,700 | 1,800 | 48.1 | 1,142.15 | 59.23 | ✅ / ✅ |
| draw-4 | 345,600 | 59.0 | 15.09 | 10.86 | 98.5 | 704.75 | 221.07 | 59.88 | 16,800 | 8,600 | 70.2 | 1,146.21 | 56.08 | ✅ / ✅（临界边缘） |
| draw-8 | 691,200 | **42.0** | 25.79 | 18.29 | 114.5 | 1,297.25 | 271.86 | **59.88** ⚠️ | **49,900** | 17,550 | 82.35 | 1,136.03 | **36.71** | ❌ / ❌ |
| draw-16 | 1,382,400 | **20.5** | 52.14 | 39.30 | 133.25 | 2,827.0 | 405.56 | **20.0** | **100,000** | 41,450 | 83.2 | 1,137.31 | 18.05 | ❌ / ❌ |

顶层判定：`largest_native_holding_60fps = largest_web_holding_60fps = draw-4`；`first_native_drop = first_web_drop = draw-8`。

### 5.2 两个必须一起看的信号

**（1）原生掉帧是帧时钟直接掉。** draw-4 时 `canvas_mean` 已到 10.86 ms、CPU 98.5%，只差一点就吃满 16.67 ms 预算；draw-8 时 `canvas_mean` 18.29 ms 超过帧预算，GTK frame clock 从 59 掉到 42 FPS，draw-16 再掉到 20.5 FPS（canvas 39.30 ms）。瓶颈定位清楚：`nsh::fps` 的 `canvas` 是画布光栅化，`nDraw`（Painter 构造 1.3→2.8 ms）远小于光栅化，说明不是 Rust 侧构建绘制列表，而是把 69 万 / 138 万次矩形真正画出来。

**（2）Web 的中位 FPS 是陷阱。** draw-8 的 `effective_fps` 仍是 59.88（rAF 中位 16.7 ms），但 `raf_mean` 25.12 / 25.29 ms、`raf_p95` 49.9 ms、`raf_max` 66.8–83.3 ms、实际 `draws_per_s` 只剩 36.71；draw-16 才在中位数上暴露为 20 FPS（rAF 中位 50 ms、p95 100 ms、`draws_per_s` 18.05）。也就是说 Web 在 draw-4 已出现长帧（两轮 `raf_max` 33.4 ms，`draws/s` 56.08 逼近 55 门槛），draw-8 已经实质掉帧，只是被「取中位数」掩盖。判定脚本因此要求 `draws_per_s >= 55`，而不是只看 FPS。

逐轮原始证据（draw-8 / draw-16）：

| 轮次 | 原生 FPS | 原生 canvas ms | Web rAF mean/p95/max µs | Web draws/s | Web draw_us |
| --- | ---: | ---: | --- | ---: | ---: |
| draw-8 run1 | 43.0 | 17.54 | 25,121 / 49,900 / 83,300 | 36.87 | 17,600 |
| draw-8 run2 | 41.0 | 19.03 | 25,286 / 49,900 / 66,800 | 36.55 | 17,500 |
| draw-16 run1 | 21.0 | 38.98 | 54,664 / 100,000 / 183,300 | 18.25 | 41,600 |
| draw-16 run2 | 20.0 | 39.62 | 53,603 / 100,000 / 166,600 | 17.85 | 41,300 |

### 5.3 临界点结论

- **双端最后一个稳住 60 FPS 的点**：`draw-4` = 345,600 cell-visits/frame（原生 59 FPS / draws 59.97/s；Web 59.88 FPS / draws 56.08/s，已到边缘）。
- **双端第一个判定掉帧的点**：`draw-8` = 691,200 cell-visits/frame。原生明确 42 FPS；Web 实际绘制吞吐 36.71/s + rAF p95 49.9 ms，虽然中位 FPS 仍显示 59.88。
- **真实临界区间（粗扫阶段）**：345,600 ~ 691,200 cell-visits/frame 之间；18:05 细扫与 18:10 同批次边界复测见 §5A，细扫已把 Web 掉帧点推进到 draw-5、原生掉帧点收窄到 draw-4。
- **极限压力点**：`draw-16` 双端都塌到约 20 FPS。原生 CPU 133%、canvas 39.3 ms、单进程 RSS 405.56 MB；Web JS `fillRect` 41.45 ms、CPU 83.2%、Chrome 进程树 RSS 1,137.31 MB、JS heap 9.76 MB。
- 两端在 draw-8 一起失效，说明瓶颈是「同一帧里画 69 万+ 个小矩形」这一公共绘制量，而不是 Life 规则计算（原生 `step_us` 全程 565–904 µs，Web `step_us` 900–1,300 µs，几乎不随 repeat 变化）。

---

## 5A. 同批次边界复测：draw-4 与 draw-5

粗扫（17:39）说 draw-4 守住，18:05 细扫说 draw-5 已掉，但两者是不同批次。为免把批次噪声当临界点，补一轮**同批次** 3 次复测（`life_scale_draw_boundary.py`，端口 9445，18:10 批次）：

| 档位 | cell-visits/frame | 原生 FPS（3 轮） | 原生判定 | Web draws/s | Web rAF p95 | Web 判定 | digest |
|---|---:|---|---|---:|---:|---|---|
| draw-4 | 345,600 | 57 / 57 / 55 | nHold=False | 55.74 / 56.13 / 56.54 | 16.8 / 16.8 / 16.8 ms | wHold=True | 32e242a6 |
| draw-5 | 432,000 | 54 / 50 / 59 | nHold=False | 48.64 / 55.07 / 55.18 | 33.4 / 16.8 / 16.8 ms | wHold=False | 32e242a6 |
- SHA-256：细扫结果 `52a8dbf2f255391447e3e2d7d936c25c70139b44fb47a9fbd58fc2be63760061`（24,863 B）；同批次边界结果 `eecb503bde25d6128c8e1d87dce36e52cfe675e93c57b8a05f62565407453a3a`（20,019 B）。

- 判定门槛仍是「全部轮次 FPS≥58 且绘制吞吐≥55/s」。原生 `nsh::fps` 的分母是 60，draw-4 三轮 57/57/55 已稳定低于门槛（窗口偶发 59 不能抵消），因此**原生第一批掉帧已收窄到 draw-4 = 345,600 cell-visits/frame**（相对粗扫 draw-8 的 691,200 下调一半）。
- Web 在同批次 draw-4 三轮全达标（draws 55.74–56.54/s、rAF p95 16.8 ms），draw-5 第 1 轮 draws 48.64/s + rAF p95 33.4 ms 掉帧，故 **Web 第一批掉帧 = draw-5 = 432,000 cell-visits/frame**。Web 侧比原生多约 25% 绘制预算余量，但两批样本都小（原生 3 轮均值 56.3，Web draw-4 均值 56.1 仅略高于 55 门槛）。
- 18:05 细扫 5/6/7 的结论（两轮均不达标，draw-6/7 明显崩到 20–30 FPS）仍成立；但该批次内部没有 draw-4 对照，不能用于确定最后达标点。边界以本 18:10 同批次为准。
- 所有档位仍是同一 360×240×2 / 720×480 夹具，双端 digest/config 完全一致，Web 0 异常 / 0 外链，原生 0 panic。

---

## 6. 已知限制与诚实保留

1. **FPS 口径不等价**：原生是 `nsh::fps` 报的 GTK frame clock（目标 60），Web 是 `1e6 / median(rAF)`。二者不是同一个计时器，本报告只分别给出各自的临界档位，**不给跨后端加速比**。
2. **中位 FPS 会掩盖长帧**：见 §5.2。Web draw-8 的 59.88 不能读成「没掉帧」，必须结合 `draws_per_s`、`raf_mean`、`raf_p95`；这是本轮判定加入吞吐门槛的原因。
3. **负载是合成的**：`DRAW_REPEAT` 把同一批矩形重复画多遍，不是产品真实界面（真实 UI 不会每帧画 138 万个格子）。它用于定位渲染管线饱和点，不代表 lilyco 13 变体业务界面的性能。
4. **计时与光栅化口径不同**：原生 `draw_us` 只量 Painter 构造，`canvas_mean_ms` 才是光栅化；Web `draw_us` 是 `performance.now()` 包住 JS `fillRect` 调用，不含浏览器合成/提交；`performance.now()` 有 100 µs 量化。两者不能直接相减。
5. **内存口径不同**：`nRSS` 是单个原生进程 `/proc/<pid>/status VmRSS`，`wRSS` 是整个 Chromium 进程树求和；`1,137 MB` vs `405 MB` 不是同一抽象层的对比。
6. **平台与采样有限**：仅 Linux / XWayland / headless Chromium 且 `--disable-gpu`；无 macOS / Windows / 移动端 / GPU 合成路径；每点 2 轮、稳态 7 s，未做 soak、无物理键鼠、未测 120 Hz。
7. **CDN 冷启动排除**：稳态 Web 用内联 vendor 夹具（`externalRequests=0`）；按 `TEST_REPORT.md` 的既有结论，在线 CDN 冷启动抖动 1,163→6,906 ms 不可复现，不纳入本曲线。
8. **批次隔离**：A（17:20）、B（17:39）、细扫（18:05）、边界复测（18:10）与 16:05 基线各自独立；跨批次结论只用于定性，精确临界点以 §5A 同批次复测为准，禁止混算数值。
9. **未覆盖的后续项**：lycomc 可开关双后端 PoC、`Patch{id,node}` 增量协议、AI 侧 JSON Schema 导出均未在本轮实现；本轮也未改 13 变体契约。

---

## 7. 复现步骤

~~~bash
cd /home/lilyco/code/lilyco/lilyco-native-ui
cargo build --release --bin life_scale
cargo build --release --bin life_draw

cd bench
python3 make_life_scale_fixtures.py      # -> fixtures/.generated/life-scale-*.html
python3 make_life_draw_fixtures.py       # -> fixtures/.generated/life-draw-*.html

# 批次 A：网格放大（端口 9442）
LIFE_SCALE_RUNS=2 LIFE_SCALE_HOLD_MS=7000 LIFE_SCALE_PORT=9442 CHROMIUM=chromium \
  python3 life_scale.py > /tmp/life-scale-run.log 2>&1

# 批次 B：绘制重复（端口 9443，逐点增量落盘）
LIFE_SCALE_RUNS=2 LIFE_SCALE_HOLD_MS=7000 LIFE_SCALE_PORT=9443 CHROMIUM=chromium \
  python3 life_scale_draw.py > /tmp/life-draw-run.log 2>&1

# 细扫（端口 9444）与同批次边界复测（端口 9445）
LIFE_SCALE_RUNS=2 LIFE_SCALE_HOLD_MS=7000 LIFE_SCALE_PORT=9444 CHROMIUM=chromium \
  python3 life_scale_draw_fine.py > /tmp/life-draw-fine-run.log 2>&1
LIFE_SCALE_RUNS=3 LIFE_SCALE_HOLD_MS=7000 LIFE_SCALE_PORT=9445 CHROMIUM=chromium \
  python3 life_scale_draw_boundary.py > /tmp/life-draw-boundary-run.log 2>&1

# 契约 / 单元回归
cd .. && cargo test --release --lib --tests
~~~

结果文件：`bench/results/game-scale-2026-11-15.json`（批次 A）、`bench/results/game-scale-draw-2026-11-15.json`（批次 B）、`bench/results/game-scale-draw-fine-2026-11-15.json`（18:05 细扫）、`bench/results/game-scale-draw-boundary-2026-11-15.json`（18:10 同批次边界）。驱动 `life_scale.py` / `life_scale_draw.py` 复用 `life_perf.py` 的 `native_run()` / `web_run()`；原生侧通过 `LIFE_COLS` / `LIFE_ROWS` / `LIFE_CELL` / `LIFE_TICK_US` / `LIFE_DRAW_REPEAT` 环境变量切档，Web 侧由对应内联夹具切档。

---

## 8. 总评

把负载从 9,600 格放大到 86,400 格（批次 A）双端仍满帧 60 FPS，说明 lilyco 原生 NSH/GTK 与 React/Web 生成器在「正常规模逐帧界面」下都有充足余量。继续把绘制量推到 34.6 万 → 69.1 万 → 138.2 万次 cell-visit/frame（批次 B）后，粗扫把临界点落在 **draw-4 与 draw-8 之间**，同批次边界复测进一步收窄为：**原生首批掉帧 draw-4 / 345,600 cell-visits，Web 首批掉帧 draw-5 / 432,000 cell-visits**（§5A）；draw-16 双端都跌到约 20 FPS。原生的瓶颈可读地定位在 canvas 光栅化（`canvas_mean` 从 4.73 → 39.30 ms），Web 的瓶颈是同步 `fillRect` + 合成（`draw_us` 900 → 41,450 µs，rAF p95 100 ms）。

**过程无功能回归**：9 个档位双端 digest 全部一致、Web 0 异常 / 0 外链、原生 0 panic、`cargo test --release --lib --tests` 6 passed / 0 failed。改动全部落在 `lilyco-native-ui/src/bin/` 与 `lilyco-native-ui/bench/` 的新增文件，未提交 git，未触碰 `apps/lilyco-manager` 用户文件。

若要继续细化，建议：① 在 draw-4/5 增加轮次（如 10 轮）并同步提高 Web 门槛到 draws≥57/s，确认 Web 是否也落在 draw-4；② 换成 GPU 开启的真实窗口与 120 Hz 屏重测；③ 把该曲线接到 lycomc 双后端开关上，按绘制预算自动选择后端。

