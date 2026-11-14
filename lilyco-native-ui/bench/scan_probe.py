#!/usr/bin/env python3
import os, subprocess, sys, time
from pathlib import Path
sys.path = [p for p in sys.path if p not in ("/tmp", "")]
sys.path.insert(0, "/home/lilyco/code/lilyco/lilyco-native-ui/bench")
import native_bench as nb
BIN = "/home/lilyco/.cache/lilyco-build/target/release/lilyco-native-ui"
SPEC = "/tmp/bench-spec.json"
SCAN = "/home/lilyco/code/lilyco/lilyco-native-ui/bench/atspi_scan.py"
log = Path("/tmp/atspi-scan-probe.log")
f = log.open("w")
env = dict(os.environ); env.update({"GDK_BACKEND": "x11", "LILYCO_BENCH": "1"})
base = nb.xids()
p = subprocess.Popen([BIN, SPEC], env=env, stdout=f, stderr=subprocess.STDOUT)
wid = nb.wait_new(base, pid=p.pid)
print("WID", wid, "PID", p.pid, flush=True)
time.sleep(1.2)
if wid:
    r = subprocess.run([sys.executable, "-I", SCAN, str(p.pid)], capture_output=True, text=True, timeout=15)
    print(r.stdout.strip(), flush=True)
    print("SCAN_STDERR", r.stderr.strip()[:300], flush=True)
p.terminate()
try: p.wait(timeout=3)
except subprocess.TimeoutExpired: p.kill(); p.wait()
f.close()
