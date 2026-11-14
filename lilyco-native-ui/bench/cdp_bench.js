const http = require("http"), fs = require("fs");
const PORT = 9351, MODE = process.argv[2], OUT = process.argv[3], URLS = process.argv.slice(4);
function gj(p) {
  return new Promise((res, rej) => {
    http.get({ host: "127.0.0.1", port: PORT, path: p }, r => {
      let b = ""; r.on("data", d => b += d); r.on("end", () => { try { res(JSON.parse(b)); } catch (e) { rej(e); } });
    }).on("error", rej);
  });
}
(async () => {
  const ver = await gj("/json/version");
  const list = await gj("/json/list");
  const page = list.find(t => t.type === "page");
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  let seq = 0; const pending = new Map(); const evs = [];
  ws.onmessage = e => {
    const m = JSON.parse(e.data);
    if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
    else if (m.method) evs.push(m);
  };
  const send = (method, params) => new Promise(resolve => {
    const id = ++seq; pending.set(id, resolve); ws.send(JSON.stringify({ id, method, params: params || {} }));
  });
  await send("Page.enable"); await send("Runtime.enable");
  await send("Network.enable"); await send("Performance.enable");
  await send("Network.setCacheDisabled", { cacheDisabled: true });
  if (MODE === "block") await send("Network.setBlockedURLs", { urls: ["*unpkg.com*", "*://unpkg.com/*"] });
  const results = [];
  for (const u of URLS) {
    evs.length = 0;
    const t0 = Date.now();
    await send("Page.navigate", { url: u });
    let snap = null, firstRenderMs = null;
    while (Date.now() - t0 < 20000) {
      const e = await send("Runtime.evaluate", {
        expression: "({els:document.querySelectorAll('#root .el').length,btn:document.querySelectorAll('#root button').length,title:document.querySelector('#root .wtitle')&&document.querySelector('#root .wtitle').textContent,ready:document.readyState})",
        returnByValue: true
      });
      snap = e.result && e.result.result ? e.result.result.value : null;
      if (snap && snap.els > 0) { firstRenderMs = Date.now() - t0; break; }
      await new Promise(r => setTimeout(r, 20));
    }
    await new Promise(r => setTimeout(r, 300));
    const perf = await send("Performance.getMetrics");
    const metrics = Object.fromEntries(((perf.result && perf.result.metrics) || []).map(m => [m.name, m.value]));
    const p2 = await send("Runtime.evaluate", {
      expression: "({nodes:document.getElementsByTagName('*').length,heap:performance.memory?performance.memory.usedJSHeapSize:null,els:document.querySelectorAll('#root .el').length})",
      returnByValue: true
    });
    const post = p2.result && p2.result.result ? p2.result.result.value : null;
    let clickLatency = null;
    if (snap && snap.btn > 0) {
      const expr = [
        "new Promise(resolve => {",
        "  const btn = document.querySelector('#root button');",
        "  if (!btn) { resolve(null); return; }",
        "  const obs = new MutationObserver(() => {",
        "    if (document.querySelector('.el-toast')) { obs.disconnect(); resolve(performance.now() - t0); }",
        "  });",
        "  obs.observe(document.getElementById('root'), {childList:true,subtree:true});",
        "  const t0 = performance.now();",
        "  btn.click();",
        "  setTimeout(() => { obs.disconnect(); resolve(null); }, 3000);",
        "})"
      ].join("\n");
      const q = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
      if (q.result && q.result.result) clickLatency = q.result.result.value;
    }
    const ex = evs.filter(x => x.method === "Runtime.exceptionThrown").map(x => String((x.params.exceptionDetails.exception && x.params.exceptionDetails.exception.description) || x.params.exceptionDetails.text).split("\n")[0].slice(0, 200));
    const netFail = evs.filter(x => x.method === "Network.loadingFailed").map(x => x.params.errorText);
    const net = { requests: 0, failures: 0, external: 0, bytes: 0 };
    for (const x of evs) {
      if (x.method === "Network.requestWillBeSent") { net.requests++; if (/unpkg\.com/.test(x.params.request.url)) net.external++; }
      if (x.method === "Network.loadingFailed") net.failures++;
      if (x.method === "Network.dataReceived") net.bytes += x.params.dataLength || 0;
    }
    results.push({
      url: u.split("/").pop(), firstRenderMs, wallMs: Date.now() - t0, snap, post, metrics,
      clickLatency, exceptions: ex.slice(0, 3), netFail: netFail.slice(0, 5), net,
      consoleErrors: evs.filter(x => x.method === "Runtime.consoleAPICalled" && x.params.type === "error").length
    });
  }
  fs.writeFileSync(OUT, JSON.stringify({ browser: ver.Browser, mode: MODE, results }, null, 2));
  console.log("CDP_DONE", JSON.stringify(results.map(r => ({ url: r.url, els: r.snap && r.snap.els, ms: r.firstRenderMs, click: r.clickLatency, ex: r.exceptions.length, ext: r.net.external }))));
  ws.close(); process.exit(0);
})().catch(e => { console.log("CDP_ERR", String(e.stack || e).slice(0, 600)); process.exit(1); });
