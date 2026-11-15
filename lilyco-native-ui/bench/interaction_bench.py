#!/usr/bin/env python3
"""NSH 原生交互延迟：AT-SPI nsh.activate → on_press → NSH commit/turn。"""
import fcntl, json, os, re, statistics, subprocess, sys, time
from datetime import datetime, timezone
from pathlib import Path
sys.path = [p for p in sys.path if p not in ("/tmp", "")]
sys.path.insert(0, "/home/lilyco/code/lilyco/lilyco-native-ui/bench")
import native_bench as nb
BIN = "/home/lilyco/.cache/lilyco-build/target/release/lilyco-native-ui"
HERE = Path(__file__).resolve().parent
X11 = str(HERE / "bin/x11bench") if (HERE / "bin/x11bench").exists() else "/tmp/x11bench"
SPEC = str(HERE / "fixtures/bench-spec.json")
ACT = str(HERE / "atspi_activate.py")
OUT = Path(os.environ.get("BENCH_OUT", "/tmp/bench-native-interaction.json"))
ARCHIVE = Path(os.environ.get("BENCH_ARCHIVE", str(HERE / "results/native-interaction-2026-11-14.json")))
RUNS = int(os.environ.get("BENCH_RUNS", "5"))
HOLD = float(os.environ.get("BENCH_HOLD", "1.3"))
LOGDIR = Path("/tmp/lilyco-interaction-logs")
LOGDIR.mkdir(exist_ok=True)
def log_ts_us(line):
    m = re.search(r"\[(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3})Z", line)
    if not m:
        return None
    dt = datetime.strptime(m.group(1), "%Y-%m-%dT%H:%M:%S.%f").replace(tzinfo=timezone.utc)
    return int(dt.timestamp() * 1000000)
def one_run(i):
    base = nb.xids()
    log = LOGDIR / f"interaction-{i}.log"
    f = log.open("w")
    env = dict(os.environ); env.update({"GDK_BACKEND": "x11", "LILYCO_BENCH": "1"})
    t0 = time.perf_counter()
    p = subprocess.Popen([BIN, SPEC], env=env, stdout=f, stderr=subprocess.STDOUT)
    wid = nb.wait_new(base, pid=p.pid)
    window_ms = (time.perf_counter() - t0) * 1000 if wid else None
    time.sleep(HOLD)
    act = None
    if wid:
        subprocess.run([X11, "activate", wid], capture_output=True, text=True)
        r = subprocess.run([sys.executable, "-I", ACT, str(p.pid), "开始压缩"], capture_output=True, text=True, timeout=15)
        act = {"rc": r.returncode, "stdout": r.stdout.strip(), "stderr": r.stderr.strip()[:300]}
        time.sleep(0.55)
    p.terminate()
    try: p.wait(timeout=3)
    except subprocess.TimeoutExpired: p.kill(); p.wait()
    f.close()
    text = log.read_text()
    m = re.search(r"start_epoch_us=(\d+).*?do_action_us=(\d+).*?accepted=(\w+)", act["stdout"] if act else "")
    start_us = int(m.group(1)) if m else None
    do_action_us = int(m.group(2)) if m else None
    accepted = (m.group(3).lower() == "true") if m else None
    press = re.findall(r"LILYCO_NATIVE_PRESS (\d+) (\S+)", text)
    press_us = int(press[0][0]) if press else None
    delta_us = press_us - start_us if start_us is not None and press_us is not None else None
    # 旧二进制曾以 epoch 毫秒打点，与注入端 epoch 微秒相减会得到约 -1.8e12 ms。
    # 只在同钟且合理范围内接受，避免把单位错配写成基准结果。
    press_handler_ms = delta_us / 1000.0 if delta_us is not None and 0 <= delta_us < 1_000_000 else None
    lines = text.splitlines()
    ev = next((j for j, l in enumerate(lines) if "nsh::perf] event Press" in l), None)
    event_us = log_ts_us(lines[ev]) if ev is not None else None
    commit_us = turn_us = None
    if ev is not None:
        for l in lines[ev + 1: ev + 12]:
            if "nsh::perf] commit:" in l and commit_us is None:
                commit_us = log_ts_us(l)
            if "nsh::perf] turn:" in l and turn_us is None:
                turn_us = log_ts_us(l)
    event_to_commit_ms = (commit_us - event_us) / 1000.0 if commit_us and event_us else None
    event_to_turn_ms = (turn_us - event_us) / 1000.0 if turn_us and event_us else None
    state_commit_ms = (press_handler_ms + event_to_turn_ms) if press_handler_ms is not None and event_to_turn_ms is not None else None
    return {"run": i, "pid": p.pid, "window_id": wid, "window_appear_ms": window_ms,
            "activation": act, "do_action_us": do_action_us, "accepted": accepted,
            "press_count": len(press), "event_count": 1 if ev is not None else 0,
            "press_handler_ms": press_handler_ms, "event_to_commit_ms": event_to_commit_ms,
            "event_to_turn_ms": event_to_turn_ms, "state_commit_ms": state_commit_ms,
            "crash": "panicked" in text.lower(),
            "critical": "GLib-CRITICAL" in text or "Gtk-CRITICAL" in text}
def stats(values):
    values = [v for v in values if v is not None]
    if not values:
        return None
    return {"n": len(values), "min": round(min(values), 2), "median": round(statistics.median(values), 2),
            "max": round(max(values), 2), "mean": round(statistics.mean(values), 2)}
def main():
    lock = open("/tmp/lilyco-interaction-bench.lock", "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        print("BENCH_ALREADY_RUNNING", file=sys.stderr)
        sys.exit(5)
    runs = [one_run(i) for i in range(1, RUNS + 1)]
    result = {"backend": "NSH/GTK4 Rust AppSource::Rust",
              "method": "AT-SPI nsh.activate on button 开始压缩 → Rust on_press → NSH perf commit/turn",
              "binary": BIN, "spec": SPEC, "runs": RUNS, "hold_s": HOLD,
              "press_handler_ms": stats([r["press_handler_ms"] for r in runs]),
              "event_to_commit_ms": stats([r["event_to_commit_ms"] for r in runs]),
              "event_to_turn_ms": stats([r["event_to_turn_ms"] for r in runs]),
              "state_commit_ms": stats([r["state_commit_ms"] for r in runs]),
              "do_action_us": stats([r["do_action_us"] for r in runs]),
              "press_count": sum(r["press_count"] for r in runs),
              "event_count": sum(r["event_count"] for r in runs),
              "accepted": sum(1 for r in runs if r["accepted"]),
              "crashes": sum(1 for r in runs if r["crash"]),
              "criticals": sum(1 for r in runs if r["critical"]),
              "raw": runs}
    if result["press_handler_ms"] is None or result["press_handler_ms"]["n"] < RUNS:
        print("INVALID_BENCH: press_handler_ms missing/clock-unit mismatch; refuse to overwrite archive", file=sys.stderr)
        sys.exit(4)
    OUT.write_text(json.dumps(result, ensure_ascii=False, indent=2))
    ARCHIVE.parent.mkdir(exist_ok=True)
    ARCHIVE.write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps({k: v for k, v in result.items() if k != "raw"}, ensure_ascii=False, indent=2))
if __name__ == "__main__":
    main()
