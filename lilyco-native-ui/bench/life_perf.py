#!/usr/bin/env python3
"""Conway Life steady-state A/B driver: NSH native GTK vs React/Web canvas.

Sequential runs avoid CPU contention. Native metrics come from nsh::fps plus
an in-process timer/canvas instrumentation file; Web metrics come from CDP
requestAnimationFrame/performance.now() plus /proc Chrome process-tree RSS.
"""
import hashlib
import json
import math
import os
import re
import signal
import statistics
import subprocess
import threading
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
BIN = Path('/home/lilyco/.cache/lilyco-build/target/release/life')
OFFLINE = HERE / 'fixtures/.generated/life-offline.html'
FIXED = HERE / 'fixtures/life-fixed.html'
RESULTS = HERE / 'results'
OUT = RESULTS / 'game-perf-2026-11-15.json'
NODE = HERE / 'life_cdp.js'
PORT = int(os.environ.get('LIFE_CDP_PORT', '9441'))
HOLD_MS = int(os.environ.get('LIFE_HOLD_MS', '9000'))
WARMUP_S = float(os.environ.get('LIFE_WARMUP_S', '2'))
WARMUP_SAMPLES = 120
RUNS = int(os.environ.get('LIFE_RUNS', '3'))
CHROMIUM = os.environ.get('CHROMIUM', 'chromium')
RESULTS.mkdir(exist_ok=True)


def median(values):
    return round(statistics.median(values), 2) if values else None


def mean(values):
    return round(statistics.fmean(values), 2) if values else None


def percentile(values, p):
    if not values:
        return None
    s = sorted(values)
    return s[min(len(s) - 1, math.floor(p / 100 * len(s)))]


def summarize(values):
    a = [float(v) for v in values if v is not None]
    if not a:
        return None
    m = statistics.fmean(a)
    return {
        'n': len(a), 'min': round(min(a), 2), 'median': median(a),
        'mean': round(m, 2), 'p95': round(percentile(a, 95), 2),
        'max': round(max(a), 2),
        'stdev': round(statistics.pstdev(a), 2) if len(a) > 1 else 0,
    }


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def command(*args):
    return subprocess.run(args, capture_output=True, text=True, timeout=20).stdout.strip()


def environment():
    return {
        'kernel': command('uname', '-sr'),
        'cpu_count': os.cpu_count(),
        'display': os.environ.get('DISPLAY'),
        'wayland_display': os.environ.get('WAYLAND_DISPLAY'),
        'chromium': command(CHROMIUM, '--version'),
        'node': command('node', '--version'),
        'native_binary': str(BIN),
        'native_binary_sha256': sha256(BIN),
        'web_fixture': str(OFFLINE),
        'web_fixture_sha256': sha256(OFFLINE),
        'web_fixture_bytes': OFFLINE.stat().st_size,
        'fixed_fixture_sha256': sha256(FIXED),
        'hold_ms': HOLD_MS,
        'warmup_s': WARMUP_S,
        'warmup_samples': WARMUP_SAMPLES,
        'runs_per_backend': RUNS,
    }


def proc_rss_mb(pid):
    try:
        for line in Path(f'/proc/{pid}/status').read_text().splitlines():
            if line.startswith('VmRSS:'):
                return int(line.split()[1]) / 1024.0
    except OSError:
        return None
    return None


def profile_pids(profile):
    out = []
    key = str(profile)
    for p in Path('/proc').iterdir():
        if not p.name.isdigit():
            continue
        try:
            cmdline = (p / 'cmdline').read_bytes().replace(b'\x00', b' ').decode(errors='replace')
            if key in cmdline:
                out.append(int(p.name))
        except OSError:
            pass
    return out


def profile_rss_mb(profile):
    total = 0.0
    found = False
    for pid in profile_pids(profile):
        v = proc_rss_mb(pid)
        if v is not None:
            total += v
            found = True
    return total if found else None


def profile_jiffies(profile):
    total = 0
    for pid in profile_pids(profile):
        try:
            stat = Path(f'/proc/{pid}/stat').read_text()
            rest = stat[stat.rfind(')') + 2:].split()
            total += int(rest[11]) + int(rest[12])
        except (OSError, ValueError, IndexError):
            pass
    return total


