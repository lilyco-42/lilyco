//! 图编程画布 v2 — LiteGraph（vendored MIT）+ 六项成熟交互 + 真执行

pub fn graph_html() -> String {
    String::from(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Lilyco Ultra UI — 图编程 v2</title>
<style>
:root{
  --canvas:#f6f7f9;--surface:#fff;--surface2:#f1f3f5;--hair:#e0e3e8;--grid:#e7eaee;
  --ink:#16191d;--muted:#5f6b7a;--accent:#2563eb;--danger:#b3261e;--ok:#0e7a55;
  --mono:ui-monospace,"SF Mono",Consolas,monospace;
  --ui:system-ui,-apple-system,"PingFang SC","Microsoft YaHei",sans-serif;
}
@media(prefers-color-scheme:dark){:root{
  --canvas:#0d1014;--surface:#151a20;--surface2:#1b2128;--hair:#28313b;--grid:#232b34;
  --ink:#e7eaee;--muted:#9aa6b4;--accent:#6ea1ff;--danger:#ff7b72;--ok:#3ddc97;
}}
*{box-sizing:border-box}
body{margin:0;background:var(--canvas);color:var(--ink);font:14px/1.6 var(--ui);overflow:hidden}
header{display:flex;align-items:center;gap:10px;padding:8px 14px;border-bottom:1px solid var(--hair);background:var(--surface)}
h1{margin:0;font-size:15px;font-weight:650}
.badge{font:11px var(--mono);color:var(--accent);border:1px solid var(--hair);border-radius:999px;padding:1px 8px}
.tools{margin-left:auto;display:flex;gap:6px}
button{height:30px;padding:0 10px;border:1px solid var(--hair);border-radius:8px;background:var(--surface);color:var(--ink);cursor:pointer;font-size:13px}
button:hover{border-color:var(--accent);color:var(--accent)}
button.primary{background:var(--accent);border-color:var(--accent);color:#fff}
button:disabled{opacity:.45;cursor:not-allowed}
main{display:grid;grid-template-columns:minmax(340px,34%) 1fr;height:calc(100vh - 48px)}
.left{display:grid;grid-template-rows:auto 1fr auto auto;border-right:1px solid var(--hair);min-width:0}
.bar{padding:5px 12px;border-bottom:1px solid var(--hair);color:var(--muted);font-size:12px}
textarea{margin:0;border:0;resize:none;width:100%;height:100%;padding:12px;background:var(--surface);color:var(--ink);font:12.5px/1.6 var(--mono);outline:none}
#status{padding:7px 12px;border-top:1px solid var(--hair);font:12px var(--mono);color:var(--muted);max-height:96px;overflow:auto;white-space:pre-wrap}
#status.err{color:var(--danger)} #status.ok{color:var(--ok)}
#history{border-top:1px solid var(--hair);max-height:150px;overflow:auto;font:11.5px var(--mono);padding:6px 12px;color:var(--muted)}
#history b{color:var(--ink);font-weight:600}
.right{display:grid;grid-template-rows:auto 1fr}
#c{width:100%;height:100%;display:block;outline:none;touch-action:none}
.hint{color:var(--muted);font:11px var(--mono)}
</style>
</head>
<body>
<header>
  <h1>节点图 <span class="badge">LiteGraph · v2</span></h1>
  <div class="tools">
    <button onclick="loadExample()">载入示例</button>
    <button onclick="addNode()">+ 节点</button>
    <button onclick="autoLayout()">自动布局</button>
    <button onclick="fitView()">适应视图</button>
    <button id="undoBtn" onclick="undo()">撤销</button>
    <button id="redoBtn" onclick="redo()">重做</button>
    <button onclick="check()">校验</button>
    <button class="primary" onclick="runGraph()">运行</button>
  </div>
</header>
<main>
  <div class="left">
    <div class="bar">拖节点 / 框选 / 滚轮缩放 / 空格或中键平移；Ctrl+Z 撤销</div>
    <textarea id="editor" spellcheck="false"></textarea>
    <div id="status">就绪。可框选多个节点、拉线、运行。</div>
    <div id="history"></div>
  </div>
  <div class="right">
    <div class="bar">画布 · 端口类型 * · 选中后方向键 8px 微调</div>
    <canvas id="c" tabindex="0"></canvas>
  </div>
</main>
<script src="/assets/litegraph.js"></script>
<script>
const ed = document.getElementById('editor');
const st = document.getElementById('status');
const hist = document.getElementById('history');
const GRID = 8;
let ARGS = {};
let currentJson = '';
let undoStack = [];
let redoStack = [];
let applying = false;
let edgePointer = {};
let historyRuns = [];
let saveTimer = null;
let keyHits = 0;

const graph = new LiteGraph.LGraph();
const canvas = new LiteGraph.LGraphCanvas('#c', graph);
canvas.background_image = null;
canvas.render_canvas_border = false;
canvas.clear_background_color = getComputedStyle(document.documentElement).getPropertyValue('--surface2').trim() || '#f1f3f5';

function say(msg, cls){ st.textContent = msg; st.className = cls || ''; }
function esc(s){ return String(s).replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c])); }
function byId(id){ return graph._nodes.find(n => n.properties && n.properties.lilyco_id === id); }

