#!/usr/bin/env python3
"""Scalable Life stress sweep: locate the 60 FPS drop-off point on both backends.

Reuses native_run()/web_run() from life_perf.py but points them at the
environment-configurable life_scale binary and the matching per-size fixture.
Every sweep point keeps the same 720 x 480 canvas area, so the variable is the
number of cells / draw ops, not the number of pixels rasterized.
"""
import hashlib
import json
import os
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import life_perf as lp

BIN = Path('/home/lilyco/.cache/lilyco-build/target/release/life_scale')
CONFIGS = [
    {'tag': '120x80x6', 'cols': 120, 'rows': 80, 'cell': '6'},
    {'tag': '180x120x4', 'cols': 180, 'rows': 120, 'cell': '4'},
    {'tag': '240x160x3', 'cols': 240, 'rows': 160, 'cell': '3'},
    {'tag': '360x240x2', 'cols': 360, 'rows': 240, 'cell': '2'},
]
RUNS = int(os.environ.get('LIFE_SCALE_RUNS', '2'))
HOLD_MS = int(os.environ.get('LIFE_SCALE_HOLD_MS', '7000'))
PORT = int(os.environ.get('LIFE_SCALE_PORT', '9442'))
OUT = HERE / 'results' / 'game-scale-2026-11-15.json'

lp.BIN = BIN
lp.PORT = PORT
lp.HOLD_MS = HOLD_MS
lp.WARMUP_S = 2.0
lp.WARMUP_SAMPLES = 60
lp.RESULTS = Path('/tmp/life-scale-results')
lp.RESULTS.mkdir(exist_ok=True)
lp.CHROMIUM = os.environ.get('CHROMIUM', 'chromium')

def med(d):
    return d.get('median') if isinstance(d, dict) else None

def pick(d, key):
    return d.get(key) if isinstance(d, dict) else None