class Sampler:
    def __init__(self, fn, interval=0.25):
        self.fn = fn
        self.interval = interval
        self.values = []
        self.stop_event = threading.Event()
        self.thread = threading.Thread(target=self._run, daemon=True)

    def _run(self):
        while not self.stop_event.wait(self.interval):
            try:
                value = self.fn()
                if value is not None:
                    self.values.append(value)
            except Exception:
                pass

    def start(self):
        self.thread.start()

    def stop(self):
        self.stop_event.set()
        self.thread.join(timeout=2)
        return self.values


def read_kv(path):
    data = {}
    if not Path(path).exists():
        return data
    for line in Path(path).read_text().splitlines():
        if '=' not in line:
            continue
        key, value = line.split('=', 1)
        if key in {'tick_gaps_us', 'step_us', 'draw_us'}:
            data[key] = [int(x) for x in value.split(',') if x]
        else:
            data[key] = value
    return data


def parse_fps_line(line):
    out = {}
    m = re.search(r'fps (\d+)/(\d+) frame ([\d.]+)/([\d.]+)/([\d.]+)/([\d.]+)', line)
    if m:
        out['fps'] = int(m.group(1))
        out['fps_target'] = int(m.group(2))
        out['frame_mean_ms'] = float(m.group(3))
        out['frame_p50_ms'] = float(m.group(4))
        out['frame_p95_ms'] = float(m.group(5))
        out['frame_max_ms'] = float(m.group(6))
    for key in ('gap', 'paint', 'snapshot', 'canvas'):
        m = re.search(key + r' ([\d.]+)/([\d.]+)/([\d.]+)/([\d.]+)', line)
        if m:
            out[key + '_mean_ms'] = float(m.group(1))
            out[key + '_p50_ms'] = float(m.group(2))
            out[key + '_p95_ms'] = float(m.group(3))
            out[key + '_max_ms'] = float(m.group(4))
    m = re.search(r'cpu (\d+)% rss ([\d.]+) MB', line)
    if m:
        out['cpu_pct'] = int(m.group(1))
        out['fps_rss_mb'] = float(m.group(2))
    return out


def native_run(index):
    metrics_path = Path(f'/tmp/life-native-{index}.txt')
    log_path = Path(f'/tmp/life-native-{index}.log')
    for p in (metrics_path, log_path):
        p.unlink(missing_ok=True)
    env = os.environ.copy()
    env.update({
        'GDK_BACKEND': 'x11',
        'LILYCO_BENCH': '1',
        'LILYCO_LIFE_METRICS': str(metrics_path),
    })
    with log_path.open('w') as log:
        proc = subprocess.Popen([str(BIN)], stdout=log, stderr=subprocess.STDOUT,
                                env=env, start_new_session=True)
    pid = proc.pid
    for _ in range(80):
        text = log_path.read_text(errors='replace') if log_path.exists() else ''
        m = re.search(r'LILYCO_BENCH_PID (\d+)', text)
        if m:
            pid = int(m.group(1))
            break
        time.sleep(0.1)
    sampler = Sampler(lambda: proc_rss_mb(pid))
    sampler.start()
    time.sleep(WARMUP_S + HOLD_MS / 1000 + 0.5)
    rss_samples = sampler.stop()
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        proc.wait(timeout=3)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait(timeout=3)

    text = log_path.read_text(errors='replace')
    digest = re.search(r'LILYCO_GAME_DIGEST (\S+)', text)
    config = re.search(r'LILYCO_GAME_CONFIG (.+)', text)
    first_frame = re.search(r'first frame: eval \d+µs, total (\d+)µs', text)
    fps_lines = [l for l in text.splitlines() if 'nsh::fps]' in l]
    steady = [parse_fps_line(l) for l in fps_lines[2:]]
    steady = [d for d in steady if d]
    kv = read_kv(metrics_path)
    trim = lambda a: (a or [])[WARMUP_SAMPLES:]
    ticks = int(kv.get('ticks', 0))
    draws = int(kv.get('draws', 0))
    elapsed_ms = int(kv.get('elapsed_ms', 0)) or 1
    return {
        'run': index,
        'pid': pid,
        'digest': digest.group(1) if digest else None,
        'config': config.group(1).strip() if config else None,
        'first_frame_internal_ms': round(int(first_frame.group(1)) / 1000, 2) if first_frame else None,
        'ticks': ticks,
        'draws': draws,
        'elapsed_ms': elapsed_ms,
        'steps_per_s': round(ticks / (elapsed_ms / 1000), 2),
        'draws_per_s': round(draws / (elapsed_ms / 1000), 2),
        'tick_gaps_us': summarize(trim(kv.get('tick_gaps_us'))),
        'step_us': summarize(trim(kv.get('step_us'))),
        'draw_us': summarize(trim(kv.get('draw_us'))),
        'fps_monitor': {
            'lines_total': len(fps_lines),
            'lines_steady': len(steady),
            'fps': summarize([d.get('fps') for d in steady]),
            'frame_p50_ms': summarize([d.get('frame_p50_ms') for d in steady]),
            'frame_mean_ms': summarize([d.get('frame_mean_ms') for d in steady]),
            'gap_p50_ms': summarize([d.get('gap_p50_ms') for d in steady]),
            'paint_mean_ms': summarize([d.get('paint_mean_ms') for d in steady]),
            'snapshot_mean_ms': summarize([d.get('snapshot_mean_ms') for d in steady]),
            'canvas_mean_ms': summarize([d.get('canvas_mean_ms') for d in steady]),
            'cpu_pct': summarize([d.get('cpu_pct') for d in steady]),
            'fps_rss_mb': summarize([d.get('fps_rss_mb') for d in steady]),
        },
        'proc_rss_mb': summarize(rss_samples),
        'panics': len(re.findall(r'panicked at|CRITICAL|GLib-GIO-ERROR', text)),
        'fps_lines_steady': fps_lines[2:],
    }