function define(cmd, args){
  const Ctor = function(){
    this.addOutput('out', '*');
    args.forEach(a => this.addInput(a, '*'));
    this.properties = { lilyco_id: '', command: cmd, args: {} };
    this.title = cmd;
    this.boxcolor = '#2563eb';
  };
  LiteGraph.registerNodeType('lilyco/' + cmd, Ctor);
}
define('count', ['steps']);
define('echo', ['text']);
define('compress', ['input', 'quality']);

function createNode(id, cmd, args, x, y){
  const n = LiteGraph.createNode('lilyco/' + cmd);
  n.pos = [Number.isFinite(x) ? x : 24, Number.isFinite(y) ? y : 24];
  n.properties.lilyco_id = id;
  n.properties.args = args || {};
  n.title = id + ' · ' + cmd;
  graph.add(n);
  return n;
}

function applySpec(spec){
  applying = true;
  graph.clear();
  canvas.selected_nodes = {};
  canvas.current_node = null;
  canvas.node_dragged = null;
  edgePointer = {};
  const nodes = {};
  (spec.nodes || []).forEach((n, i) => {
    const x = n.x == null ? 24 + i * 230 : n.x;
    const y = n.y == null ? 24 : n.y;
    nodes[n.id] = createNode(n.id, n.command, n.args, x, y);
  });
  for (const e of (spec.edges || [])) {
    const a = nodes[e.from], b = nodes[e.to];
    if (!a || !b) continue;
    const slot = (b.inputs || []).findIndex(i => i.name === e.arg);
    if (slot < 0) continue;
    a.connect(0, b, slot);
    edgePointer[e.from + '|' + e.to + '|' + e.arg] = e.pointer || '/value';
  }
  currentJson = JSON.stringify(spec);
  applying = false;
  canvas.dirty_canvas = true;
}

function toSpec(){
  const nodes = graph._nodes.map(n => ({
    id: n.properties.lilyco_id,
    command: n.properties.command,
    args: n.properties.args || {},
    x: Math.round(n.pos[0]),
    y: Math.round(n.pos[1]),
  }));
  const edges = [];
  for (const link of Object.values(graph.links || {})) {
    const a = graph.getNodeById(link.origin_id);
    const b = graph.getNodeById(link.target_id);
    const input = b && b.inputs && b.inputs[link.target_slot];
    if (!a || !b || !input) continue;
    const key = a.properties.lilyco_id + '|' + b.properties.lilyco_id + '|' + input.name;
    edges.push({
      from: a.properties.lilyco_id,
      to: b.properties.lilyco_id,
      arg: input.name,
      pointer: edgePointer[key] || '/value',
    });
  }
  return { nodes, edges };
}

function syncEditor(){
  if (applying) return;
  const spec = toSpec();
  const s = JSON.stringify(spec);
  if (s === currentJson) return;
  if (currentJson) { undoStack.push(currentJson); if (undoStack.length > 60) undoStack.shift(); }
  redoStack = [];
  currentJson = s;
  ed.value = JSON.stringify(spec, null, 2);
  saveToServer(s);
  updateButtons();
}

let syncTimer = null;
function scheduleSync(){ clearTimeout(syncTimer); syncTimer = setTimeout(syncEditor, 300); }

// 结构/位置变化 → 写回 JSON（轮询兜底，兼容任何 LiteGraph 内部改动）
let lastSer = '';
setInterval(() => {
  if (applying) return;
  const ser = JSON.stringify(graph.serialize());
  if (ser !== lastSer) { lastSer = ser; scheduleSync(); }
}, 350);

function updateButtons(){
  document.getElementById('undoBtn').disabled = undoStack.length === 0;
  document.getElementById('redoBtn').disabled = redoStack.length === 0;
}

