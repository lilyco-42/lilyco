#!/usr/bin/env python3
import os, re, subprocess, sys, time
from pathlib import Path
sys.path = [p for p in sys.path if p not in ("/tmp", "")]
from PIL import Image, ImageChops
sys.path.insert(0, "/home/lilyco/code/lilyco/lilyco-native-ui/bench")
import native_bench as nb
BIN = "/home/lilyco/.cache/lilyco-build/target/release/lilyco-native-ui"
SPEC = "/tmp/bench-spec.json"
log = Path("/tmp/native-click-probe3.log")
f = log.open("w")
env = dict(os.environ); env.update({"GDK_BACKEND": "x11", "LILYCO_BENCH": "1"})
base = nb.xids()
p = subprocess.Popen([BIN, SPEC], env=env, stdout=f, stderr=subprocess.STDOUT)
wid = nb.wait_new(base, pid=p.pid)
print("WID", wid, "PID", p.pid, flush=True)
time.sleep(1.2)
if wid:
    act = subprocess.run(["/tmp/x11bench", "activate", wid], capture_output=True, text=True)
    print("ACT", act.stdout.strip(), act.stderr.strip(), flush=True)
    g = nb.geom(wid)
    x = int(g.get("root_x", 0)) + int(g.get("w", 1280)) // 2
    y = int(g.get("root_y", 0)) + 820
    print("XY", x, y, flush=True)
    wp = subprocess.run(["/tmp/x11bench", "warp", str(x), str(y)], capture_output=True, text=True)
    print("WARP", wp.stdout.strip(), wp.stderr.strip(), flush=True)
    before = "/tmp/click3-before.png"
    subprocess.run(["import", "-window", wid, before], capture_output=True, text=True)
    cr = subprocess.run(["/tmp/x11bench", "click", str(x), str(y)], capture_output=True, text=True)
    print("CLICK_STDOUT", cr.stdout.strip(), flush=True)
    print("CLICK_STDERR", cr.stderr.strip(), flush=True)
    time.sleep(0.8)
    after = "/tmp/click3-after.png"
    subprocess.run(["import", "-window", wid, after], capture_output=True, text=True)
    try:
        a = Image.open(before).convert("RGB"); b = Image.open(after).convert("RGB")
        diff = ImageChops.difference(a, b)
        print("DIFF_BBOX", diff.getbbox(), flush=True)
        print("STATUS_DIFF_BBOX", diff.crop((0, 900, min(a.size[0],1280), min(a.size[1],1150))).getbbox(), flush=True)
    except Exception as e: print("IMG_ERR", repr(e), flush=True)
p.terminate()
try: p.wait(timeout=3)
except subprocess.TimeoutExpired: p.kill(); p.wait()
f.close()
text = log.read_text()
print("LOG_LINES", len(text.splitlines()), flush=True)
for line in text.splitlines():
    if any(k in line for k in ("LILYCO_NATIVE_PRESS", "nsh::perf]", "nsh::fps]", "WARN", "CRITICAL", "panick")):
        print("LOG", line[:300], flush=True)