def cdp_ready(port):
    try:
        with urllib.request.urlopen(f'http://127.0.0.1:{port}/json/version', timeout=1) as r:
            return r.status == 200
    except Exception:
        return False


def web_run(index):
    profile = Path(f'/tmp/life-chrome-{index}')
    subprocess.run(['rm', '-rf', str(profile)], check=False)
    log_path = Path(f'/tmp/life-chrome-{index}.log')
    out_path = Path(f'/tmp/life-web-{index}.json')
    out_path.unlink(missing_ok=True)
    args = [CHROMIUM, '--headless=new', '--no-sandbox', '--disable-gpu',
            '--disable-dev-shm-usage', '--no-first-run', '--no-default-browser-check',
            f'--user-data-dir={profile}', f'--remote-debugging-port={PORT}',
            '--window-size=760,560', 'about:blank']
    with log_path.open('w') as log:
        proc = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    for _ in range(80):
        if cdp_ready(PORT):
            break
        time.sleep(0.25)
    else:
        raise RuntimeError('chromium CDP did not start')
    jiffies_before = profile_jiffies(profile)
    sampler = Sampler(lambda: profile_rss_mb(profile))
    sampler.start()
    env = os.environ.copy()
    env.update({
        'LIFE_CDP_PORT': str(PORT),
        'LIFE_HOLD_MS': str(HOLD_MS),
        'LIFE_WARMUP_SAMPLES': str(WARMUP_SAMPLES),
    })
    if index == 1:
        env['LIFE_SCREENSHOT'] = str(RESULTS / 'life-web-2026-11-15.png')
    node = subprocess.run(['node', str(NODE), str(out_path), OFFLINE.as_uri()],
                          capture_output=True, text=True, env=env, timeout=HOLD_MS / 1000 + 60)
    rss_samples = sampler.stop()
    jiffies_after = profile_jiffies(profile)
    elapsed = HOLD_MS / 1000.0 + 3.0
    cpu_pct = round((jiffies_after - jiffies_before) / os.sysconf('SC_CLK_TCK') / elapsed * 100, 2)
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        proc.wait(timeout=3)
    except subprocess.TimeoutExpired:
        subprocess.run(['pkill', '-9', '-f', str(profile)], check=False)
    if not out_path.exists():
        raise RuntimeError(f'CDP produced no JSON: {node.stdout} {node.stderr}')
    data = json.loads(out_path.read_text())
    if 'error' in data:
        raise RuntimeError(data['error'])
    data.update({
        'run': index,
        'node_stdout': node.stdout.strip(),
        'node_stderr': node.stderr.strip(),
        'chrome_rss_mb': summarize(rss_samples),
        'chrome_cpu_pct_from_proc_jiffies': cpu_pct,
        'chrome_jiffies_delta': jiffies_after - jiffies_before,
        'js_heap_mb': round(data.get('perfMetrics', {}).get('JSHeapUsedSize', 0) / 1048576, 2),
        'steps_per_s': round(data['snapshot']['ticks'] / (data['snapshot']['elapsed_ms'] / 1000), 2),
        'draws_per_s': round(data['snapshot']['draws'] / (data['snapshot']['elapsed_ms'] / 1000), 2),
    })
    return data


