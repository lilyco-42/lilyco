#!/usr/bin/env python3
import os, subprocess, sys, time
from pathlib import Path
sys.path = [p for p in sys.path if p not in ("/tmp", "")]
sys.path.insert(0, "/home/lilyco/code/lilyco/lilyco-native-ui/bench")
import native_bench as nb
BIN = "/tmp/lilyco-native-ui-old-release"
SPEC = "/tmp/bench-spec.json"
log = Path("/tmp/clickwin-diag.log")
f = log.open("w")
env = dict(os.environ); env.update({"GDK_BACKEND": "x11", "LILYCO_BENCH": "1"})
base = nb.xids()
p = subprocess.Popen([BIN, SPEC], env=env, stdout=f, stderr=subprocess.STDOUT)
wid = nb.wait_new(base, pid=p.pid)
print("WID", wid, "PID", p.pid, flush=True)
time.sleep(1.2)
if wid:
    print("ACT", subprocess.run(["/tmp/x11bench", "activate", wid], capture_output=True, text=True).stdout.strip(), flush=True)
    g = nb.geom(wid)
    x = int(g.get("w", 1280)) // 2
    y = 820
    print("GEOM", g, "WIN_XY", x, y, flush=True)
    cr = subprocess.run(["/tmp/x11bench", "clickwin", wid, str(x), str(y)], capture_output=True, text=True)
    print("CLICK_OUT", cr.stdout.strip(), flush=True)
    print("CLICK_ERR", cr.stderr.strip(), flush=True)
    time.sleep(1.0)
p.terminate()
try: p.wait(timeout=3)
except subprocess.TimeoutExpired: p.kill(); p.wait()
f.close()
text = log.read_text()
print("LOG_LINES", len(text.splitlines()), flush=True)
for line in text.splitlines():
    if any(k in line for k in ("nsh::perf]", "nsh::fps]", "LILYCO_NATIVE_PRESS", "WARN", "CRITICAL", "panick")):
        print("LOG", line[:280], flush=True)
