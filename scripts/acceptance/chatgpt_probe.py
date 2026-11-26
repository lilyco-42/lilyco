"""lchatgpt 四端验收探针：只读命令在 CLI / Web / MCP / TUI 四端拿到同一份 JSON。

用法：
    python scripts/acceptance/chatgpt_probe.py <lchatgpt 路径> [--offline]

为什么要 --offline：这个域的命令**真的会打 chatgpt.com**（凭证取 ~/.codex/auth.json）。
CI 里没有凭证也不该联网，所以：
  - 默认（在线）：先跑 CLI 的 whoami/usage/models --json 作基准，再逐端比对。
  - --offline：只验协议面 —— registry 形状（--schema）、Web 首屏/CSRF 401、
    MCP 握手 + tools/list 分级 + T1 拒绝 + 缺参 -32602。不发起任何外网请求。

验收点：
  1. --schema 里 10 条命令齐全，whoami/usage/models/list/show-conversation/search/auth 为 T0，
     ask / login / serve 为 T1
  2. CLI --json 基准（在线）；auth 是本地体检，--offline 也照跑；
     另有一条不分在线离线的断言：ask 默认只走 chat 额度 —— 把 --cdp 指向死端口时
     必须失败、报错里要有 --allow-codex，且绝不能碰 codex/responses
  3. serve：起 OpenAI 兼容端（本地环回，不联网）→ /v1/models 只有 lchatgpt:chat、
     /health 自报 quota=chat、要 lchatgpt:codex 被拒且报错指路 --allow-codex、不认识模型 400
  4. Web：首页 10 条命令 → 裸 POST /run 401 → 带令牌 whoami → SSE done.result 与 CLI 逐字一致
  5. MCP：initialize → tools/list（ask 的 description 带 T1）→ tools/call whoami 放行
     → tools/call ask、login 被安全门拒绝（DenyElevated）→ whoami 缺参不会发生（零参数），
     改用 show-conversation 缺 conversation_id 验 -32602
  6. TUI：--tui 能起、能列命令、q 退出（无 winpty 时降级跳过）
"""
import json
import os
import re
import select
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request

BIN = sys.argv[1] if len(sys.argv) > 1 else "target/debug/lchatgpt"
OFFLINE = "--offline" in sys.argv
PORT = 18093
BASE = f"http://127.0.0.1:{PORT}"
TOKEN_HEADER = "X-Lilyco-Token"

# T0 只读；auth 只摸本地文件与 env，不联网，所以 --offline 也能验
READS = ["whoami", "usage", "models", "list", "show-conversation", "search", "auth"]
# T1 confirm：ask 会打远端、login 会落盘凭证、serve 会把端口对外开
WRITES = ["ask", "login", "serve"]
ALL = sorted(READS + WRITES)
fails = []


ANSI = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]")


def strip_ansi(s):
    return ANSI.sub("", s)


def ok(msg):
    print(f"  \u2705 {msg}")


def bad(msg):
    fails.append(msg)
    print(f"  \u274c {msg}")


def run_cli(args, timeout=120):
    p = subprocess.run(
        [BIN] + args, capture_output=True, text=True, encoding="utf-8", timeout=timeout
    )
    return p.returncode, p.stdout, p.stderr


def get(path, timeout=20):
    with urllib.request.urlopen(BASE + path, timeout=timeout) as r:
        return r.status, r.read().decode("utf-8", "replace")


