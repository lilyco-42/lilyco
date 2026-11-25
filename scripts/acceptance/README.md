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

## lbin（`lilyco-binfmt`）：一份脚本跑完四端

```bash
BIN=target/debug/lbin          # 或 .exe
python scripts/acceptance/binfmt_probe.py "$BIN" /绝对路径/某个真实二进制 \
       [winpty 用工作目录] /绝对路径/某个办公文件
```

`lbin` 十三条命令全 T0 只读：四条二进制结构（`identify` / `entries` / `regions` / `symbols`）读第 2 个参数，九条办公文件（`office-info` / `office-text` / `office-meta` / `office-doc` / `office-sheet` / `office-slide` / `office-package` / `office-objects` / `office-pdf`）读第 4 个（默认是仓库里那份 `notes.docx`）。比对基准是
**同一个文件在四端拿到同一份 JSON**：脚本先取 CLI `--json` 作基准，再逐端比对——
Web（CSRF 401 → 带令牌 SSE → 逐字比对 → 未知命令 400）、MCP（握手 → `tools/list` 全部工具且
`path` 必填 → 逐条调用与 CLI 逐字比对 → 缺参 `-32602`）、TUI（winpty 真 PTY：选择页滚一遍能列出全部命令 →
`↓` → `Enter` 进表单 → `Esc`/`q` 退出；不带 `--tui` 裸跑降级 CLI）。退出码 0 才算全过。

> 用真实文件而不是造的样本：`identify` 的判据是「表要刚好铺进文件」，
> 手搓的字节流很容易四端都拿到同一个假答案，比不出差异。

> winpty 发键的坑：`Esc` 与后一个键**贴着发**（0.4s）会被 crossterm 当成一整个转义序列吞掉，
> 于是「表单页 Esc 后 q」看起来像退不出去（实测：0.4s 挂、1.2s 干净退出）。
> 同理 `Ctrl-C` 在 winpty 下本来就不能指望——退出判定只认 Esc/q。
> 另一个坑：从表单退出要两次状态跳转，`wait` 只给 4 秒会把干净的退出误判成挂死。

## lchatgpt（`lilyco-chatgpt`）：只读命令四端一致

```bash
BIN=target/debug/lchatgpt
python scripts/acceptance/chatgpt_probe.py "$BIN"            # 在线：真打 chatgpt.com
python scripts/acceptance/chatgpt_probe.py "$BIN" --offline  # CI：只验协议面，不联网
```

`lchatgpt` 七条只读命令（`whoami` / `usage` / `models` / `list` / `show-conversation` / `search` / `auth`，全 T0）
与两条提权命令（`ask` 会消耗配额、`login` 会落盘凭证，均 T1）。探针覆盖：`--schema` 的 9 条命令与分级 →
CLI `--json` 基准（`auth` 是本地体检，`--offline` 也照跑）→
**额度归属断言**（`ask --cdp 127.0.0.1:1` 必须失败，报错里必须有 `--allow-codex`、且**不能**出现
`codex/responses` —— 证明没勾选时一分 codex 额度都没花；这条在线离线都跑）→
Web（首页 9 条命令、裸 `POST /run` 401、带令牌 `whoami` 的 SSE `done.result` 与 CLI 逐字一致）→
MCP（`tools/list` 9 工具、`ask` 描述自报 T1、`whoami` 放行、`ask` 与 `login` 都被安全门拒绝、
`show-conversation` 缺参 `-32602`）→ TUI（真 PTY：选择页列出命令 → `q` 干净退出）。

> 这个域的命令**真的会访问 chatgpt.com**，凭证按 `LCHATGPT_ACCESS_TOKEN` → `LCHATGPT_AUTH_FILE` →
> `~/.local/share/lchatgpt/auth.json` → `~/.codex/auth.json` 的顺序找。
> 所以 CI 里一律用 `--offline`：只验注册表形状与三端协议，不发起任何外网请求。
>
> `ask` 默认花 **chat 额度**（驱动一台已登录的 chromium 发消息），`--allow-codex` 才动 codex 额度。
> 上面那条额度归属断言是 P0 的守卫：探针在 CI 里从不真的发消息，只验证「没勾选时不会走到 codex/responses」。

> PTY 的坑：`pty.openpty()` 之后必须 `ioctl(TIOCSWINSZ)` 设窗口尺寸，否则是 0×0，
> ratatui 会把命令列表挤成一列甚至画不出来（实测只收到 783 字节）。另外比对前要先剥 ANSI 转义，
> 否则命令名被颜色码打断成 `\x1b[3whoami` 之类。