def aggregate(runs, keys):
    out = {}
    for key in keys:
        vals = [r.get(key) for r in runs if isinstance(r.get(key), (int, float))]
        out[key] = summarize(vals)
    return out


def median_aggregate(runs, keys):
    out = {}
    for key in keys:
        vals = []
        for r in runs:
            d = r.get(key)
            if isinstance(d, dict) and d.get('median') is not None:
                vals.append(d['median'])
        out[key] = summarize(vals)
    return out


def nested_aggregate(runs, prefix, keys):
    out = {}
    for key in keys:
        vals = []
        for r in runs:
            d = r.get(prefix, {}).get(key)
            if isinstance(d, dict) and d.get('median') is not None:
                vals.append(d['median'])
        out[key] = summarize(vals)
    return out


def main():
    env = environment()
    native, web = [], []
    for i in range(1, RUNS + 1):
        print(f'NATIVE_RUN {i}/{RUNS}', flush=True)
        native.append(native_run(i))
        print(f'WEB_RUN {i}/{RUNS}', flush=True)
        web.append(web_run(i))
    native_digests = sorted({r['digest'] for r in native})
    web_digests = sorted({r['digest'] for r in web})
    configs = sorted({r['config'] for r in native + web})
    functional_match = native_digests == web_digests and len(native_digests) == 1 and len(configs) == 1
    result = {
        'generated_at': time.strftime('%Y-%m-%dT%H:%M:%S%z'),
        'game': 'Conway Game of Life 120x80, cell 6px, xorshift32 seed 1279872089, B3/S23, reseed every 240 generations',
        'environment': env,
        'functional_match': functional_match,
        'native_digest': native_digests[0] if len(native_digests) == 1 else native_digests,
        'web_digest': web_digests[0] if len(web_digests) == 1 else web_digests,
        'config': configs[0] if len(configs) == 1 else configs,
        'native': {
            'runs': native,
            'aggregate': aggregate(native, ['first_frame_internal_ms', 'steps_per_s', 'draws_per_s', 'panics']),
            'steady_aggregate': {
                **median_aggregate(native, ['step_us', 'draw_us', 'tick_gaps_us', 'proc_rss_mb']),
                'fps_monitor': nested_aggregate(native, 'fps_monitor',
                    ['fps', 'frame_p50_ms', 'frame_mean_ms', 'gap_p50_ms', 'paint_mean_ms',
                     'snapshot_mean_ms', 'canvas_mean_ms', 'cpu_pct', 'fps_rss_mb']),
            },
        },
        'web': {
            'runs': web,
            'aggregate': aggregate(web, ['firstRenderMs', 'effective_fps', 'steps_per_s', 'draws_per_s', 'js_heap_mb', 'chrome_cpu_pct_from_proc_jiffies']),
            'steady_aggregate': {
                **median_aggregate(web, ['raf_gaps_us', 'step_us', 'draw_us', 'chrome_rss_mb']),
            },
        },
        'caveats': [
            'native FPS is the GTK frame clock reported by nsh::fps (60/60); Web FPS is 1e6 / median(requestAnimationFrame interval).',
            'native step_us/draw_us are Rust CPU work; Web step_us/draw_us are JS performance.now() around equivalent work, not rasterization.',
            'native RSS is one process; Web RSS is the whole Chromium process tree (multi-process).',
            'CDN cold start is deliberately excluded: steady-state Web uses the vendored inline fixture.',
            'Numbers are from one sequential batch on one host; do not compare across earlier batches.',
        ],
    }
    OUT.write_text(json.dumps(result, indent=2, ensure_ascii=False))
    print('GAME_PERF_RESULT', OUT)
    print('FUNCTIONAL_MATCH', functional_match, 'native', native_digests, 'web', web_digests)
    print('NATIVE_AGG', json.dumps(result['native']['aggregate'], ensure_ascii=False))
    print('WEB_AGG', json.dumps(result['web']['aggregate'], ensure_ascii=False))
    print('NATIVE_STEADY', json.dumps(result['native']['steady_aggregate'], ensure_ascii=False))
    print('WEB_STEADY', json.dumps(result['web']['steady_aggregate'], ensure_ascii=False))


if __name__ == '__main__':
    main()
