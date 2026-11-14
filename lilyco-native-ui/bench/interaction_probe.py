#!/usr/bin/env python3
import os, subprocess, sys, time
from pathlib import Path
sys.path = [p for p in sys.path if p not in ("/tmp", "")]
sys.path.insert(0, "/home/lilyco/code/lilyco/lilyco-native-ui/bench")
import native_bench as nb
BIN = "/home/lilyco/.cache/lilyco-build/target/release/lilyco-native-ui"
SPEC = "/tmp/bench-spec.json"
ACT = "/home/lilyco/code/lilyco/lilyco-native-ui/bench/atspi_activate.py"
log = Path("/tmp/atspi-probe.log")
f = log.open("w")
env = dict(os.environ); env.update({"GDK_BACKEND": "x11", "LILYCO_BENCH": "1"})
base = nb.xids()
p = subprocess.Popen([BIN, SPEC], env=env, stdout=f, stderr=subprocess.STDOUT)
wid = nb.wait_new(base, pid=p.pid)
print("WID", wid, "PID", p.pid, flush=True)
time.sleep(1.2)
if wid:
    act = subprocess.run([sys.executable, "-I", ACT, str(p.pid)], capture_output=True, text=True, timeout=15)
    print("ACTIVATE_STDOUT", act.stdout.strip(), flush=True)
    print("ACTIVATE_STDERR", act.stderr.strip()[:300], flush=True)
    print("ACTIVATE_RC", act.returncode, flush=True)
    time.sleep(0.9)
p.terminate()
try: p.wait(timeout=3)
except subprocess.TimeoutExpired: p.kill(); p.wait()
f.close()
text = log.read_text()
print("LOG_LINES", len(text.splitlines()), flush=True)
for line in text.splitlines():
    if any(k in line for k in ("LILYCO_NATIVE_PRESS", "LILYCO_BENCH_PID", "nsh::perf]", "nsh::fps]", "WARN", "CRITICAL", "panick")):
        print("LOG", line[:280], flush=True)
