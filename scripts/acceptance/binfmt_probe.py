"""lbin 四端验收探针（对齐 docs/MVP_SCOPE.md §4 的验收标准）。

用法：python binfmt_probe.py <lbin 二进制> <一个真实文件> [winpty 用工作目录]

覆盖：
  1. CLI      四条命令各出一次 --json，作为比对基准
  2. Web      --gui → 首页含四命令 → 裸 POST 401 → 带令牌 SSE → 与 CLI **逐字一致**
  3. MCP      initialize → tools/list（4 工具、path 必填、read_only）→ tools/call 与
              CLI 逐字一致 → 缺参 -32602
  4. TUI      winpty 真 PTY：选择页四条命令都在 → Enter 进表单 → Esc/q 干净退出；
              裸跑（不带 --tui）降级 CLI

实现要点沿用 tui_probe.py：winpty 下 stdout 是管道，必须后台线程 read1 累积。
"""
import json
import os
import re
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

BIN = os.path.abspath(sys.argv[1])
SAMPLE = os.path.abspath(sys.argv[2])
CWD = os.path.abspath(sys.argv[3]) if len(sys.argv) > 3 else os.path.dirname(SAMPLE)
COMMANDS = ["identify", "entries", "regions", "symbols"]
ARGS = {"path": SAMPLE, "max-bytes": 1 << 24}
STRIP = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]|\x1b[()][A-Z0-9]|\x1b\][^\x07]*\x07|\x1b[=>]")
CTRL = re.compile(r"[\r\x00-\x08\x0b-\x1f\x7f]")
RESULTS = []


def record(name, ok, detail=""):
    RESULTS.append((name, ok, detail))
    print("  %s %-46s %s" % ("PASS" if ok else "FAIL", name, detail[:96]))


def cli_json(cmd):
    out = subprocess.run([BIN, cmd, "--path", SAMPLE, "--max-bytes", str(ARGS["max-bytes"]),
                          "--json"], capture_output=True, text=True, encoding="utf-8",
                         errors="replace", timeout=60, cwd=CWD)
    try:
        return json.loads(out.stdout), out
    except Exception:
        return None, out


def canon(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False)


# ── 1) CLI ────────────────────────────────────────────────────────
print("=== 1) CLI：四条命令的 --json 作基准 ===")
base = {}
for cmd in COMMANDS:
    payload, proc = cli_json(cmd)
    base[cmd] = payload if payload is not None else {
        "__error__": (proc.stderr or proc.stdout or "").strip()
    }
    record("cli %s 出结构化 JSON" % cmd, payload is not None,
           "exit=%d %s" % (proc.returncode, (proc.stderr or "")[:60]))

# ── 2) Web ────────────────────────────────────────────────────────
print("\n=== 2) Web：--gui + CSRF + SSE，与 CLI 逐字比对 ===")
PORT = 18099
BASE = "http://127.0.0.1:%d" % PORT
gui = subprocess.Popen([BIN, "--gui"], cwd=CWD, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL,
                       env=dict(os.environ, LILYCO_PORT=str(PORT)))
