const http = require('http');
const fs = require('fs');

const PORT = process.env.LIFE_CDP_PORT || '9441';
const OUT = process.argv[2];
const URL = process.argv[3];
const HOLD_MS = Number(process.env.LIFE_HOLD_MS || '9000');
const WARMUP_SAMPLES = Number(process.env.LIFE_WARMUP_SAMPLES || '120');
const SCREENSHOT = process.env.LIFE_SCREENSHOT || '';

function get(path) {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port: Number(PORT), path }, res => {
      let data = '';
      res.on('data', c => data += c);
      res.on('end', () => { try { resolve(JSON.parse(data)); } catch (e) { reject(e); } });
    }).on('error', reject);
  });
}
function percentile(values, p) {
  if (!values.length) return 0;
  const s = [...values].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))];
}
function median(values) {
  if (!values.length) return null;
  const s = [...values].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}
function stdev(values, mean) {
  if (values.length < 2) return 0;
  return Math.sqrt(values.reduce((s, v) => s + (v - mean) * (v - mean), 0) / values.length);
}
function summarize(values) {
  const a = values || [];
  if (!a.length) return null;
  const mean = a.reduce((x, y) => x + y, 0) / a.length;
  return {
    n: a.length, min: Math.min(...a), median: +median(a).toFixed(2),
    mean: +mean.toFixed(2), p95: +percentile(a, 95).toFixed(2),
    max: Math.max(...a), stdev: +stdev(a, mean).toFixed(2)
  };
}

async function main() {
  const version = await get('/json/version');
  const pages = await get('/json/list');
  const page = pages.find(p => p.type === 'page');
  if (!page) throw new Error('no CDP page target');
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
  let seq = 0;
  const pending = new Map();
  const events = [];
  ws.onmessage = ev => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const p = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) p.reject(new Error(JSON.stringify(msg.error))); else p.resolve(msg.result);
    } else if (msg.method) {
      events.push({ method: msg.method, params: msg.params });
    }
  };
  const send = (method, params) => new Promise((resolve, reject) => {
    const id = ++seq;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params: params || {} }));
  });
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Network.enable');
  await send('Performance.enable');

  const started = Date.now();
  await send('Page.navigate', { url: URL });
  let ready = false;
  let firstRenderMs = null;
  for (let i = 0; i < 120; i++) {
    const r = await send('Runtime.evaluate', {
      expression: '!!(window.__LIFE && window.__LIFE.digest)', returnByValue: true
    });
    if (r.result && r.result.value === true) {
      ready = true; firstRenderMs = Date.now() - started; break;
    }
    await new Promise(r => setTimeout(r, 250));
  }
  if (!ready) throw new Error('window.__LIFE did not appear within 30s');

  const before = await send('Runtime.evaluate', {
    expression: 'JSON.stringify({digest: window.__LIFE.digest, config: window.__LIFE.config})',
    returnByValue: true
  });
  const meta = JSON.parse(before.result.value);
  await new Promise(r => setTimeout(r, HOLD_MS));
  const snapResult = await send('Runtime.evaluate', {
    expression: 'JSON.stringify(window.__LIFE.snapshot())', returnByValue: true
  });
  const snap = JSON.parse(snapResult.result.value);
  const errorsResult = await send('Runtime.evaluate', {
    expression: 'JSON.stringify(window.__LIFE.errors)', returnByValue: true
  });
  const perf = await send('Performance.getMetrics');
  const metrics = {};
  for (const m of perf.metrics) metrics[m.name] = m.value;
  if (SCREENSHOT) {
    const shot = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    fs.writeFileSync(SCREENSHOT, Buffer.from(shot.data, 'base64'));
  }
  const steady = a => (a || []).slice(WARMUP_SAMPLES);
  const rafSteady = steady(snap.raf_gaps_us);
  const stepSteady = steady(snap.step_us);
  const drawSteady = steady(snap.draw_us);
  const raf = summarize(rafSteady);
  const result = {
    browser: version.Browser,
    mode: 'offline_vendored',
    url: URL,
    firstRenderMs,
    wallMs: Date.now() - started,
    digest: meta.digest,
    config: meta.config,
    snapshot: { ticks: snap.ticks, draws: snap.draws, elapsed_ms: snap.elapsed_ms },
    warmupSamples: WARMUP_SAMPLES,
    raf_gaps_us: raf,
    effective_fps: raf && raf.median ? +(1e6 / raf.median).toFixed(2) : null,
    step_us: summarize(stepSteady),
    draw_us: summarize(drawSteady),
    errors: JSON.parse(errorsResult.result.value),
    perfMetrics: metrics,
    exceptions: events.filter(e => e.method === 'Runtime.exceptionThrown').map(e => e.params.exceptionDetails),
    consoleErrors: events.filter(e => e.method === 'Runtime.consoleAPICalled' && e.params.type === 'error').map(e => e.params.args),
    netFail: events.filter(e => e.method === 'Network.loadingFailed').map(e => e.params),
    externalRequests: events.filter(e => e.method === 'Network.requestWillBeSent' && /unpkg\.com/.test(e.params.request.url)).map(e => e.params.request.url)
  };
  fs.writeFileSync(OUT, JSON.stringify(result, null, 2));
  ws.close();
  console.log('LIFE_WEB', JSON.stringify({ digest: meta.digest, firstRenderMs, ticks: snap.ticks, draws: snap.draws, raf_median_us: raf && raf.median, errors: result.errors.length }));
}
main().catch(e => {
  fs.writeFileSync(OUT, JSON.stringify({ error: String(e && e.stack || e) }, null, 2));
  console.error('LIFE_WEB_ERROR', e);
  process.exit(1);
});
