# 四端验收探针

`lfiles` 作为「一域一二进制 × 四端」样板时写的端到端验收脚本。
CLI 端直接用 `cargo test` 之外的**真机**命令（见 `docs/MVP_SCOPE.md` §4），
其余三端用这里的探针跑。

## 用法

先准备夹具目录（含几个 jpg / txt，用于比对四端结果）：

```bash
BIN=target/debug/lfiles        # 或 .exe
FIX=/tmp/lfweb                 # 夹具目录（绝对路径！）
mkdir -p "$FIX/ren_demo"
printf aaa > "$FIX/a.jpg"; printf bbb > "$FIX/b.jpg"; printf cc > "$FIX/c.txt"
printf xxxx > "$FIX/ren_demo/x.txt"; printf yyyy > "$FIX/ren_demo/y.txt"
```

### MCP

```bash
python scripts/acceptance/mcp_probe.py "$BIN" "$FIX"
```

覆盖：`initialize` → `tools/list`（4 工具，`rename` 带 `[safety: T1]`）→
`tools/call find`(T0 放行) → `tools/call rename`(T1 被拒) → 缺参 `-32602`。

> 时序坑：探针**轮询 stdout 直到期望 id 到齐**，不用 `communicate()`。
> 后者会写完立即关 stdin，触发 MCP EOF 语义（已修复为 join 在途 worker，
> 但轮询更稳）。

### Web

```bash
# 先产 CLI 参考 JSON（必须用同一个绝对 root，否则 path 形态不同无法比对）
"$BIN" find --root "$FIX" --pattern '*.jpg' --json > "$FIX/cli_find.json"

python scripts/acceptance/web_probe.py "$BIN" "$FIX" 18081 --expect "$FIX/cli_find.json"
```

覆盖：首页下拉 4 命令 → `?cmd=` 渲染各异 → **裸 `POST /run` 应 401**（CSRF 令牌
中间件）→ 带令牌 `POST /run` + SSE 拿 `done` → **结果与 CLI `--json` 逐字一致**
（忽略 `duration_ms`）→ `rename`(T1) 在 Web 面放行。

> 令牌从首页 `<meta name="lilyco-token">` 解析，头部名 `X-Lilyco-Token`。
> 比对务必传**绝对** `root`：`find` 结果里 `root`/`path` 是原样回填的，
> 相对 vs 绝对会产生假失败。

### TUI

```bash
python scripts/acceptance/tui_probe.py "$BIN" "$FIX"
```

需要 `winpty`（Git Bash 自带）起真 PTY。覆盖：裸跑降级 CLI →
`--tui` 落选择页（4 命令 + 光标）→ `↓` 移动 → `Enter` 进表单（字段 + CLI 预览
+ 实时校验）→ `Esc`/`q` 干净退出。

> 实现要点：winpty 下 stdout 是管道，`read()` 会阻塞到缓冲满 →
> 必须用**后台线程持续 `read1`** 累积，主线程只管发键与计时。