function restore(json){
  if (applying || !json) return;
  const cur = JSON.stringify(toSpec());
  applying = true;
  ed.value = json;
  applySpec(JSON.parse(json));
  applying = false;
  currentJson = json;
  lastSer = JSON.stringify(graph.serialize());
  canvas.dirty_canvas = true;
  return cur;
}

function undo(){
  const cur = JSON.stringify(toSpec());
  const prev = undoStack.pop();
  if (!prev) return;
  redoStack.push(cur);
  ed.value = prev;
  applySpec(JSON.parse(prev));
  lastSer = JSON.stringify(graph.serialize());
  canvas.dirty_canvas = true;
  updateButtons();
  say('已撤销', 'ok');
}

function redo(){
  const cur = JSON.stringify(toSpec());
  const next = redoStack.pop();
  if (!next) return;
  undoStack.push(cur);
  ed.value = next;
  applySpec(JSON.parse(next));
  lastSer = JSON.stringify(graph.serialize());
  canvas.dirty_canvas = true;
  updateButtons();
  say('已重做', 'ok');
}

// 批量拖动：在捕获阶段记住选中组（早于 LiteGraph 的重选），拖动期间其他节点跟随
let groupDrag = null;
document.addEventListener('pointerdown', (e) => {
  const c = document.getElementById('c');
  if (e.target !== c) { groupDrag = null; return; }
  const rect = c.getBoundingClientRect();
  const gx = (e.clientX - rect.x) / canvas.ds.scale - canvas.ds.offset[0];
  const gy = (e.clientY - rect.y) / canvas.ds.scale - canvas.ds.offset[1];
  const node = graph.getNodeOnPos(gx, gy, undefined, 6);
  const sel = Object.values(canvas.selected_nodes || {});
  groupDrag = node && sel.length > 1 && sel.includes(node)
    ? { node, origin: [node.pos[0], node.pos[1]], others: sel.filter(n => n !== node).map(n => ({ n, p: [n.pos[0], n.pos[1]] })) }
    : null;
}, true);
// 连线释放：用屏幕坐标做几何命中（与 portPos 同一公式），避免 LiteGraph 内部坐标系差异
document.addEventListener('pointerup', (e) => {
  if (!canvas.connecting_node || !canvas.connecting_output) { window.__linkDiag = { trying: false }; return; }
  const r = document.getElementById('c').getBoundingClientRect();
  const s = canvas.ds.scale;
  let best = null;
  const ports = [];
  for (const n of graph._nodes) {
    (n.inputs || []).forEach((inp, i) => {
      const p = n.getConnectionPos(true, i);
      const sx = r.x + (p[0] + canvas.ds.offset[0]) * s;
      const sy = r.y + (p[1] + canvas.ds.offset[1]) * s;
      const d = Math.hypot(e.clientX - sx, e.clientY - sy);
      ports.push([n.properties.lilyco_id, i, Math.round(d), Math.round(sx), Math.round(sy)]);
      if (d < 40 && (!best || d < best.d)) best = { n, i, d };
    });
  }
  window.__linkDiag = { trying: true, cx: e.clientX, cy: e.clientY, best: best && { id: best.n.properties.lilyco_id, d: Math.round(best.d) }, ports };
  if (best) {
    const src = canvas.connecting_node;
    const outSlot = canvas.connecting_output.slot_index != null
      ? canvas.connecting_output.slot_index
      : src.outputs.indexOf(canvas.connecting_output);
    if (src !== best.n) {
      src.connect(outSlot, best.n, best.i);
      canvas.dirty_canvas = true;
      scheduleSync();
      say('已连线 ' + src.title + ' → ' + best.n.title, 'ok');
    }
  }
  canvas.connecting_node = null;
  canvas.connecting_output = null;
  canvas.connecting_pos = null;
  canvas.dirty_canvas = true;
}, true);

function syncGroupDrag(){
  if (!groupDrag) return;
  const n = groupDrag.node;
  const dx = n.pos[0] - groupDrag.origin[0];
  const dy = n.pos[1] - groupDrag.origin[1];
  if (!dx && !dy) return;
  for (const o of groupDrag.others) { o.n.pos[0] = o.p[0] + dx; o.n.pos[1] = o.p[1] + dy; }
  canvas.dirty_canvas = true;
}
document.addEventListener('pointerup', () => { syncGroupDrag(); groupDrag = null; }, true);
setInterval(syncGroupDrag, 16);

