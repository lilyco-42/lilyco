// CDP 全变体功能矩阵 + 交互状态 + 异常/断网 + 首帧性能（单页单次）。
// 用法: node cdp_matrix.js <mode> <out.json> <url>   CDP_PORT 可覆盖默认 9361
const http = require("http"), fs = require("fs");
const PORT = Number(process.env.CDP_PORT || "9361");
const MODE = process.argv[2], OUT = process.argv[3], URL = process.argv[4];
function gj(p) {
  return new Promise((res, rej) => {
    http.get({ host: "127.0.0.1", port: PORT, path: p }, r => {
      let b = ""; r.on("data", d => b += d); r.on("end", () => { try { res(JSON.parse(b)); } catch (e) { rej(e); } });
    }).on("error", rej);
  });
}
const probe = [
  "(() => {",
  "  const q = s => document.querySelector(s);",
  "  const qa = s => Array.from(document.querySelectorAll(s));",
  "  const win = q('#root .window');",
  "  const els = qa('#root .el');",
  "  return {",
  "    ready: document.readyState,",
  "    rootChildren: q('#root') ? q('#root').children.length : 0,",
  "    windowClass: win ? win.className : null,",
  "    title: q('#root .wtitle') ? q('#root .wtitle').textContent : null,",
  "    elCount: els.length,",
  "    els: els.map(e => ({tag:e.tagName, cls:String(e.className), text:(e.textContent||'').trim().slice(0,160), value:e.value===undefined?null:e.value, checked:e.checked===undefined?null:e.checked, src:e.getAttribute('src'), href:e.getAttribute('href'), width:e.style?e.style.width:null, rows:e.rows||null, min:e.getAttribute('min'), max:e.getAttribute('max'), bar:e.querySelector('.el-pb')?e.querySelector('.el-pb').style.width:null, children:e.children.length})),",
  "    buttons: qa('#root button').map(b=>({text:(b.textContent||'').trim(),cls:String(b.className)})),",
  "    inputs: qa('#root input').map(i=>({type:i.type,value:i.value,checked:i.checked,min:i.min,max:i.max,placeholder:i.placeholder})),",
  "    selects: qa('#root select').map(s=>({value:s.value,options:Array.from(s.options).map(o=>o.value)})),",
  "    textareas: qa('#root textarea').map(t=>({value:t.value,rows:t.rows})),",
  "    images: qa('#root img').map(i=>({src:i.getAttribute('src'),width:i.style.width,complete:i.complete,naturalWidth:i.naturalWidth})),",
  "    links: qa('#root a').map(a=>({href:a.getAttribute('href'),text:(a.textContent||'').trim()})),",
  "    headings: qa('#root .el-heading').map(h=>({tag:h.tagName,cls:String(h.className),text:(h.textContent||'').trim()})),",
  "    bars: qa('#root .el-pb').map(b=>b.style.width),",
  "    calcs: qa('#root .el-calc').map(c=>(c.textContent||'').trim()),",
  "    toast: q('.el-toast') ? (q('.el-toast').textContent||'').trim().slice(0,500) : null",
  "  };",
  "})()"
].join("\n");
const click1 = [
  "new Promise(resolve => {",
  "  const btn = document.querySelector('#root button');",
  "  if (!btn) { resolve({ok:false,reason:'no button'}); return; }",
  "  const obs = new MutationObserver(() => {",
  "    const t = document.querySelector('.el-toast');",
  "    if (t) { obs.disconnect(); resolve({ok:true,latency:performance.now()-t0,text:(t.textContent||'').trim().slice(0,500)}); }",
  "  });",
  "  obs.observe(document.getElementById('root'), {childList:true,subtree:true,characterData:true});",
  "  const t0 = performance.now(); btn.click();",
  "  setTimeout(() => { obs.disconnect(); resolve({ok:false,reason:'timeout'}); }, 3000);",
  "})"
].join("\n");
const setValue = [
  "(() => {",
  "  const el = document.querySelector('#root input.el-input');",
  "  if (!el) return {ok:false,reason:'no input'};",
  "  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;",
  "  setter.call(el,'matrix-ok');",
  "  el.dispatchEvent(new Event('input',{bubbles:true}));",
  "  el.dispatchEvent(new Event('change',{bubbles:true}));",
  "  return {ok:true,value:el.value};",
  "})()"
].join("\n");
const click2 = [
  "new Promise(resolve => {",
  "  const btn = document.querySelector('#root button');",
  "  const t0 = performance.now(); btn.click();",
  "  (function poll(){",
  "    const t = document.querySelector('.el-toast');",
  "    if (t && (t.textContent||'').includes('matrix-ok')) resolve({ok:true,latency:performance.now()-t0,text:(t.textContent||'').trim().slice(0,500)});",
  "    else if (performance.now()-t0 > 3000) resolve({ok:false,reason:'timeout',text:t?(t.textContent||'').trim().slice(0,300):null});",
  "    else setTimeout(poll,10);",
  "  })();",
  "})"
].join("\n");
function matrix(s) {
  const els = s.els || [], has = f => els.some(f);
  return {
    "标题": has(e => e.tag === "H1" && /el-heading/.test(e.cls) && e.text === "十三变体"),
    "文本": has(e => e.tag === "P" && /el-text/.test(e.cls) && e.text.includes("纯 Rust")),
    "输入框": (s.inputs||[]).some(i => i.type === "text" && i.value === "abc" && i.placeholder === "输入"),
    "数字框": (s.inputs||[]).some(i => i.type === "number" && i.value === "7" && i.min === "0" && i.max === "100"),
    "选择框": (s.selects||[]).some(x => x.value === "png" && x.options.join(",") === "png,webp"),
    "复选框": (s.inputs||[]).some(i => i.type === "checkbox" && i.checked === true),
    "文本域": (s.textareas||[]).some(t => t.value === "hello" && t.rows === 3),
    "图片": (s.images||[]).some(i => i.width === "48px" && /document-share\.png/.test(i.src||"")),
    "分隔线": has(e => e.tag === "HR" && /el-divider/.test(e.cls)),
    "进度条": (s.bars||[]).some(b => b === "42%"),
    "链接": (s.links||[]).some(a => a.href === "https://nsh.dev" && a.text === "NSH 官网"),
    "计算器": (s.calcs||[]).some(c => c.includes("计算器") && c.includes("scientific")),
    "按钮": (s.buttons||[]).some(b => b.text === "执行" && /el-btn/.test(b.cls)),
    "标题栏": s.title === "Lilyco 原生全变体烟测"
  };
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
  const evalExpr = (expression, awaitPromise) => send("Runtime.evaluate", { expression, awaitPromise: !!awaitPromise, returnByValue: true });
  const t0 = Date.now();
  await send("Page.navigate", { url: URL });
  let snap = null, firstRenderMs = null;
  while (Date.now() - t0 < 20000) {
    const e = await evalExpr(probe);
    snap = e.result && e.result.result ? e.result.result.value : null;
    if (snap && snap.elCount > 0) { firstRenderMs = Date.now() - t0; break; }
    await new Promise(r => setTimeout(r, 20));
  }
  await new Promise(r => setTimeout(r, 300));
  const perf = await send("Performance.getMetrics");
  const metrics = Object.fromEntries(((perf.result && perf.result.metrics) || []).map(m => [m.name, m.value]));
  const mat = snap ? matrix(snap) : null;
  const interaction = { firstClick: null, setValue: null, secondClick: null };
  if (snap && (snap.buttons || []).length > 0) {
    let q = await evalExpr(click1, true);
    interaction.firstClick = q.result && q.result.result ? q.result.result.value : null;
    q = await evalExpr(setValue);
    interaction.setValue = q.result && q.result.result ? q.result.result.value : null;
    q = await evalExpr(click2, true);
    interaction.secondClick = q.result && q.result.result ? q.result.result.value : null;
  }
  const ex = evs.filter(x => x.method === "Runtime.exceptionThrown").map(x => String((x.params.exceptionDetails.exception && x.params.exceptionDetails.exception.description) || x.params.exceptionDetails.text).split("\n")[0].slice(0, 240));
  const netFail = evs.filter(x => x.method === "Network.loadingFailed").map(x => x.params.errorText);
  const net = { requests: 0, failures: 0, external: 0, bytes: 0 };
  for (const x of evs) {
    if (x.method === "Network.requestWillBeSent") { net.requests++; if (/unpkg\.com/.test(x.params.request.url)) net.external++; }
    if (x.method === "Network.loadingFailed") net.failures++;
    if (x.method === "Network.dataReceived") net.bytes += x.params.dataLength || 0;
  }
  const result = {
    browser: ver.Browser, mode: MODE, url: URL.split("/").pop(),
    firstRenderMs, wallMs: Date.now() - t0, snap,
    matrix: mat, matrixPass: mat ? Object.values(mat).filter(Boolean).length : 0,
    matrixTotal: mat ? Object.keys(mat).length : 14,
    windowClass: snap ? snap.windowClass : null,
    expectedWindowClass: "window large",
    interaction, metrics,
    exceptions: ex.slice(0, 4), netFail: netFail.slice(0, 6), net,
    consoleErrors: evs.filter(x => x.method === "Runtime.consoleAPICalled" && x.params.type === "error").length
  };
  fs.writeFileSync(OUT, JSON.stringify(result, null, 2));
  console.log("MATRIX_DONE", JSON.stringify({ mode: MODE, els: snap && snap.elCount, ms: firstRenderMs, pass: result.matrixPass, click: interaction.firstClick && interaction.firstClick.latency, ex: result.exceptions.length, ext: net.external, fail: net.failures }));
  ws.close(); process.exit(0);
})().catch(e => { console.log("MATRIX_ERR", String(e.stack || e).slice(0, 800)); process.exit(1); });
