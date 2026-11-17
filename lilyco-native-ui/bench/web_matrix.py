#!/usr/bin/env python3
"""Web 全变体功能矩阵 + 交互状态 + 异常/断网 + 首帧性能。"""
import json, os, statistics, subprocess, sys, time, urllib.request
from pathlib import Path
HERE = Path(__file__).resolve().parent
FIX = HERE / "fixtures"
GEN = FIX / ".generated"
CDP = str(HERE / "cdp_matrix.js")
PORT = int(os.environ.get("MATRIX_PORT", "9361"))
RUNS = int(os.environ.get("MATRIX_RUNS", "3"))
OUT = Path(os.environ.get("MATRIX_OUT", str(HERE / "results/web-all13-2026-11-15.json")))
LOGDIR = Path("/tmp/lilyco-matrix-logs"); LOGDIR.mkdir(exist_ok=True)

GROUPS = [
    ("fixed_online", str(FIX / "all-13-react-fixed.html"), "online", False, RUNS),
    ("offline_vendored", str(GEN / "all-13-react-offline.html"), "offline", False, RUNS),
    ("cdn_blocked", str(FIX / "all-13-react-fixed.html"), "block", True, RUNS),
    ("old_unpinned", str(FIX / "all-13-react-old-unpinned.html"), "old", False, 1),
]

def cdp_ready():
    try:
        urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version", timeout=0.2).read()
        return True
    except Exception:
        return False

def chrome_rss(profile):
    total = 0
    for q in Path("/proc").iterdir():
        if not q.name.isdigit(): continue
        try:
            if profile not in q.joinpath("cmdline").read_bytes().decode(errors="ignore"): continue
            for line in q.joinpath("status").read_text().splitlines():
                if line.startswith("VmRSS:"): total += int(line.split()[1]) * 1024; break
        except OSError: pass
    return total

def one(group, html, node_mode, blocked, i):
    tag = f"{group}-{i}"
    profile = f"/tmp/lilyco-matrix-chrome-{tag}"
    subprocess.run(["rm", "-rf", profile])
    log = open(LOGDIR / f"chrome-{tag}.log", "w")
    args = ["chromium", "--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
            "--no-first-run", "--no-default-browser-check", f"--user-data-dir={profile}",
            f"--remote-debugging-port={PORT}", "--window-size=640,820", "about:blank"]
    t0 = time.perf_counter()
    p = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT)
    ready = None
    for _ in range(240):
        if cdp_ready(): ready = (time.perf_counter() - t0) * 1000; break
        time.sleep(0.05)
    if ready is None:
        p.kill(); log.close(); return {"error": "CDP never ready"}
    cdp_out = f"/tmp/lilyco-matrix-{tag}.json"
    env = dict(os.environ); env["CDP_PORT"] = str(PORT)
    r = subprocess.run(["node", CDP, node_mode, cdp_out, "file://" + html],
                       capture_output=True, text=True, timeout=180, env=env)
    rss = chrome_rss(profile)
    p.terminate()
    try: p.wait(timeout=3)
    except subprocess.TimeoutExpired: p.kill(); p.wait()
    log.close()
    if r.returncode: return {"error": (r.stdout + r.stderr)[:600], "stdout": r.stdout.strip()}
    d = json.loads(Path(cdp_out).read_text())
    d.update({"browser_ready_ms": ready, "chrome_tree_rss_mb": round(rss / 1048576, 2) if rss else None,
              "stdout": r.stdout.strip(), "blocked": blocked})
    return d

def stats(vals):
    vals = [v for v in vals if isinstance(v, (int, float))]
    if not vals: return None
    return {"n": len(vals), "min": round(min(vals),2), "median": round(statistics.median(vals),2),
            "max": round(max(vals),2), "mean": round(statistics.mean(vals),2)}

def summarize(group, runs):
    ok = [r for r in runs if "results" not in r and "error" not in r]
    rendered = sum(1 for r in ok if (r.get("snap") or {}).get("elCount", 0) > 0)
    return {
        "runs": len(runs),
        "errors": [r.get("error") for r in runs if "error" in r],
        "rendered": rendered,
        "matrixPass": stats([r.get("matrixPass") for r in ok]),
        "matrixTotal": next((r.get("matrixTotal") for r in ok if r.get("matrixTotal")), 14),
        "firstRenderMs": stats([r.get("firstRenderMs") for r in ok]),
        "browserReadyMs": stats([r.get("browser_ready_ms") for r in ok]),
        "coldStartMs": stats([(r.get("firstRenderMs") + r.get("browser_ready_ms"))
                              for r in ok if r.get("firstRenderMs") is not None and r.get("browser_ready_ms") is not None]),
        "clickLatencyMs": stats([(r.get("interaction") or {}).get("firstClick", {}).get("latency")
                                 for r in ok if ((r.get("interaction") or {}).get("firstClick") or {}).get("ok")]),
        "secondClickCarriesState": sum(1 for r in ok if ((r.get("interaction") or {}).get("secondClick") or {}).get("ok")),
        "chromeTreeRssMb": stats([r.get("chrome_tree_rss_mb") for r in ok]),
        "jsHeapMb": stats([(r.get("metrics") or {}).get("JSHeapUsedSize", 0) / 1048576 for r in ok]),
        "externalRequests": stats([(r.get("net") or {}).get("external") for r in ok]),
        "networkFailures": stats([(r.get("net") or {}).get("failures") for r in ok]),
        "exceptions": sum(len(r.get("exceptions", [])) for r in ok),
        "windowClasses": sorted(set(str(r.get("windowClass")) for r in ok)),
    }

def main():
    for _, path, _, _, _ in GROUPS:
        if not Path(path).exists(): sys.exit("MISSING " + path + "；先运行 make_all13_fixtures.py")
    out = {"browser": None, "runs_per_group": RUNS, "groups": {}, "raw": {}}
    for group, html, node_mode, blocked, count in GROUPS:
        runs = [one(group, html, node_mode, blocked, i) for i in range(count)]
        out["raw"][group] = runs
        out["groups"][group] = summarize(group, runs)
        if out["browser"] is None:
            out["browser"] = next((r.get("browser") for r in runs if r.get("browser")), None)
        print("GROUP", group, json.dumps(out["groups"][group], ensure_ascii=False)[:600], flush=True)
    OUT.parent.mkdir(exist_ok=True)
    OUT.write_text(json.dumps(out, ensure_ascii=False, indent=2))
    print("WEB_MATRIX_OUT", OUT)
if __name__ == "__main__":
    main()