try:
    time.sleep(3)
    html = urllib.request.urlopen(BASE + "/", timeout=20).read().decode("utf-8", "replace")
    hit = [c for c in COMMANDS if c in html]
    record("web 首页渲染四条命令", len(hit) == 4, "%d/4 命中，页面 %d 字节" % (len(hit), len(html)))
    token = re.search(r'name="lilyco-token" content="([^"]+)"', html)
    record("web 首页带 CSRF 令牌", token is not None)
    token = token.group(1) if token else ""
    body = json.dumps({"cmd": "identify", "args": ARGS}).encode()
    try:
        urllib.request.urlopen(urllib.request.Request(BASE + "/run", data=body), timeout=20)
        record("web 裸 POST 被拒（401）", False, "竟然放行了")
    except urllib.error.HTTPError as error:
        record("web 裸 POST 被拒（401）", error.code == 401, "HTTP %d" % error.code)
    for cmd in COMMANDS:
        payload = json.dumps({"cmd": cmd, "args": ARGS}).encode()
        req = urllib.request.Request(BASE + "/run", data=payload, headers={
            "Content-Type": "application/json", "X-Lilyco-Token": token})
        try:
            sid = json.loads(urllib.request.urlopen(req, timeout=60).read())["session_id"]
        except Exception as error:
            record("web %s 结果与 CLI 逐字一致" % cmd, False, "派发失败 %s" % error)
            continue
        got, types = None, []
        with urllib.request.urlopen(BASE + "/progress/" + sid, timeout=90) as stream:
            for raw in stream:
                line = raw.decode("utf-8", "replace").strip()
                if not line.startswith("data:"):
                    continue
                event = json.loads(line[5:].strip())
                types.append(event.get("type"))
                if "result" in event or event.get("type") == "error":
                    got = event
                    break
        if base[cmd].get("__error__"):
            said = ((got or {}).get("result") or got or {})
            same = str(said.get("message", "")).strip() == base[cmd]["__error__"]
        else:
            same = got is not None and "result" in got and canon(got["result"]) == canon(base[cmd])
        record("web %s 结果与 CLI 逐字一致" % cmd, same, "事件流 %s" % "->".join(types))
    bad = json.dumps({"cmd": "nope", "args": ARGS}).encode()
    try:
        urllib.request.urlopen(urllib.request.Request(BASE + "/run", data=bad, headers={
            "Content-Type": "application/json", "X-Lilyco-Token": token}), timeout=20)
        record("web 未知命令被拒（400）", False, "竟然放行了")
    except urllib.error.HTTPError as error:
        record("web 未知命令被拒（400）", error.code == 400, "HTTP %d" % error.code)
finally:
    gui.terminate()
    time.sleep(0.5)
    if gui.poll() is None:
        gui.kill()

# ── 3) MCP ────────────────────────────────────────────────────────
print("\n=== 3) MCP：stdio 握手 / 工具表 / 调用 / 缺参 ===")
NL = chr(10)
lines = [
    {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2024-11-05", "capabilities": {},
        "clientInfo": {"name": "binfmt-probe", "version": "0"}}},
    {"jsonrpc": "2.0", "method": "notifications/initialized"},
    {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
]
for index, cmd in enumerate(COMMANDS):
    lines.append({"jsonrpc": "2.0", "id": 10 + index, "method": "tools/call",
                  "params": {"name": cmd, "arguments": ARGS}})
lines.append({"jsonrpc": "2.0", "id": 99, "method": "tools/call",
              "params": {"name": "identify", "arguments": {}}})
