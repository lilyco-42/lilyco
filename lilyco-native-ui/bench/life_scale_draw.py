#!/usr/bin/env python3
"""Draw-op scaling sweep: fixed 360x240x2 Life, growing identical fill_rect passes.

Reuses native_run()/web_run() and pickers from the earlier scale harness, but
runs an independent native binary and per-repeat vendored Web fixtures.
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
import life_scale as ls

BIN = Path('/home/lilyco/.cache/lilyco-build/target/release/life_draw')
REPEATS = [int(x) for x in os.environ.get('LIFE_DRAW_REPEATS', '1,2,4,8,16').split(',')]
RUNS = int(os.environ.get('LIFE_SCALE_RUNS', '2'))
HOLD_MS = int(os.environ.get('LIFE_SCALE_HOLD_MS', '7000'))
PORT = int(os.environ.get('LIFE_SCALE_PORT', '9443'))
OUT = HERE / 'results' / 'game-scale-draw-2026-11-15.json'

lp.BIN = BIN
lp.PORT = PORT
lp.HOLD_MS = HOLD_MS
lp.WARMUP_S = 2.0
lp.WARMUP_SAMPLES = 60
lp.RESULTS = Path('/tmp/life-draw-results')
lp.RESULTS.mkdir(exist_ok=True)
lp.CHROMIUM = os.environ.get('CHROMIUM', 'chromium')

KEYS_N = ['fps', 'steps_per_s', 'draws_per_s', 'step_us', 'draw_us', 'frame_mean_ms',
          'canvas_mean_ms', 'snapshot_mean_ms', 'proc_rss_mb', 'cpu_pct']
KEYS_W = ['effective_fps', 'steps_per_s', 'draws_per_s', 'step_us', 'draw_us', 'raf_p95_us',
          'chrome_rss_mb', 'js_heap_mb', 'chrome_cpu_pct', 'firstRenderMs']

def sha256(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()

def shell(results):
    native_ok = [r['tag'] for r in results if r['native_holds_60fps']]
    web_ok = [r['tag'] for r in results if r['web_holds_60fps']]
    return {
        'generated_at': time.strftime('%Y-%m-%dT%H:%M:%S%z'),
        'purpose': '60 FPS drop-off curve: fixed 360x240x2 Life grid, repeated identical fill_rect passes',
        'environment': {
            'kernel': lp.command('uname', '-sr'), 'cpu_count': os.cpu_count(),
            'native_binary': str(BIN), 'native_binary_sha256': sha256(BIN),
            'chromium': lp.command(lp.CHROMIUM, '--version'),
            'node': lp.command('node', '--version'), 'cdp_port': PORT,
            'hold_ms': HOLD_MS, 'warmup_s': lp.WARMUP_S,
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
            'Every point keeps a 720x480 canvas and the same 360x240x2 simulation; DRAW_REPEAT multiplies identical alive-cell fill_rect passes only.',
            'Simulation state, seed and FNV-1a digest are identical across all repeat counts; only CPU/raster work grows.',
            'native step_us measures the Life step (unchanged); draw_us measures Painter construction for all repeated passes. Web step_us/draw_us are performance.now() around equivalent JS work, not rasterization.',
            'Native RSS is one process; Web RSS is the whole Chromium process tree.',
            'Steady-state Web uses vendored inline fixtures; CDN cold start is excluded.',
            'All numbers come from one sequential batch on one host; do not mix with the 17:20 grid-scaling batch or the 16:05 baseline.',
        ],
    }

def main():
    results = []
    for i, rep in enumerate(REPEATS):
        tag = 'draw-' + str(rep)
        fixture = HERE / 'fixtures' / '.generated' / ('life-' + tag + '.html')
        if not fixture.exists():
            raise RuntimeError('missing fixture: ' + str(fixture))
        lp.OFFLINE = fixture
        os.environ.update({'LIFE_COLS': '360', 'LIFE_ROWS': '240', 'LIFE_CELL': '2',
                           'LIFE_TICK_US': '16667', 'LIFE_DRAW_REPEAT': str(rep)})
        nruns, wruns, error = [], [], None
        try:
            for j in range(RUNS):
                idx = 200 + i * 10 + j
                print('CONFIG', tag, 'RUN', j + 1, '/', RUNS, flush=True)
                n = ls.native_pick(lp.native_run(idx))
                w = ls.web_pick(lp.web_run(idx))
                n['run'], w['run'] = j + 1, j + 1
                nruns.append(n)
                wruns.append(w)
                print('  native', json.dumps(n, ensure_ascii=False), flush=True)
                print('  web   ', json.dumps(w, ensure_ascii=False), flush=True)
        except Exception as exc:
            error = repr(exc)
            print('  ERROR', tag, error, flush=True)
        na = {k: lp.summarize([r[k] for r in nruns]) for k in KEYS_N} if nruns else {}
        wa = {k: lp.summarize([r[k] for r in wruns]) for k in KEYS_W} if wruns else {}
        digests = sorted({r['digest'] for r in nruns + wruns})
        configs = sorted({r['config'] for r in nruns + wruns})
        n_ok = (error is None and len(nruns) == RUNS and
                all((r['fps'] or 0) >= 58 and (r['steps_per_s'] or 0) >= 55 and (r['draws_per_s'] or 0) >= 55 for r in nruns))
        w_ok = (error is None and len(wruns) == RUNS and
                all((r['effective_fps'] or 0) >= 58 and (r['steps_per_s'] or 0) >= 55 and (r['draws_per_s'] or 0) >= 55 for r in wruns))
        results.append({
            'tag': tag, 'cols': 360, 'rows': 240, 'cell': 2.0, 'cells': 86400,
            'canvas_px': '720x480', 'draw_repeat': rep, 'cell_visits_per_frame': 86400 * rep,
            'fixture': str(fixture), 'fixture_sha256': sha256(fixture),
            'functional_match': error is None and len(digests) == 1 and len(configs) == 1,
            'digest': digests[0] if len(digests) == 1 else digests,
            'config': configs[0] if len(configs) == 1 else configs,
            'error': error,
            'native_runs': nruns, 'web_runs': wruns,
            'native_aggregate': na, 'web_aggregate': wa,
            'native_holds_60fps': n_ok, 'web_holds_60fps': w_ok,
        })
        OUT.write_text(json.dumps(shell(results), indent=2, ensure_ascii=False))
        print('POINT_WRITTEN', tag, 'nHold', n_ok, 'wHold', w_ok, flush=True)
    out = shell(results)
    print('GAME_SCALE_DRAW_RESULT', OUT)
    print('LARGEST_NATIVE_60', out['largest_native_holding_60fps'], 'LARGEST_WEB_60', out['largest_web_holding_60fps'])
    print('FIRST_NATIVE_DROP', out['first_native_drop'], 'FIRST_WEB_DROP', out['first_web_drop'])

if __name__ == '__main__':
    main()
