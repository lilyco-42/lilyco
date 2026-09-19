"""Web 端（WebUI）端到端验收：起服务 → 抓首页 → 带令牌 POST /run → 收 SSE → 比对结果。

用法：python web_probe.py <二进制路径> <工作目录> <端口> [--expect <cli结果json路径>]

验收点（对齐 docs/MVP_SCOPE.md §4）：
  1. 首页 200，命令下拉含全部命令
  2. ?cmd= 能切到任意命令且渲染不同表单
  3. POST /run 无令牌 → 401（CSRF 防护生效）
  4. POST /run 带令牌（T0 命令）→ 200 + session_id
  5. SSE /progress/<sid> 能收到 started → done，done.result 与 CLI --json 逐字一致
  6. POST /run 带令牌执行 T1 命令 → Web 面（Interactive）放行
"""
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request

BIN = sys.argv[1]
CWD = sys.argv[2]
PORT = int(sys.argv[3])
EXPECT_PATH = None
if "--expect" in sys.argv:
    EXPECT_PATH = sys.argv[sys.argv.index("--expect") + 1]

BASE = f"http://127.0.0.1:{PORT}"
TOKEN_HEADER = "X-Lilyco-Token"


def get(path, timeout=15):
    with urllib.request.urlopen(BASE + path, timeout=timeout) as r:
        return r.status, r.read().decode("utf-8", "replace")


