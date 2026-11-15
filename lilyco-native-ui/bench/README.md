# bench/ — A/B 复现资产

同一份 `fixtures/bench-spec.json`（10 元素图片压缩工具）同时喂给 React 页面与 NSH/GTK 原生窗口。
数字与口径见上层 `README.md`，逐轮原始数据在 `results/`。

## 一键准备

```bash
cd lilyco-native-ui
cargo build --release
cd bench
python3 fetch_vendor.py           # 下载 vendor/ 并按 SRI(sha384)+sha256 校验
./build_x11bench.sh               # gcc -O2 -lX11 -lXtst
python3 make_fixtures.py          # 生成与实测逐字节一致的 fixtures/.generated/react-offline.html
```

`vendor/` 不入库（5.4 MB，会触发仓库大文件守则），版本、URL、sha384/SHA-256
都钉在 `fetch_vendor.py` 里。已有文件用 `python3 fetch_vendor.py --check` 校验。

## 三组脚本

```bash
BENCH_RUNS=3 python3 web_bench.py /tmp/bench-web.json
python3 native_bench.py           # 默认 5 轮，输出 /tmp/bench-native.json
BENCH_RUNS=5 python3 interaction_bench.py
```

Web 分四档：`fixed_online`（固定版本 + SRI + classic，走 3 个 CDN）、
`old_unpinned`（修复前页面，白屏对照）、`offline_vendored`（脚本内联、零网络）、
`cdn_blocked`（CDP 屏蔽 unpkg，测断网可用性）。交互脚本用 AT-SPI
`nsh.activate` 精确激活「开始压缩」；本机是 XWayland，XTest 合成按钮事件不投递 GTK 子窗口，
不能用模拟鼠标点击测原生延迟。

`interaction_bench.py` 默认写 `results/native-interaction-2026-11-14.json`；
只想冒烟不覆盖归档时设 `BENCH_OUT=/tmp/x.json BENCH_ARCHIVE=/tmp/y.json BENCH_RUNS=1`。

## 依赖

`chromium`（headless + CDP）、`node`（全局 WebSocket）、`libatspi.so.0`、`libXtst`、
`gcc`、`Python 3.10+`。`fetch_vendor.py` 拉下来的四个脚本是固定版本字节，
前三个的 SRI 与生产 `generator.rs` 一一对应。

`make_fixtures.py` 生成的离线页面 sha256 为
`e83dfeb2bb4584da8e11fc59b9cf6263beeee45fd601a02034281ac88bdee545`，
与 2026-11-14 实测的 `/tmp/fix-proposed.html` 完全相同。重构路径后单轮 CDP 冒烟：
`firstRenderMs=452`、`clickLatency=16.2 ms`、`exceptions=[]`、`netFail=[]`；
这轮冒烟只验证复现脚本可跑，不覆盖 `results/` 里的正式汇总。