def sha256(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()

def native_pick(n):
    fm = n['fps_monitor']
    return {
        'digest': n['digest'], 'config': n['config'],
        'fps': med(fm['fps']), 'frame_mean_ms': med(fm['frame_mean_ms']),
        'frame_p50_ms': med(fm['frame_p50_ms']),
        'canvas_mean_ms': med(fm['canvas_mean_ms']),
        'snapshot_mean_ms': med(fm['snapshot_mean_ms']),
        'paint_mean_ms': med(fm['paint_mean_ms']),
        'cpu_pct': med(fm['cpu_pct']),
        'step_us': med(n['step_us']), 'draw_us': med(n['draw_us']),
        'tick_gaps_us': med(n['tick_gaps_us']),
        'proc_rss_mb': med(n['proc_rss_mb']),
        'steps_per_s': n['steps_per_s'], 'draws_per_s': n['draws_per_s'],
        'ticks': n['ticks'], 'draws': n['draws'], 'panics': n['panics'],
    }

def web_pick(w):
    raf = w.get('raf_gaps_us') or {}
    return {
        'digest': w['digest'], 'config': w['config'],
        'effective_fps': w['effective_fps'],
        'raf_median_us': pick(raf, 'median'), 'raf_mean_us': pick(raf, 'mean'),
        'raf_p95_us': pick(raf, 'p95'), 'raf_max_us': pick(raf, 'max'),
        'step_us': med(w.get('step_us')), 'draw_us': med(w.get('draw_us')),
        'chrome_rss_mb': med(w.get('chrome_rss_mb')),
        'js_heap_mb': w.get('js_heap_mb'),
        'chrome_cpu_pct': w.get('chrome_cpu_pct_from_proc_jiffies'),
        'steps_per_s': w['steps_per_s'], 'draws_per_s': w['draws_per_s'],
        'firstRenderMs': w['firstRenderMs'],
        'errors': len(w.get('errors') or []),
        'exceptions': len(w.get('exceptions') or []),
        'netFail': len(w.get('netFail') or []),
        'externalRequests': len(w.get('externalRequests') or []),
    }

def main():
    results = []
    for i, c in enumerate(CONFIGS):
        tag = c['tag']
        fixture = HERE / 'fixtures' / '.generated' / ('life-scale-' + tag + '.html')
        if not fixture.exists():
            raise RuntimeError('missing fixture: ' + str(fixture))
        lp.OFFLINE = fixture
        os.environ['LIFE_COLS'] = str(c['cols'])
        os.environ['LIFE_ROWS'] = str(c['rows'])
        os.environ['LIFE_CELL'] = c['cell']
        os.environ['LIFE_TICK_US'] = '16667'
        nruns, wruns = [], []
        for j in range(RUNS):
            idx = 100 + i * 10 + j
            print('CONFIG', tag, 'RUN', j + 1, '/', RUNS, flush=True)
            n = native_pick(lp.native_run(idx))
            w = web_pick(lp.web_run(idx))
            n['run'] = j + 1
            w['run'] = j + 1
            nruns.append(n)
            wruns.append(w)
            print('  native', json.dumps(n, ensure_ascii=False), flush=True)
            print('  web   ', json.dumps(w, ensure_ascii=False), flush=True)
        keys_n = ['fps', 'steps_per_s', 'draws_per_s', 'step_us', 'draw_us', 'frame_mean_ms', 'canvas_mean_ms', 'snapshot_mean_ms', 'proc_rss_mb', 'cpu_pct']
        keys_w = ['effective_fps', 'steps_per_s', 'draws_per_s', 'step_us', 'draw_us', 'raf_p95_us', 'chrome_rss_mb', 'js_heap_mb', 'chrome_cpu_pct', 'firstRenderMs']
        na = {k: lp.summarize([r[k] for r in nruns]) for k in keys_n}
        wa = {k: lp.summarize([r[k] for r in wruns]) for k in keys_w}
        digests = sorted({r['digest'] for r in nruns + wruns})
        configs = sorted({r['config'] for r in nruns + wruns})
        n_ok = all((r['fps'] or 0) >= 58 and (r['steps_per_s'] or 0) >= 55 and (r['draws_per_s'] or 0) >= 55 for r in nruns)
        w_ok = all((r['effective_fps'] or 0) >= 58 and (r['steps_per_s'] or 0) >= 55 and (r['draws_per_s'] or 0) >= 55 for r in wruns)
        results.append({
            'tag': tag, 'cols': c['cols'], 'rows': c['rows'], 'cell': float(c['cell']),
            'cells': c['cols'] * c['rows'], 'canvas_px': '720x480',
            'fixture': str(fixture), 'fixture_sha256': sha256(fixture),
            'functional_match': len(digests) == 1 and len(configs) == 1,
            'digest': digests[0] if len(digests) == 1 else digests,
            'config': configs[0] if len(configs) == 1 else configs,
            'native_runs': nruns, 'web_runs': wruns,
            'native_aggregate': na, 'web_aggregate': wa,
            'native_holds_60fps': n_ok, 'web_holds_60fps': w_ok,
        })
    native_ok = [r['tag'] for r in results if r['native_holds_60fps']]
    web_ok = [r['tag'] for r in results if r['web_holds_60fps']]
    out = {
        'generated_at': time.strftime('%Y-%m-%dT%H:%M:%S%z'),
        'purpose': '60 FPS drop-off curve: same 720x480 canvas, growing cell/draw-op count',
        'environment': {
            'kernel': lp.command('uname', '-sr'),
            'cpu_count': os.cpu_count(),
            'native_binary': str(BIN), 'native_binary_sha256': sha256(BIN),
            'chromium': lp.command(lp.CHROMIUM, '--version'),
            'node': lp.command('node', '--version'),
            'cdp_port': PORT, 'hold_ms': HOLD_MS, 'warmup_s': lp.WARMUP_S,
            'warmup_samples': lp.WARMUP_SAMPLES, 'runs_per_point': RUNS,
            'display': os.environ.get('DISPLAY'),
            'wayland_display': os.environ.get('WAYLAND_DISPLAY'),
        },
        'configs': results,
        'largest_native_holding_60fps': native_ok[-1] if native_ok else None,
        'largest_web_holding_60fps': web_ok[-1] if web_ok else None,
        'first_native_drop': next((r['tag'] for r in results if not r['native_holds_60fps']), None),
        'first_web_drop': next((r['tag'] for r in results if not r['web_holds_60fps']), None),
        'caveats': [
            'Native FPS is the GTK frame clock reported by nsh::fps (target 60); Web FPS is 1e6 / median(rAF interval).',
            'Every point keeps a 720x480 canvas; only the number of cells and fill_rect draw ops grows.',
            'native step_us/draw_us are Rust CPU work; Web ones are JS performance.now() around equivalent work, not rasterization.',
            'Native RSS is one process; Web RSS is the whole Chromium process tree.',
            'Steady-state Web uses vendored inline fixtures; CDN cold start is excluded.',
            'All numbers come from one sequential batch on one host; do not mix with 2026-11-15 16:05 or earlier batches.',
        ],
    }
    OUT.write_text(json.dumps(out, indent=2, ensure_ascii=False))
    print('GAME_SCALE_RESULT', OUT)
    print('LARGEST_NATIVE_60', out['largest_native_holding_60fps'], 'LARGEST_WEB_60', out['largest_web_holding_60fps'])
    print('FIRST_NATIVE_DROP', out['first_native_drop'], 'FIRST_WEB_DROP', out['first_web_drop'])

if __name__ == '__main__':
    main()