def post(path, payload, token=None, timeout=60):
    headers = {"Content-Type": "application/json"}
    if token is not None:
        headers[TOKEN_HEADER] = token
    req = urllib.request.Request(
        BASE + path, data=json.dumps(payload).encode(), headers=headers, method="POST"
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", "replace")


def mcp_call(reqs, expect_ids, timeout=90.0):
    """MCP 面：写完立刻关 stdin，然后轮询 stdout 直到期望 id 到齐（同 mcp_probe.py 的时序坑）"""
    inp = "\n".join(json.dumps(r) for r in reqs) + "\n"
    p = subprocess.Popen(
        [BIN, "--mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    p.stdin.write(inp)
    p.stdin.flush()
    p.stdin.close()
    want = set(expect_ids)
    got = []
    deadline = time.time() + timeout
    while time.time() < deadline:
        line = p.stdout.readline()
        if not line:
            if p.poll() is not None:
                break
            time.sleep(0.02)
            continue
        line = line.strip()
        if not line:
            continue
        try:
            got.append(json.loads(line))
        except json.JSONDecodeError:
            pass
        if want.issubset({m.get("id") for m in got}):
            break
    time.sleep(0.4)
    try:
        rest, _ = p.communicate(timeout=10)
    except subprocess.TimeoutExpired:
        p.kill()
        rest, _ = p.communicate()
    for line in (rest or "").splitlines():
        line = line.strip()
        if line:
            try:
                got.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return got


def check_schema():
    print("=== 1) registry 形状（--schema）===")
    code, out, err = run_cli(["--schema"])
    if code != 0:
        bad(f"--schema 退出码 {code}: {err[:200]}")
        return
    reg = json.loads(out)
    # --schema 导出的是 RegisteredCommand 数组：[{name, aliases, hidden, schema:{name,about,args,safety}}]
    cmds = reg["commands"] if isinstance(reg, dict) else reg
    names = sorted(c["name"] for c in cmds)
    if names == ALL:
        ok(f"{len(ALL)} 条命令齐全: {names}")
    else:
        bad(f"命令集合不符: {names} != {ALL}")
    tiers = {c["name"]: c.get("schema", c).get("safety", "read_only") for c in cmds}
    wrong = [
        n for n in READS if tiers.get(n) not in ("read_only", "ReadOnly", "t0")
    ]
    if wrong:
        bad(f"这些只读命令的分级不是 T0: {wrong}")
    else:
        ok(f"{len(READS)} 条只读命令均为 T0")
    for name, why in (
        ("ask", "会写远端服务"),
        ("login", "会落盘凭证"),
        ("serve", "会把端口对外开"),
    ):
        if tiers.get(name) in ("confirm", "Confirm", "t1"):
            ok(f"{name} 为 T1（{why}）")
        else:
            bad(f"{name} 的分级应为 T1，实际 {tiers.get(name)}")


def check_default_quota():
    """P0：ask 默认只能花 chat 额度；codex 额度必须 --allow-codex 显式打开。

    这里把 --cdp 指到一个死端口，保证「没有浏览器」这条路上的行为被测到，
    同时绝不会真的把消息发出去（不发消息 = 不花任何一份额度）。
    """
    print("  -- ask 的默认额度 --")
    code, out, err = run_cli(["ask", "--prompt", "ping", "--cdp", "127.0.0.1:1", "--json"])
    if code == 0:
        bad(f"没有浏览器时 ask 竟然成功了: {out[:160]}")
        return
    text = (err or "") + (out or "")
    if "--allow-codex" in text:
        ok("ask 默认走 chat 额度：没有浏览器就报错，并指路 --allow-codex")
    else:
        bad(f"ask 的报错没告诉用户可以加 --allow-codex: {text.strip()[:200]}")
    if "codex/responses" in text:
        bad("ask 在没勾选的情况下就打到了 codex/responses")
    else:
        ok("没勾选时没有碰 codex/responses（没花 codex 额度）")


def check_serve():
    """serve 是 OpenAI 兼容端，本身不会替我们回答问题（那要花额度），
    所以这里只验协议面：它肯起来、模型清单对、没勾选时 codex 那条路是关着的。"""
    print("\n=== 3) serve（OpenAI 兼容端）===")
    port = PORT + 1
    p = subprocess.Popen(
        [BIN, "serve", "--port", str(port)],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
    )
    base = f"http://127.0.0.1:{port}"

    def probe(path, payload=None):
        url = base + path
        data = None if payload is None else json.dumps(payload).encode()
        headers = {"Content-Type": "application/json"}
        req = urllib.request.Request(url, data=data, headers=headers, method=None if data is None else "POST")
        try:
            with urllib.request.urlopen(req, timeout=15) as r:
                return r.status, r.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            return e.code, e.read().decode("utf-8", "replace")
        except Exception as e:  # noqa: BLE001
            return None, str(e)

    try:
        st, body = None, ""
        for _ in range(80):
            st, body = probe("/v1/models")
            if st == 200:
                break
            time.sleep(0.25)
        if st != 200:
            bad(f"serve 的 /v1/models 起不来: {st} {body[:200]}")
            return
        ids = [m["id"] for m in json.loads(body)["data"]]
        if ids == ["lchatgpt:chat"]:
            ok(f"只露出 chat 模型: {ids}")
        else:
            bad(f"没勾选 --allow-codex 却有这些模型: {ids}")
        st, body = probe("/health")
        health = json.loads(body) if st == 200 else {}
        if health.get("ok") and health.get("quota") == "chat":
            ok("健康检查自报 quota=chat")
        else:
            bad(f"/health 不符: {st} {body[:200]}")
        msg = {"messages": [{"role": "user", "content": "ping"}], "model": "lchatgpt:codex"}
        st, body = probe("/v1/chat/completions", msg)
        text = body if isinstance(body, str) else ""
        if st == 400 and "--allow-codex" in text:
            ok("没勾选时 lchatgpt:codex 被拒，报错指路 --allow-codex")
        else:
            bad(f"codex 模型应 400 并提 --allow-codex，实际 {st} {text[:200]}")
        st, body = probe(
            "/v1/chat/completions",
            {"messages": [{"role": "user", "content": "ping"}], "model": "gpt-4o"},
        )
        if st == 400 and "lchatgpt" in body:
            ok("不认识模型回 400 并列出本端提供哪些")
        else:
            bad(f"未知模型应 400，实际 {st} {body[:200]}")
        st, body = probe("/v1/chat/completions", {"model": "lchatgpt:chat", "messages": []})
        if st == 400:
            ok("空 messages 回 400")
        else:
            bad(f"空 messages 应 400，实际 {st} {body[:200]}")

        # 生图端：这里只验拒绝路径（真生成要花一次 chat 额度，留给在线人工验）
        st, body = probe("/v1/images/generations", {})
        if st == 400 and "prompt" in body:
            ok("生图缺 prompt 回 400")
        else:
            bad(f"生图缺 prompt 应 400，实际 {st} {body[:200]}")
        st, body = probe(
            "/v1/images/generations", {"prompt": "a cat", "response_format": "url"}
        )
        if st == 400 and "b64_json" in body:
            ok("生图只回 b64_json，response_format=url 被拒并说明原因")
        else:
            bad(f"response_format=url 应 400 并提 b64_json，实际 {st} {body[:200]}")
    finally:
        p.terminate()
        try:
            p.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            p.kill()
            p.communicate()

def cli_baseline():
    print("\n=== 2) CLI 基准（--json）===")
    code, out, err = run_cli(["auth", "--json"])
    if code != 0:
        bad(f"auth（本地体检）失败: {err.strip()[:200]}")
    else:
        a = json.loads(out)
        if "candidates" in a and isinstance(a["candidates"], list):
            ok(f"auth 列出 {len(a['candidates'])} 条候选凭证")
        else:
            bad(f"auth 结果里没有 candidates: {sorted(a)[:6]}")
        tok = (a.get("resolved") or {}).get("token", "")
        if tok == "" or ("…" in tok and len(tok) <= 12):
            ok(f"auth 里的 token 已打码: {tok or '(无凭证)'}")
        else:
            bad(f"auth 里的 token 没打码: {tok[:20]}")
    check_default_quota()
    if OFFLINE:
        print("  （--offline：跳过所有外网命令）")
        return {}
    base = {}
    for cmd in ("whoami", "usage", "models"):
        code, out, err = run_cli([cmd, "--json"])
        if code != 0:
            bad(f"{cmd} 失败: {err.strip()[:200]}")
            continue
        base[cmd] = json.loads(out)
        ok(f"{cmd} 拿到 {sorted(base[cmd].keys())[:4]} …")
    if "whoami" in base:
        tok = base["whoami"].get("token", "")
        if "…" in tok and len(tok) <= 12:
            ok(f"token 已打码: {tok}")
        else:
            bad(f"token 没打码: {tok[:20]}")
    return base


def check_web(base):
    print("\n=== 4) Web 面 ===")
    env = dict(os.environ, LILYCO_PORT=str(PORT), LILYCO_UI="web")
    p = subprocess.Popen(
        [BIN, "--gui"],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
    )
    try:
        html, token = "", None
        for _ in range(120):
            try:
                st, html = get("/")
                if st == 200:
                    break
            except Exception:
                pass
            time.sleep(0.25)
        if not html:
            bad("Web 首屏起不来")
            return
        ok("首页 200")
        missing = [c for c in ALL if c not in html]
        if missing:
            bad(f"首页下拉缺命令: {missing}")
        else:
            ok(f"首页含全部 {len(ALL)} 条命令")
        marker = 'name="lilyco-token" content="'
        i = html.find(marker)
        if i < 0:
            bad("首页没有 lilyco-token（CSRF 令牌）")
            return
        token = html[i + len(marker) : html.find('"', i + len(marker))]
        st, body = post("/run", {"cmd": "whoami", "args": {}}, token=None)
        if st == 401:
            ok("裸 POST /run 被 401 拦下（CSRF 生效）")
        else:
            bad(f"裸 POST /run 应 401，实际 {st}")
        if OFFLINE:
            print("  （--offline：不跑 whoami 的 SSE 比对）")
            return
        st, body = post("/run", {"cmd": "whoami", "args": {}}, token=token)
        if st != 200:
            bad(f"带令牌 POST /run → {st}: {body[:200]}")
            return
        sid = json.loads(body)["session_id"]
        result, err = None, None
        with urllib.request.urlopen(f"{BASE}/progress/{sid}", timeout=90) as r:
            for raw in r:
                line = raw.decode("utf-8", "replace").strip()
                if not line.startswith("data:"):
                    continue
                try:
                    ev = json.loads(line[5:].strip())
                except Exception:
                    continue
                if ev.get("type") == "done":
                    result = ev.get("result")
                    break
                if ev.get("type") == "error":
                    err = ev.get("message")
                    break
        if err:
            bad(f"whoami SSE error: {err[:200]}")
            return
        if result is None:
            bad("whoami 没收到 done")
            return
        ok("whoami SSE 收到 done")
        expect = base.get("whoami")
        if expect:
            a = {k: v for k, v in result.items() if k != "duration_ms"}
            b = {k: v for k, v in expect.items() if k != "duration_ms"}
            if a == b:
                ok("Web done.result 与 CLI --json 逐字一致")
            else:
                bad(f"Web 与 CLI 不一致: 仅 CLI 有 {set(b) - set(a)}，仅 Web 有 {set(a) - set(b)}")
    finally:
        p.terminate()
        try:
            p.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            p.kill()
            p.communicate()


def check_mcp():
    print("\n=== 5) MCP 面 ===")
    init = {"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2024-11-05", "capabilities": {}}}
    lst = {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}
    got = mcp_call([init, lst], [1, 2])
    tools = []
    for m in got:
        if m.get("id") == 2:
            tools = m["result"]["tools"]
    names = sorted(t["name"] for t in tools)
    if names == ALL:
        ok(f"tools/list {len(tools)} 个工具: {names}")
    else:
        bad(f"tools/list 不符: {names}")
    ask = [t for t in tools if t["name"] == "ask"]
    if ask and "T1" in ask[0]["description"]:
        ok("ask 的工具描述自报 T1")
    elif ask:
        bad("ask 的描述里没有 T1")
    if OFFLINE:
        print("  （--offline：跳过 whoami 的 tools/call）")
    else:
        call = {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "whoami", "arguments": {}}}
        got = mcp_call([init, lst, call], [1, 2, 3])
        r3 = [m for m in got if m.get("id") == 3][-1]
        if "error" in r3:
            bad(f"whoami（T0）被拒: {json.dumps(r3['error'], ensure_ascii=False)[:200]}")
        else:
            data = json.loads(r3["result"]["content"][0]["text"])
            ok(f"whoami（T0）放行: plan={data.get('plan_type')}")
    denied = {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
              "params": {"name": "ask", "arguments": {"prompt": "hi"}}}
    got = mcp_call([init, lst, denied], [1, 2, 4])
    r4 = [m for m in got if m.get("id") == 4][-1]
    text = json.dumps(r4, ensure_ascii=False)
    if "T1" in text or "拒绝" in text or "denied" in text.lower():
        ok("ask（T1）被安全门拒绝，MCP 面不能发消息")
    else:
        bad(f"ask（T1）未被拒绝: {text[:240]}")
    denied_login = {"jsonrpc": "2.0", "id": 6, "method": "tools/call",
                    "params": {"name": "login", "arguments": {"from": "browser"}}}
    got = mcp_call([init, lst, denied_login], [1, 2, 6])
    r6 = [m for m in got if m.get("id") == 6][-1]
    text = json.dumps(r6, ensure_ascii=False)
    if "T1" in text or "拒绝" in text or "denied" in text.lower():
        ok("login（T1）被安全门拒绝，MCP 面不能替换凭证")
    else:
        bad(f"login（T1）未被拒绝: {text[:240]}")
    denied_serve = {"jsonrpc": "2.0", "id": 7, "method": "tools/call",
                    "params": {"name": "serve", "arguments": {}}}
    got = mcp_call([init, lst, denied_serve], [1, 2, 7])
    r7 = [m for m in got if m.get("id") == 7][-1]
    text = json.dumps(r7, ensure_ascii=False)
    if "T1" in text or "拒绝" in text or "denied" in text.lower():
        ok("serve（T1）被安全门拒绝，MCP 面不能开监听端口")
    else:
        bad(f"serve（T1）未被拒绝: {text[:240]}")
    missing = {"jsonrpc": "2.0", "id": 5, "method": "tools/call",
               "params": {"name": "show-conversation", "arguments": {}}}
    got = mcp_call([init, lst, missing], [1, 2, 5])
    r5 = [m for m in got if m.get("id") == 5][-1]
    code = (r5.get("error") or {}).get("code")
    if code == -32602:
        ok("缺 conversation_id 被 validate_args 拦下（-32602）")
    else:
        bad(f"缺参应回 -32602，实际 {json.dumps(r5, ensure_ascii=False)[:200]}")

    # 协议版本协商 + resources/*：dsh 的 MCP 客户端只有在 capabilities
    # 里看到 resources 才会真的发 resources/list，所以这两条必须验。
    init_new = {"jsonrpc": "2.0", "id": 8, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {}}}
    rlist = {"jsonrpc": "2.0", "id": 9, "method": "resources/list", "params": {}}
    rread = {"jsonrpc": "2.0", "id": 10, "method": "resources/read",
             "params": {"uri": "lilyco://command/models"}}
    got = mcp_call([init_new, rlist, rread], [8, 9, 10])
    by_id = {}
    for m in got:
        if m.get("id") is not None:
            by_id[m["id"]] = m
    r8 = by_id.get(8, {})
    ver = (r8.get("result") or {}).get("protocolVersion")
    caps = (r8.get("result") or {}).get("capabilities") or {}
    if ver == "2025-06-18":
        ok("协议版本按客户端请求协商（2025-06-18 原样回）")
    else:
        bad(f"协议版本协商失败，回了 {ver!r}")
    if "resources" in caps and "tools" in caps:
        ok("capabilities 同时声明 tools 与 resources")
    else:
        bad(f"capabilities 缺键: {sorted(caps)}")
    r9 = by_id.get(9, {})
    resources = (r9.get("result") or {}).get("resources") or []
    uris = sorted(r.get("uri", "") for r in resources)
    if len(resources) == len(ALL) and all(
        u.startswith("lilyco://command/") for u in uris
    ):
        ok(f"resources/list 把 {len(resources)} 条命令当资源暴露")
    else:
        bad(f"resources/list 不符（{len(resources)} 条）: {uris[:5]}")
    r10 = by_id.get(10, {})
    contents = (r10.get("result") or {}).get("contents") or []
    if contents and "models" in (contents[0].get("text") or ""):
        ok("resources/read 读回命令 schema JSON")
    else:
        bad(f"resources/read 不符: {json.dumps(r10, ensure_ascii=False)[:200]}")