proc = subprocess.Popen([BIN, "--mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL, cwd=CWD, text=True,
                        encoding="utf-8", errors="replace")
buf = {}


def reader():
    for line in proc.stdout:
        line = line.strip()
        if not line:
            continue
        try:
            d = json.loads(line)
        except Exception:
            continue
        if isinstance(d, dict) and d.get("id") is not None:
            buf[d["id"]] = d


threading.Thread(target=reader, daemon=True).start()
try:
    proc.stdin.write(NL.join(json.dumps(one) for one in lines) + NL)
    proc.stdin.flush()
except Exception as error:
    record("mcp 写入请求", False, str(error))
deadline = time.time() + 90
while time.time() < deadline and not all(key in buf for key in (1, 2, 10, 11, 12, 13, 99)):
    time.sleep(0.3)
try:
    proc.stdin.close()
except Exception:
    pass
proc.wait(timeout=10) if proc.poll() is None else None

init = buf.get(1, {})
record("mcp initialize 回协议版本", init.get("result", {}).get("protocolVersion") == "2024-11-05")
tools = buf.get(2, {}).get("result", {}).get("tools", [])
names = sorted(one.get("name") for one in tools)
record("mcp tools/list 四条命令齐", names == sorted(COMMANDS), str(names))
shape = all(one["inputSchema"].get("required") == ["path"] and
            "path" in one["inputSchema"]["properties"] for one in tools) and len(tools) == 4
record("mcp 每个工具 path 必填且类型对", shape)
tags = [one.get("description", "") for one in tools]
readable = all(("T0" in one or "read_only" in one or "只读" in one or
                "read-only" in one.lower()) for one in tags)
record("mcp 每条工具都带上只读分级", readable, (tags[0][:70] if tags else "无工具"))
for index, cmd in enumerate(COMMANDS):
    got = buf.get(10 + index, {}).get("result", {})
    text = "".join(one.get("text", "") for one in got.get("content", []))
    if base[cmd].get("__error__"):
        same = text.strip() == base[cmd]["__error__"]
    else:
        try:
            same = canon(json.loads(text)) == canon(base[cmd])
        except Exception:
            same = False
    record("mcp %s 结果与 CLI 逐字一致" % cmd, same, "isError=%s" % got.get("isError"))
missing = buf.get(99, {})
error = missing.get("error", {})
record("mcp 缺 path 被 -32602 拦下", error.get("code") == -32602 and
       "path" in json.dumps(error, ensure_ascii=False), json.dumps(error, ensure_ascii=False)[:70])

# ── 4) TUI ────────────────────────────────────────────────────────
print("\n=== 4) TUI：winpty 真 PTY ===")
try:
    probe = json.loads(subprocess.run([BIN, "identify", "--path", SAMPLE, "--json"],
                                      capture_output=True, text=True, encoding="utf-8",
                                      errors="replace", timeout=30, cwd=CWD,
                                      env={k: v for k, v in os.environ.items()
                                           if k != "LILYCO_UI"}).stdout)
    record("tui 裸跑（不带 --tui）降级 CLI", "magic_hex" in probe)
except Exception as error:
    record("tui 裸跑（不带 --tui）降级 CLI", False, str(error))


def drive(keys, settle=3.0):
    child = subprocess.Popen(["winpty", "-Xallow-non-tty", BIN, "--tui"], cwd=CWD,
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT,
                             env=dict(os.environ, TERM="xterm-256color", LILYCO_UI="tui"))
    seen = bytearray()
    stop = threading.Event()

    def pump():
        while not stop.is_set():
            try:
                chunk = child.stdout.read1(65536)
            except Exception:
                break
            if not chunk:
                break
            seen.extend(chunk)

    thread = threading.Thread(target=pump, daemon=True)
    thread.start()
    time.sleep(settle)
    for payload in ([b""] if not keys else keys):
        try:
            child.stdin.write(payload)
            child.stdin.flush()
        except Exception:
            break
        time.sleep(settle)
    # Esc 与 q 之间要留够间隔：贴着发会被 crossterm 当成一整个转义序列吞掉（实测 0.4s 会挂，1.2s 干净退出）
    try:
        child.stdin.write(chr(27).encode())
        child.stdin.flush()
        time.sleep(1.2)
        child.stdin.write(b"q" + chr(13).encode())
        child.stdin.flush()
    except Exception:
        pass
    time.sleep(1.2)
    stop.set()
    exited = True
    try:
        # 表单页要两次状态跳转才回到选择页，4 秒会误判成挂死（winpty 下的退出实测是干净的）
        child.wait(timeout=10)
    except subprocess.TimeoutExpired:
        exited = False
        child.kill()
    thread.join(timeout=2)
    raw = bytes(seen).decode("utf-8", "replace")
    return raw, CTRL.sub("", STRIP.sub("", raw)), exited


ESC = chr(27).encode()
CR = chr(13).encode()
raw1, screen1, out1 = drive([])
hit = [cmd for cmd in COMMANDS if cmd in screen1]
record("tui 选择页列出四条命令", len(hit) == 4, "%d/4 命中（捕获 %d 字节）" % (len(hit), len(raw1)))
record("tui 标题含 lbin", "lbin" in screen1)
raw2, screen2, out2 = drive([ESC + b"[B" + ESC + b"[B"])
record("tui ↓ 移动高亮后画面有变", screen2 != screen1, "尾部窗口差异")
raw3, screen3, out3 = drive([CR])
cues = [key for key in ["path", "max-bytes", "Run", "运行", "$", "About", "关于"] if key in screen3]
record("tui Enter 进表单并渲染字段/预览", bool(cues), str(cues))
record("tui Esc/q 干净退出（未被 kill）", out1 and out2 and out3,
       "选择页=%s / â=%s / 表单=%s" % ("退" if out1 else "挂", "退" if out2 else "挂",
                                  "退" if out3 else "挂"))

# ── 汇总 ──────────────────────────────────────────────────────────
passed = sum(1 for _, ok, _ in RESULTS if ok)
print("\n=== 汇总 ===")
print("%d/%d 通过" % (passed, len(RESULTS)))
for name, ok, detail in RESULTS:
    if not ok:
        print("  FAIL %s  %s" % (name, detail))
sys.exit(0 if passed == len(RESULTS) else 1)