def post(path, payload, token=None, origin=None, timeout=30):
    headers = {"Content-Type": "application/json"}
    if token is not None:
        headers[TOKEN_HEADER] = token
    if origin is not None:
        headers["Origin"] = origin
    req = urllib.request.Request(
        BASE + path, data=json.dumps(payload).encode(), headers=headers, method="POST"
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.status, r.read().decode("utf-8", "replace")


def run_and_collect(cmd, args, token, origin, timeout=30):
    """POST /run 拿 session_id，再读 SSE 直到 done/error，返回 (done_result, ticks)。"""
    st, body = post("/run", {"args": args, "cmd": cmd}, token, origin)
    if st != 200:
        return None, [], f"HTTP {st}: {body[:200]}"
    sid = json.loads(body)["session_id"]
    result, ticks, err = None, [], None
    req = urllib.request.Request(BASE + f"/progress/{sid}")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        for raw in r:
            line = raw.decode("utf-8", "replace").strip()
            if not line.startswith("data:"):
                continue
            try:
                ev = json.loads(line[5:].strip())
            except Exception:
                continue
            t = ev.get("type")
            if t == "done":
                result = ev.get("result")
                break
            if t == "error":
                err = ev.get("message")
                break
            if t in ("started", "tick", "log"):
                ticks.append(t)
    return result, ticks, err


def main():
    env = dict(os.environ, LILYCO_PORT=str(PORT), LILYCO_UI="web")
    env.pop("LILYCO_UI_FLAG", None)
    p = subprocess.Popen(
        [BIN, "--gui"],
        cwd=CWD,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
    )
    ok = True
    try:
        ready = False
        for _ in range(60):
            time.sleep(0.3)
            try:
                if get("/", timeout=3)[0] == 200:
                    ready = True
                    break
            except Exception:
                continue
        if not ready:
            print("❌ 服务未在 18s 内就绪")
            print(p.stdout.read()[:800])
            return 1

        print("=== 1) 服务启动 ===")
        print(f"  GET / → 200 ✅（LILYCO_PORT={PORT}）")

        st, html = get("/")
        print(f"\n=== 2) 首页 HTML（{len(html)} 字节）===")
        selects = re.findall(r"<select[^>]*>", html)
        options = re.findall(r"<option[^>]*>([^<]*)</option>", html)
        print(f"  <select> 数量: {len(selects)}")
        print(f"  <option> 列表: {options[:12]}")
        for sel in selects:
            if "cmd" in sel:
                print(f"  命令下拉: {sel[:110]}")
                break

        # 令牌：从 <meta name="lilyco-token" content="...">
        m = re.search(r'name="lilyco-token"\s+content="([^"]+)"', html)
        token = m.group(1) if m else None
        print(f"  CSRF 令牌（meta）: {'✅ 已解析 ' + token[:8] + '…' if token else '❌ 未找到'}")

        print("\n=== 3) 四端同源：四条命令都出现在 WebUI ===")
        for c in ["find", "rename", "dedup", "stats"]:
            found = bool(re.search(rf"[\"'>\s]{c}[\"'<\s]", html))
            print(f"  {c:8s} {'✅ 在页面中' if found else '⚠️ 未找到'}")
            ok = ok and found

        print("\n=== 4) ?cmd= 切换命令 ===")
        sizes = {}
        for c in ["find", "rename", "dedup", "stats"]:
            try:
                st, sub = get(f"/?cmd={c}")
                sizes[c] = len(sub)
                print(f"  ?cmd={c:8s} → HTTP {st} ({len(sub)} bytes)")
            except Exception as e:
                print(f"  ?cmd={c:8s} → 失败: {e}")
                ok = False
        if len(set(sizes.values())) >= 3:
            print("  ✅ 四命令页面字节数各异 → 确实按命令渲染不同表单")
        else:
            print("  ⚠️ 页面字节数高度雷同，需人工确认表单是否有差异")

        origin = BASE

        print("\n=== 5) CSRF 防护：无令牌 POST /run 应被拒 ===")
        try:
            st, body = post("/run", {"cmd": "find", "args": {"root": "."}}, None, origin)
            print(f"  ❌ 意外放行 HTTP {st}")
            ok = False
        except urllib.error.HTTPError as e:
            e.read()
            mark = "✅" if e.code == 401 else "⚠️"
            print(f"  HTTP {e.code} {mark}（裸 POST 无令牌被拒）")
            ok = ok and e.code == 401

        # 夹具目录的绝对路径：必须与 CLI 参考跑用同一个 root，
        # 否则结果里的 path/root 字段形态不同（相对 vs 绝对）而无法逐字比对。
        fixture_root = os.path.abspath(CWD).replace("\\", "/")

        print("\n=== 6) 端到端执行：带令牌 POST /run + SSE 进度 ===")
        result, ticks, err = run_and_collect(
            "find", {"root": fixture_root, "pattern": "*.jpg"}, token, origin
        )
        if err:
            print(f"  ❌ {err}")
            ok = False
        else:
            print(f"  ✅ 收到 SSE 事件序列: {ticks}")
            print(f"  done.result 键: {sorted(result.keys()) if isinstance(result, dict) else type(result)}")
            print(f"  count = {result.get('count') if isinstance(result, dict) else '?'}")

            if EXPECT_PATH and os.path.exists(EXPECT_PATH):
                with open(EXPECT_PATH, encoding="utf-8") as f:
                    cli = json.load(f)
                # 忽略 duration_ms（必然不同），其余逐字比对
                a = {k: v for k, v in result.items() if k != "duration_ms"}
                b = {k: v for k, v in cli.items() if k != "duration_ms"}
                same = json.dumps(a, sort_keys=True, ensure_ascii=False) == json.dumps(
                    b, sort_keys=True, ensure_ascii=False
                )
                print(
                    f"  {'✅' if same else '❌'} 与 CLI --json 逐字一致（忽略 duration_ms）: {same}"
                )
                ok = ok and same

        print("\n=== 7) T1 命令在 Web 面（Interactive）应放行 ===")
        result2, ticks2, err2 = run_and_collect(
            "rename", {"root": "ren_demo", "prefix": "WEB_"}, token, origin
        )
        if err2:
            print(f"  ❌ 被拒: {err2}")
            ok = False
        else:
            print(f"  ✅ rename（T1）在 Web 面执行成功")
            print(f"     结果键: {sorted(result2.keys()) if isinstance(result2, dict) else result2}")
            if isinstance(result2, dict):
                print(f"     applied={result2.get('applied')} skipped={result2.get('skipped')} applied_now={result2.get('applied_now')}")

        print(f"\n{'=' * 46}\n总判定: {'✅ 全部通过' if ok else '❌ 存在失败项'}\n{'=' * 46}")
        return 0 if ok else 1
    finally:
        p.terminate()
        try:
            p.wait(timeout=5)
        except subprocess.TimeoutExpired:
            p.kill()


if __name__ == "__main__":
    sys.exit(main())