def check_tui():
    print("\n=== 6) TUI 面 ===")
    """用 stdlib pty 起真终端。必须 ioctl 设窗口尺寸：不设就是 0x0，
    ratatui 会把列表挤成一列甚至什么都不画（踩过：只收到 783 字节）。
    容器/无 pty 环境直接跳过，不制造假失败。"""
    try:
        import fcntl
        import pty
        import struct
        import termios
    except ImportError:
        print("  \u26a0\ufe0f 缺 pty/fcntl，TUI 面跳过")
        return
    try:
        master, slave = pty.openpty()
    except OSError as e:
        print(f"  \u26a0\ufe0f 起不了 pty，TUI 面跳过: {e}")
        return
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
    env = dict(os.environ, TERM="xterm-256color")
    p = subprocess.Popen(
        [BIN, "--tui"],
        env=env,
        stdin=slave,
        stdout=slave,
        stderr=slave,
        preexec_fn=os.setsid,
    )
    os.close(slave)
    buf = b""
    try:
        deadline = time.time() + 15
        while time.time() < deadline:
            r, _, _ = select.select([master], [], [], 0.3)
            if not r:
                if p.poll() is not None:
                    break
                continue
            try:
                buf += os.read(master, 65536)
            except OSError:
                break
            plain = strip_ansi(buf.decode("utf-8", "replace"))
            if "whoami" in plain and "search" in plain:
                break
        plain = strip_ansi(buf.decode("utf-8", "replace"))
        if "whoami" in plain and "search" in plain:
            ok("--tui 落到选择页并列出命令")
        else:
            bad(f"--tui 没有渲染出命令列表（收到 {len(buf)} 字节）")
        os.write(master, b"q")
        try:
            p.wait(timeout=8)
            ok("q 干净退出")
        except subprocess.TimeoutExpired:
            bad("q 之后 8 秒没退出")
    except Exception as e:  # noqa: BLE001
        print(f"  \u26a0\ufe0f TUI 面跳过: {e}")
    finally:
        if p.poll() is None:
            p.kill()
        try:
            p.wait(timeout=5)
        except Exception:
            pass
        os.close(master)


def main():
    if not shutil.which("python3") and sys.version_info < (3, 8):
        print("需要 python3.8+")
        return 1
    print(f"binary: {BIN}  offline: {OFFLINE}")
    check_schema()
    base = cli_baseline()
    check_serve()
    check_web(base)
    check_mcp()
    check_tui()
    print("\n" + "=" * 60)
    if fails:
        print(f"\u274c {len(fails)} 项失败：")
        for f in fails:
            print("  -", f)
        return 1
    print("\u2705 四端验收全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
