#!/usr/bin/env python3
"""A/B: NSH/GTK 原生窗口的启动、稳态与交互延迟测量（不修改业务代码）。"""
import json, os, re, statistics, subprocess, sys, time
from pathlib import Path
HERE = Path(__file__).resolve().parent
X11 = str(HERE / 'bin/x11bench') if (HERE / 'bin/x11bench').exists() else X11

BIN = sys.argv[1] if len(sys.argv) > 1 else '/home/lilyco/.cache/lilyco-build/target/release/lilyco-native-ui'
SPEC = sys.argv[2] if len(sys.argv) > 2 else str(HERE / 'fixtures/bench-spec.json')
OUT = sys.argv[3] if len(sys.argv) > 3 else '/tmp/bench-native.json'
RUNS = int(os.environ.get('BENCH_RUNS', '5'))
HOLD = float(os.environ.get('BENCH_HOLD', '3.5'))
LOGDIR = Path('/tmp/lilyco-bench-logs'); LOGDIR.mkdir(exist_ok=True)

def xids():
    r = subprocess.run(['xprop', '-root', '_NET_CLIENT_LIST'], capture_output=True, text=True).stdout
    if '#' not in r: return set()
    return set(t.strip().split()[0] for t in r.split('#', 1)[1].split(',') if t.strip())

def wait_new(base, sec=6.0, pid=None):
    end = time.monotonic() + sec
    while time.monotonic() < end:
        n = xids() - base
        for w in sorted(n):
            if pid:
                q = subprocess.run(['xprop', '-id', w, '_NET_WM_PID'], capture_output=True, text=True).stdout
                if str(pid) not in q: continue
            return w
        time.sleep(0.01)
    return None

def geom(wid):
    r = subprocess.run([X11, 'geometry', wid], capture_output=True, text=True).stdout.strip()
    m = dict(re.findall(r'(\w+)=([^\s]+)', r))
    return m

def proc_rss(pid):
    try:
        for line in Path(f'/proc/{pid}/status').read_text().splitlines():
            if line.startswith('VmRSS:'): return int(line.split()[1]) * 1024
    except OSError: pass
    return None

def parse_log(text):
    wall = None
    m = re.search(r'first frame: eval \d+µs, total (\d+)µs', text)
    if m: wall = float(m.group(1)) / 1000.0
    fps = []
    for line in text.splitlines():
        if 'nsh::fps]' not in line: continue
        m = re.search(r'cpu (\d+)% rss ([\d.]+) MB', line)
        if m: fps.append((int(m.group(1)), float(m.group(2))))
    turns = [float(x)/1000.0 for x in re.findall(r'turn: (\d+)µs', text)]
    events = re.findall(r'event (\S+) on view (\d+)', text)
    event_turn_ms = []
    lines = text.splitlines()
    for idx, line in enumerate(lines):
        if 'nsh::perf]' in line and '] event ' in line:
            for nxt in lines[idx+1:idx+8]:
                mt = re.search(r'turn: (\d+)\u00b5s', nxt)
                if mt: event_turn_ms.append(float(mt.group(1))/1000.0); break
    return {'first_frame_ms': wall, 'fps_samples': fps, 'turn_ms': turns, 'events': events, 'event_turn_ms': event_turn_ms}

def one_run(i):
    base = xids()
    log = LOGDIR / f'native-run-{i}.log'
    f = log.open('w')
    env = dict(os.environ); env.update({'GDK_BACKEND':'x11','LILYCO_BENCH':'1'})
    t0 = time.perf_counter()
    p = subprocess.Popen([BIN, SPEC], env=env, stdout=f, stderr=subprocess.STDOUT)
    wid = wait_new(base, pid=p.pid)
    window_ms = (time.perf_counter()-t0)*1000 if wid else None
    g = geom(wid) if wid else {}
    # 100ms 后复查，规避 xprop 窗口号瞬时复用
    time.sleep(0.1)
    if wid and geom(wid).get('id') != wid: wid = wait_new(base, pid=p.pid)
    pid = p.pid
    shown_ms = None
    if wid and g.get('mapped') == '0':
        t1 = time.perf_counter()
        while time.perf_counter()-t1 < 5:
            if geom(wid).get('mapped') == '1':
                shown_ms = (time.perf_counter()-t0)*1000; break
            time.sleep(0.005)
    time.sleep(HOLD)
    rss = proc_rss(pid)
    click = None
    if wid:
        gg = geom(wid)
        # 按钮在内容高度的约一半（截图实测：1280x1640 窗口的 '开始压缩' 按钮中心 y=820）
        x = int(gg.get('root_x', 0)) + int(gg.get('w', 1280)) // 2
        y = int(gg.get('root_y', 0)) + int(gg.get('h', 1640)) // 2
        tclick = time.perf_counter()
        subprocess.run([X11, 'click', str(x), str(y)], capture_output=True, text=True)
        time.sleep(0.7)
        click = {'x': x, 'y': y, 'wall_ms': (time.perf_counter()-tclick)*1000}
    p.terminate()
    try: p.wait(timeout=3)
    except subprocess.TimeoutExpired: p.kill(); p.wait()
    f.close()
    text = log.read_text()
    d = parse_log(text)
    d.update({'run': i, 'pid': pid, 'window_id': wid, 'window_appear_ms': window_ms,
              'window_mapped_ms': shown_ms, 'geometry': g, 'rss_bytes': rss,
              'click': click, 'exit': p.returncode, 'crash': 'panicked' in text.lower(),
              'critical': 'GLib-CRITICAL' in text or 'Gtk-CRITICAL' in text})
    return d

def main():
    runs = [one_run(i) for i in range(1, RUNS+1)]
    def stats(vals):
        vals = [v for v in vals if v is not None]
        if not vals: return None
        return {'n': len(vals), 'min': round(min(vals),2), 'median': round(statistics.median(vals),2),
                'max': round(max(vals),2), 'mean': round(statistics.mean(vals),2)}
    all_fps = [(cpu, rss) for r in runs for cpu,rss in r['fps_samples']]
    all_turns = [x for r in runs for x in r['turn_ms']]
    result = {
      'backend': 'NSH/GTK4 (Rust, no WebView, no QuickJS)',
      'binary': BIN, 'spec': SPEC, 'runs': RUNS, 'hold_s': HOLD,
      'window_appear_ms': stats([r['window_appear_ms'] for r in runs]),
      'window_mapped_ms': stats([r['window_mapped_ms'] for r in runs]),
      'first_frame_internal_ms': stats([r['first_frame_ms'] for r in runs]),
      'rss_mb': stats([r['rss_bytes']/1048576 if r['rss_bytes'] else None for r in runs]),
      'fps_monitor_rss_mb': stats([rss for cpu,rss in all_fps]),
      'fps_monitor_cpu_pct': stats([float(cpu) for cpu,rss in all_fps]),
      'turn_ms': stats(all_turns),
      'events_per_run': [len(r['events']) for r in runs],
      'interaction_turn_ms': stats([x for r in runs for x in r.get('event_turn_ms', [])]),
      'clicks': [r['click'] for r in runs],
      'crashes': sum(1 for r in runs if r['crash']), 'criticals': sum(1 for r in runs if r['critical']),
      'raw': runs,
    }
    Path(OUT).write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps({k:v for k,v in result.items() if k!='raw'}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