// 对齐参考线：选中节点与其他节点 x/y 差 < 8px 时画虚线
canvas.onDrawForeground = function(ctx){
  const sel = Object.values(canvas.selected_nodes || {});
  if (!sel.length) return;
  const s = sel[0];
  ctx.save();
  ctx.strokeStyle = '#2563eb';
  ctx.lineWidth = 1;
  ctx.setLineDash([4, 4]);
  for (const n of graph._nodes) {
    if (n === s) continue;
    if (Math.abs(n.pos[0] - s.pos[0]) < GRID) {
      window.__alignHits = (window.__alignHits || 0) + 1;
      ctx.beginPath(); ctx.moveTo(n.pos[0], -2000); ctx.lineTo(n.pos[0], 4000); ctx.stroke();
    }
    if (Math.abs(n.pos[1] - s.pos[1]) < GRID) {
      window.__alignHits = (window.__alignHits || 0) + 1;
      ctx.beginPath(); ctx.moveTo(-2000, n.pos[1]); ctx.lineTo(4000, n.pos[1]); ctx.stroke();
    }
  }
  ctx.restore();
};

function fitView(){
  const nodes = graph._nodes;
  if (!nodes.length) return;
  const minX = Math.min(...nodes.map(n => n.pos[0])) - 60;
  const minY = Math.min(...nodes.map(n => n.pos[1])) - 80;
  const maxX = Math.max(...nodes.map(n => n.pos[0] + n.size[0])) + 60;
  const maxY = Math.max(...nodes.map(n => n.pos[1] + n.size[1])) + 80;
  const el = document.getElementById('c');
  const scale = Math.min(el.clientWidth / (maxX - minX), el.clientHeight / (maxY - minY), 1.2);
  canvas.ds.scale = Math.max(0.2, scale);
  canvas.ds.offset = [-minX, -minY];
  canvas.dirty_canvas = true;
}

async function loadExample(){
  const [g, c] = await Promise.all([
    fetch('/api/graph/example').then(r => r.json()),
    fetch('/api/graph/commands').then(r => r.json()),
  ]);
  ARGS = c;
  edgePointer = {};
  undoStack = []; redoStack = [];
  ed.value = JSON.stringify(g, null, 2);
  applySpec(g);
  lastSer = JSON.stringify(graph.serialize());
  fitView();
  updateButtons();
  say('已载入示例：可框选、缩放、Ctrl+Z。', 'ok');
}

async function autoLayout(){
  const spec = toSpec();
  const r = await fetch('/api/graph/layout', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify(spec) });
  const data = await r.json();
  if (!r.ok) { say('布局失败: ' + JSON.stringify(data), 'err'); return; }
  for (const p of data.nodes) {
    const n = byId(p.id);
    if (n) { n.pos = [p.x, p.y]; }
  }
  canvas.dirty_canvas = true;
  syncEditor();
  fitView();
  say('已按执行阶段布局', 'ok');
}

function addNode(){
  const cmd = prompt('命令（count / echo / compress）', 'count');
  if (!cmd) return;
  if (!ARGS[cmd]) { say('未知命令: ' + cmd, 'err'); return; }
  let i = 1, id = cmd;
  while (byId(id)) id = cmd + i++;
  const n = createNode(id, cmd, {}, 24, 24 + graph._nodes.length * 96);
  canvas.selectNodes([n]);
  canvas.dirty_canvas = true;
  syncEditor();
}

async function check(){
  const spec = toSpec();
  const r = await fetch('/api/graph/validate', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify(spec) });
  const v = await r.json();
  if (v.valid) say('✓ 校验通过（' + spec.nodes.length + ' 节点 / ' + spec.edges.length + ' 边）', 'ok');
  else say('✗ ' + v.errors.map(x => JSON.stringify(x)).join('\n'), 'err');
}

async function runGraph(){
  const spec = toSpec();
  const r = await fetch('/api/graph/run', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify(spec) });
  const data = await r.json();
  const now = new Date().toLocaleTimeString();
  if (!r.ok) {
    say('✗ 运行失败: ' + JSON.stringify(data), 'err');
    pushHistory(now, '✗', data.error || 'failed', 0);
    return;
  }
  const res = data.result;
  for (const o of res.outputs) {
    const n = byId(o.id);
    if (n) n.boxcolor = '#0e7a55';
  }
  canvas.dirty_canvas = true;
  say('✓ ' + res.order.join(' → ') + '\n' + JSON.stringify(res.outputs, null, 2), 'ok');
  pushHistory(now, '✓', res.order.join(' → '), res.elapsed_ms);
}

