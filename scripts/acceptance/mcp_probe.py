"""MCP 端验收探针：直接对 lfiles --mcp 说 JSON-RPC。

用法：python mcp_probe.py <二进制路径> <工作目录>
"""
import json
import subprocess
import sys
import time

BIN = sys.argv[1]
CWD = sys.argv[2]


def call(reqs, expect_ids=None, timeout=15.0):
    """发一批请求并收集响应。

    注意：lilyco-mcp 把 `tools/call` 丢到 worker 线程异步执行（为了支持
    handler 反向发起 sampling/roots）。如果写完就立刻关 stdin，主循环读到
    EOF 会直接退出、worker 来不及写响应。

    所以这里 **写完立刻关 stdin**（让主循环能正常收尾），然后**轮询 stdout**
    直到期望的 id 都到齐或超时 —— 这样既不依赖猜测 sleep 时长，也不会漏响应。
    """
    inp = "\n".join(json.dumps(r) for r in reqs) + "\n"
    p = subprocess.Popen(
        [BIN, "--mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        cwd=CWD,
        encoding="utf-8",
    )
    p.stdin.write(inp)
    p.stdin.flush()
    p.stdin.close()

    want = set(expect_ids if expect_ids is not None else [r.get("id") for r in reqs if r.get("id")])
    got, err = [], ""
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
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        got.append(msg)
        if want.issubset({m.get("id") for m in got}):
            break

    # 收尾：给 worker 一点时间，然后读残余
    time.sleep(0.4)
    try:
        rest, err = p.communicate(timeout=10)
    except subprocess.TimeoutExpired:
        p.kill()
        rest, err = p.communicate()
    for line in (rest or "").splitlines():
        line = line.strip()
        if line:
            try:
                got.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return got, err


def main():
    reqs = [
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"protocolVersion": "2024-11-05", "capabilities": {}},
        },
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    ]
    res, err = call(reqs, expect_ids=[1, 2])
    print("=== 1) initialize ===")
    print(json.dumps(res[0], ensure_ascii=False)[:400])

    tools = res[1]["result"]["tools"]
    print(f"\n=== 2) tools/list: {len(tools)} 个工具 ===")
    for t in tools:
        desc = t["description"]
        tier = "T0" if "T0" in desc else ("T1" if "T1" in desc else "?")
        params = list(t["inputSchema"].get("properties", {}).keys())
        print(f"  {t['name']:8s} tier={tier}  params={params}")

    # 3) tools/call：T0 的 find 应放行
    print("\n=== 3) tools/call find（T0，应放行）===")
    res2, err2 = call(
        reqs
        + [
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "find", "arguments": {"root": ".", "pattern": "*.jpg"}},
            }
        ],
        expect_ids=[1, 2, 3],
    )
    call_res = [r for r in res2 if r.get("id") == 3][-1]
    if "error" in call_res:
        print("  ERROR:", json.dumps(call_res["error"], ensure_ascii=False)[:300])
    else:
        content = call_res["result"]["content"][0]["text"]
        data = json.loads(content)
        print(f"  count={data['count']} total={data['total_size_human']}")
        print(f"  isError={call_res['result'].get('isError')}")

    # 4) tools/call：T1 的 rename 应被安全门拒绝
    print("\n=== 4) tools/call rename（T1，应被拒）===")
    res3, err3 = call(
        reqs
        + [
            {
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "rename",
                    "arguments": {"root": ".", "prefix": "X_", "apply": True},
                },
            }
        ],
        expect_ids=[1, 2, 4],
    )
    cr = [r for r in res3 if r.get("id") == 4][-1]
    text = json.dumps(cr, ensure_ascii=False)
    if "T1" in text or "拒绝" in text:
        print("  ✅ 被安全门拒绝:", text[:280])
    else:
        print("  ⚠️ 未被拒绝:", text[:280])

    # 5) 缺参校验：find 不给 root 应被 validate_args 拦下
    print("\n=== 5) tools/call find 缺 root（应被 validate_args 拦下）===")
    res4, err4 = call(
        reqs
        + [
            {
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {"name": "find", "arguments": {}},
            }
        ],
        expect_ids=[1, 2, 5],
    )
    r5 = [r for r in res4 if r.get("id") == 5][-1]
    print(" ", json.dumps(r5, ensure_ascii=False)[:300])


if __name__ == "__main__":
    main()
