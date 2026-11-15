#!/usr/bin/env python3
"""A/B: React/CDN 页面的启动、稳态、断网稳定性与交互延迟。"""
import json, os, statistics, subprocess, sys, time, urllib.request
from pathlib import Path
HERE = Path(__file__).resolve().parent
FIXED = str(HERE / "fixtures/react-fixed.html")
OLD = str(HERE / "fixtures/react-old-unpinned.html")
OFFLINE = str(HERE / "fixtures/.generated/react-offline.html")
CDP_JS = str(HERE / "cdp_bench.js")
PORT = 9351
OUT = sys.argv[1] if len(sys.argv) > 1 else "/tmp/bench-web.json"
RUNS = int(os.environ.get("BENCH_RUNS", "3"))
LOGDIR = Path("/tmp/lilyco-bench-logs"); LOGDIR.mkdir(exist_ok=True)

def run_chrome(mode, url, i, blocked=False):
    prof = f"/tmp/lilyco-chrome-{mode}-{i}"
    subprocess.run(["rm", "-rf", prof])
    log = open(LOGDIR / f"chrome-{mode}-{i}.log", "w")
    args = ["chromium", "--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
           "--no-first-run", "--no-default-browser-check", f"--user-data-dir={prof}",
           f"--remote-debugging-port={PORT}", "--window-size=640,820", "about:blank"]
    t0 = time.perf_counter()
    p = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT)
    ready = None
    for _ in range(200):
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version", timeout=0.2).read()
            ready = (time.perf_counter() - t0) * 1000; break
        except Exception: time.sleep(0.05)
    if ready is None:
        p.kill(); log.close(); return {"error": "CDP never ready"}
    out = f"/tmp/lilyco-cdp-{mode}-{i}.json"
    node_mode = "block" if blocked else mode
    r = subprocess.run(["node", CDP_JS, node_mode, out, "file://" + url],
                       capture_output=True, text=True, timeout=180)
    rss = 0
    for q in Path("/proc").iterdir():
        if not q.name.isdigit(): continue
        try:
            cmd = q.joinpath("cmdline").read_bytes().decode(errors="ignore")
            if prof in cmd:
                for line in q.joinpath("status").read_text().splitlines():
                    if line.startswith("VmRSS:"): rss += int(line.split()[1]) * 1024; break
        except OSError: pass
    p.terminate()
    try: p.wait(timeout=3)
    except subprocess.TimeoutExpired: p.kill(); p.wait()
    log.close()
    if r.returncode: return {"error": r.stdout + r.stderr, "stdout": r.stdout}
    d = json.loads(Path(out).read_text())
    d["browser_ready_ms"] = ready
    d["chrome_tree_rss_bytes"] = rss
    d["stdout"] = r.stdout.strip()
    return d

def stat(vals):
    vals = [v for v in vals if v is not None]
    if not vals: return None
    return {"n": len(vals), "min": round(min(vals), 2), "median": round(statistics.median(vals), 2),
            "max": round(max(vals), 2), "mean": round(statistics.mean(vals), 2)}

def flat(groups, key):
    out = []
    for g in groups:
        if "results" not in g: continue
        for r in g["results"]: out.append(r.get(key))
    return out

def nested(groups, keys):
    out = []
    for g in groups:
        if "results" not in g: continue
        for r in g["results"]:
            v = r
            for k in keys: v = (v or {}).get(k) if isinstance(v, dict) else None
            out.append(v)
    return out

def cold(groups):
    out = []
    for g in groups:
        if "results" not in g: continue
        for r in g["results"]:
            v = r.get("firstRenderMs")
            if v is not None: out.append(v + g.get("browser_ready_ms", 0))
    return out

def summarize(name, groups):
    rendered = sum(1 for g in groups for r in g.get("results", []) if (r.get("snap") or {}).get("els", 0) > 0)
    exceptions = sum(len(r.get("exceptions", [])) for g in groups for r in g.get("results", []))
    return {
        "runs": len(groups), "rendered": rendered, "exceptions": exceptions,
        "browser_ready_ms": stat([g.get("browser_ready_ms") for g in groups]),
        "nav_to_render_ms": stat(flat(groups, "firstRenderMs")),
        "cold_start_ms": stat(cold(groups)),
        "click_latency_ms": stat(flat(groups, "clickLatency")),
        "chrome_tree_rss_mb": stat([g.get("chrome_tree_rss_bytes", 0) / 1048576 or None for g in groups]),
        "js_heap_mb": stat([v / 1048576 if v else None for v in nested(groups, ["metrics", "JSHeapUsedSize"])]),
        "external_requests": stat(nested(groups, ["net", "external"])),
        "network_failures": stat(nested(groups, ["net", "failures"])),
    }

def main():
    for path in (FIXED, OLD, OFFLINE, CDP_JS):
        if not Path(path).exists():
            sys.exit("MISSING " + path + "；先运行 python3 make_fixtures.py")
    print("=== fixed online x", RUNS, "===")
    fixed = [run_chrome("online" + str(i), FIXED, i) for i in range(RUNS)]
    print("=== old unpinned x 1 ===")
    oldruns = [run_chrome("old0", OLD, 0)]
    print("=== offline vendored x", RUNS, "===")
    offruns = [run_chrome("off" + str(i), OFFLINE, i) for i in range(RUNS)]
    print("=== CDN blocked x", RUNS, "===")
    blockruns = [run_chrome("block" + str(i), FIXED, i, blocked=True) for i in range(RUNS)]
    result = {
      "browser": next((g.get("browser") for g in fixed if "browser" in g), None),
      "fixed_online": summarize("fixed_online", fixed),
      "old_unpinned": summarize("old_unpinned", oldruns),
      "offline_vendored": summarize("offline_vendored", offruns),
      "cdn_blocked": summarize("cdn_blocked", blockruns),
      "raw": {"fixed": fixed, "old": oldruns, "offline": offruns, "blocked": blockruns},
    }
    Path(OUT).write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps({k: v for k, v in result.items() if k != "raw"}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