function pushHistory(time, mark, text, ms){
  historyRuns.unshift({ time, mark, text, ms });
  historyRuns = historyRuns.slice(0, 5);
  hist.innerHTML = historyRuns.map(r =>
    '<div><b>' + esc(r.mark) + '</b> ' + esc(r.time) + ' · ' + esc(r.text) + ' · ' + r.ms + 'ms</div>'
  ).join('');
}

let keyboardLinkFrom = null;

document.addEventListener('keydown', e => {
  keyHits++;
  if (e.key === 'Escape') {
    keyboardLinkFrom = null;
    canvas.connecting_node = null;
    canvas.connecting_output = null;
    canvas.dirty_canvas = true;
    return;
  }
  if (e.ctrlKey && e.key.toLowerCase() === 'z') {
    e.preventDefault();
    e.shiftKey ? redo() : undo();
    return;
  }
  if (e.key === 'Escape') {
    canvas.connecting_node = null;
    canvas.connecting_output = null;
    keyboardLinkFrom = null;
    canvas.dirty_canvas = true;
    return;
  }
  if (e.key === 'Tab' && graph._nodes.length) {
    e.preventDefault();
    const ids = graph._nodes.map(n => n.id);
    const cur = sel.length ? sel[0].id : -1;
    const next = graph.getNodeById(ids[(ids.indexOf(cur) + 1) % ids.length]);
    canvas.selectNodes([next]);
    canvas.centerOnNode(next);
    canvas.dirty_canvas = true;
    return;
  }
  if (['ArrowUp','ArrowDown','ArrowLeft','ArrowRight'].includes(e.key) && sel.length) {
    e.preventDefault();
    const d = e.shiftKey ? GRID * 4 : GRID;
    for (const n of sel) {
      if (e.key === 'ArrowUp') n.pos[1] -= d;
      if (e.key === 'ArrowDown') n.pos[1] += d;
      if (e.key === 'ArrowLeft') n.pos[0] -= d;
      if (e.key === 'ArrowRight') n.pos[0] += d;
    }
    canvas.dirty_canvas = true;
    scheduleSync();
    return;
  }
  if ((e.key === 'Delete' || e.key === 'Backspace') && sel.length) {
    e.preventDefault();
    for (const n of sel) graph.remove(n);
    canvas.dirty_canvas = true;
    scheduleSync();
    say('已删除 ' + sel.length + ' 个节点');
  }
});

window.addEventListener('resize', () => { canvas.resize(); canvas.dirty_canvas = true; });

window.__lycoGraph = {
  toSpec, applySpec, undo, redo, autoLayout, fitView, runGraph, addNode,
  state: () => ({ nodes: graph._nodes.length, edges: toSpec().edges.length, scale: canvas.ds.scale, offset: canvas.ds.offset, selected: Object.keys(canvas.selected_nodes || {}).length }),
  selectedCount: () => Object.keys(canvas.selected_nodes || {}).length,
  alignHits: () => window.__alignHits || 0,
  connecting: () => !!(canvas.connecting_node || canvas.connecting_output),
  linkCount: () => Object.keys(graph.links || {}).length,
  keyHits: () => keyHits,
  activeEl: () => (document.activeElement ? document.activeElement.tagName + '#' + (document.activeElement.id || '') : 'none'),
  linkDiag: () => window.__linkDiag,
  graphMouse: () => (canvas.graph_mouse || []).slice(),
  nodeCenter: (id) => {
    const n = byId(id);
    const r = document.getElementById('c').getBoundingClientRect();
    const s = canvas.ds.scale;
    return { x: r.x + (n.pos[0] + n.size[0] / 2 + canvas.ds.offset[0]) * s, y: r.y + (n.pos[1] + 20 + canvas.ds.offset[1]) * s };
  },
  portPos: (id, isInput, slot) => {
    const n = byId(id);
    const p = n.getConnectionPos(!!isInput, slot || 0);
    const r = document.getElementById('c').getBoundingClientRect();
    const s = canvas.ds.scale;
    return { x: r.x + (p[0] + canvas.ds.offset[0]) * s, y: r.y + (p[1] + canvas.ds.offset[1]) * s };
  },
  historyCount: () => historyRuns.length,
};

function saveToServer(json){
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    fetch('/api/graph/save', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: json }).catch(() => {});
  }, 400);
}

(async () => {
  try {
    const r = await fetch('/api/graph/load');
    if (r.ok) {
      const data = await r.json();
      if (data.graph) {
        applySpec(data.graph);
        fitView();
        say('已从后端 graph.json 恢复上次布局', 'ok');
        return;
      }
    }
  } catch (_) {}
  loadExample();
})();
</script>
</body>
</html>"#,
    )
}