//! Scratch 式积木编程页：垂直咬合的事件/动作/条件/循环，输出 JSON 与可读 JS。

pub fn blocks_html() -> String {
    String::from(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Lilyco Ultra UI — 积木编程</title>
<style>
:root{
  --canvas:#f6f7f9;--surface:#fff;--surface2:#f1f3f5;--hair:#e0e3e8;
  --ink:#16191d;--muted:#5f6b7a;--accent:#2563eb;
  --evt:#f59e0b;--act:#3b82f6;--cond:#8b5cf6;--loop:#10b981;
  --mono:ui-monospace,"SF Mono",Consolas,monospace;
  --ui:system-ui,-apple-system,"PingFang SC","Microsoft YaHei",sans-serif;
}
@media(prefers-color-scheme:dark){:root{
  --canvas:#0d1014;--surface:#151a20;--surface2:#1b2128;--hair:#28313b;
  --ink:#e7eaee;--muted:#9aa6b4;--accent:#6ea1ff;
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
main{display:grid;grid-template-columns:190px 1fr 360px;height:calc(100vh - 48px)}
aside{background:var(--surface);border-right:1px solid var(--hair);overflow:auto}
aside.right{border-right:0;border-left:1px solid var(--hair)}
.panel-h{padding:6px 10px;border-bottom:1px solid var(--hair);color:var(--muted);font-size:11.5px;position:sticky;top:0;background:var(--surface)}
.grp{padding:8px}
.grp-t{font:11px var(--mono);color:var(--muted);margin:6px 0 4px}
.pal{display:flex;align-items:center;gap:6px;margin:5px 0;padding:6px 8px;border-radius:8px;color:#fff;font-size:12.5px;cursor:grab;user-select:none}
.pal.evt{background:var(--evt)}.pal.act{background:var(--act)}.pal.cond{background:var(--cond)}.pal.loop{background:var(--loop)}
#canvas{overflow:auto;padding:24px;background:var(--surface2)}
#script{min-height:400px;display:flex;flex-direction:column;gap:6px;max-width:560px}
.block{position:relative;border-radius:10px;color:#fff;padding:0;font-size:13px}
.block .head{display:flex;align-items:center;gap:6px;flex-wrap:wrap;padding:8px 10px;border-radius:10px}
.block.evt .head{background:var(--evt)}.block.act .head{background:var(--act)}
.block.cond .head{background:var(--cond)}.block.loop .head{background:var(--loop)}
.block .head input,.block .head select{height:24px;border:0;border-radius:6px;padding:0 6px;background:rgba(255,255,255,.92);color:#111;font:12px var(--ui)}
.block .head input[type=number]{width:56px}
.block .head input[type=text]{width:110px}
.block .body{margin:0 0 0 18px;padding:6px 0 6px 14px;border-left:10px solid;border-radius:0 0 0 10px;min-height:18px;display:flex;flex-direction:column;gap:6px}
.block.cond .body{border-color:var(--cond)}.block.loop .body{border-color:var(--loop)}
.block .body:empty::after{content:"拖入积木到这里";color:var(--muted);font-size:11px}
.block .body.slot-on{background:rgba(255,255,255,.14)}
.slot{height:6px;margin:1px 0;border-radius:4px}
.slot.on{background:var(--accent);opacity:.85}
.ghost{position:fixed;pointer-events:none;z-index:99;padding:6px 10px;border-radius:8px;background:var(--accent);color:#fff;font-size:12.5px;opacity:.9}
#js{margin:0;padding:12px;font:12px/1.65 var(--mono);white-space:pre-wrap;color:var(--ink)}
.muted{color:var(--muted)}
</style>
</head>
<body>
<header>
  <h1>积木编程 <span class="badge">Scratch-like</span></h1>
  <div class="tools">
    <button onclick="resetScript()">清空</button>
    <button onclick="download('blocks.json', JSON.stringify(script, null, 2), 'application/json')">导出 JSON</button>
    <button class="primary" onclick="download('blocks.js', toJs(script.children, ''), 'text/javascript')">导出 JS</button>
  </div>
</header>
<main>
  <aside><div class="panel-h">积木</div><div id="palette"></div></aside>
  <section id="canvas"><div id="script"></div></section>
  <aside class="right"><div class="panel-h">生成脚本</div><pre id="js"></pre></aside>
</main>
<script>
const GROUPS = [
  { key: 'evt', name: '事件', items: [
    { type: 'when_start', label: '当 开始' },
    { type: 'on_click', label: '当 点击', props: { target: '按钮' } },
  ]},
  { key: 'act', name: '动作', items: [
    { type: 'run_command', label: '运行命令', props: { cmd: 'count' } },
    { type: 'set_var', label: '设置变量', props: { name: 'x', value: '0' } },
    { type: 'show_text', label: '显示文本', props: { text: 'hello' } },
  ]},
  { key: 'cond', name: '条件', items: [
    { type: 'if', label: '如果', props: { name: 'x', op: '>', value: '0' }, container: 'cond' },
  ]},
  { key: 'loop', name: '循环', items: [
    { type: 'repeat', label: '重复', props: { times: 3 }, container: 'loop' },
  ]},
];

const palette = document.getElementById('palette');
const scriptEl = document.getElementById('script');
const jsEl = document.getElementById('js');
let script = { children: [] };
let drag = null;
let ghost = null;

function esc(s){ return String(s == null ? '' : s).replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c])); }
function clone(v){ return JSON.parse(JSON.stringify(v)); }

function renderPalette(){
  palette.innerHTML = GROUPS.map(g => '<div class="grp"><div class="grp-t">' + g.name + '</div>'
    + g.items.map(it => '<div class="pal ' + g.key + '" data-type="' + it.type + '">' + esc(it.label) + '</div>').join('')
    + '</div>').join('');
}

function blockDef(type){
  for (const g of GROUPS) for (const it of g.items) if (it.type === type) return { group: g.key, def: it };
  return null;
}

function blockHtml(b, path){
  const info = blockDef(b.type);
  const g = info.group;
  const p = b.props || {};
  let head = '';
  switch (b.type) {
    case 'when_start': head = '当 ▶ 开始'; break;
    case 'on_click': head = '当 点击 <input type="text" data-k="target" value="' + esc(p.target) + '">'; break;
    case 'run_command': head = '运行命令 <select data-k="cmd">' + ['count','echo','compress'].map(c => '<option' + (c===p.cmd?' selected':'') + '>' + c + '</option>').join('') + '</select>'; break;
    case 'set_var': head = '设置 <input type="text" data-k="name" value="' + esc(p.name) + '"> = <input type="text" data-k="value" value="' + esc(p.value) + '">'; break;
    case 'show_text': head = '显示文本 <input type="text" data-k="text" value="' + esc(p.text) + '">'; break;
    case 'if': head = '如果 <input type="text" data-k="name" value="' + esc(p.name) + '"> <select data-k="op">' + ['>','<','==','!='].map(o => '<option' + (o===p.op?' selected':'') + '>' + o + '</option>').join('') + '</select> <input type="text" data-k="value" value="' + esc(p.value) + '"> 则'; break;
    case 'repeat': head = '重复 <input type="number" data-k="times" value="' + esc(p.times) + '"> 次'; break;
  }
  const body = info.def.container
    ? '<div class="body" data-body="' + path + '">' + (b.children || []).map((c, i) => blockHtml(c, path + '/' + i)).join('') + '</div>'
    : '';
  return '<div class="block ' + g + '" data-type="' + b.type + '" data-path="' + path + '">'
    + '<div class="head">' + head + '</div>' + body + '</div>';
}

function render(){
  scriptEl.innerHTML = script.children.map((b, i) => blockHtml(b, String(i))).join('')
    + '<div class="slot" data-slot="end"></div>';
  jsEl.textContent = toJs(script.children, '');
}

function toJs(list, indent){
  let out = '';
  for (const b of list || []) {
    const p = b.props || {};
    switch (b.type) {
      case 'when_start': out += indent + '// 当开始\n' + toJs(b.children, indent); break;
      case 'on_click': out += indent + '// 当点击 ' + p.target + '\n' + toJs(b.children, indent); break;
      case 'run_command': out += indent + 'await run(' + JSON.stringify(p.cmd) + ');\n'; break;
      case 'set_var': out += indent + p.name + ' = ' + p.value + ';\n'; break;
      case 'show_text': out += indent + 'log(' + JSON.stringify(p.text) + ');\n'; break;
      case 'if': out += indent + 'if (' + p.name + ' ' + p.op + ' ' + p.value + ') {\n' + toJs(b.children, indent + '  ') + indent + '}\n'; break;
      case 'repeat': out += indent + 'for (let i = 0; i < ' + p.times + '; i++) {\n' + toJs(b.children, indent + '  ') + indent + '}\n'; break;
    }
  }
  return out || indent + '// （空脚本）\n';
}

function listAt(path){
  if (path === 'root') return script.children;
  const segs = path.split('/');
  let list = script.children;
  for (const s of segs) { const b = list[Number(s)]; if (!b) return null; list = b.children || (b.children = []); }
  return list;
}

function pathOf(el){
  const block = el.closest('.block');
  return block ? block.dataset.path : null;
}

function insertAt(path, index, block){
  const list = listAt(path);
  if (!list) return;
  list.splice(index, 0, block);
  render();
}

function makeBlock(type){
  const info = blockDef(type);
  if (!info) return null;
  return { type, props: clone(info.def.props || {}), children: [] };
}

palette.addEventListener('pointerdown', e => {
  const it = e.target.closest('[data-type]');
  if (!it) return;
  drag = { kind: 'new', type: it.dataset.type };
  ghost = document.createElement('div');
  ghost.className = 'ghost';
  ghost.textContent = it.textContent;
  ghost.style.left = e.clientX + 12 + 'px';
  ghost.style.top = e.clientY + 8 + 'px';
  document.body.appendChild(ghost);
  it.setPointerCapture(e.pointerId);
});

scriptEl.addEventListener('pointerdown', e => {
  if (e.target.closest('input,select,button')) return;
  const block = e.target.closest('.block');
  if (!block) { drag = { kind: 'none' }; return; }
  const path = block.dataset.path;
  const idx = Number(path.split('/').pop());
  const parent = path.split('/').slice(0, -1).join('/') || 'root';
  drag = { kind: 'move', path, parent, idx };
  block.style.opacity = '0.5';
  scriptEl.setPointerCapture(e.pointerId);
});

document.addEventListener('pointermove', e => {
  if (!drag) return;
  if (ghost) { ghost.style.left = e.clientX + 12 + 'px'; ghost.style.top = e.clientY + 8 + 'px'; }
  for (const s of scriptEl.querySelectorAll('.slot.on')) s.classList.remove('on');
  const hit = document.elementFromPoint(e.clientX, e.clientY);
  const body = hit && hit.closest('[data-body]');
  if (body) {
    body.classList.add('slot-on');
    const list = listAt(body.dataset.body) || [];
    drag.drop = { parent: body.dataset.body, index: list.length };
    return;
  }
  const target = hit && hit.closest('.block');
  if (target) {
    const r = target.getBoundingClientRect();
    const after = e.clientY > r.top + r.height / 2;
    const path = target.dataset.path;
    const idx = Number(path.split('/').pop()) + (after ? 1 : 0);
    const parent = path.split('/').slice(0, -1).join('/') || 'root';
    drag.drop = { parent, index: idx };
  } else {
    const r = scriptEl.getBoundingClientRect();
    if (e.clientX > r.left && e.clientX < r.right && e.clientY > r.top && e.clientY < r.bottom) {
      drag.drop = { parent: 'root', index: script.children.length };
    }
  }
});

document.addEventListener('pointerup', () => {
  if (!drag) return;
  const { kind, drop } = drag;
  if (ghost) { ghost.remove(); ghost = null; }
  if (drop) {
    if (kind === 'new') {
      const b = makeBlock(drag.type);
      if (b) insertAt(drop.parent, drop.index, b);
    } else if (kind === 'move') {
      const list = listAt(drag.parent);
      if (list) {
        const [moved] = list.splice(drag.idx, 1);
        const targetList = listAt(drop.parent);
        let index = drop.index;
        if (drop.parent === drag.parent && drop.index > drag.idx) index--;
        targetList.splice(index, 0, moved);
        render();
      }
    }
  }
  drag = null;
  document.querySelectorAll('.block').forEach(b => b.style.opacity = '');
  render();
});

scriptEl.addEventListener('change', e => {
  const k = e.target.dataset.k;
  if (!k) return;
  const block = e.target.closest('.block');
  if (!block) return;
  const path = block.dataset.path.split('/');
  let list = script.children;
  for (let i = 0; i < path.length - 1; i++) list = list[Number(path[i])].children;
  const b = list[Number(path[path.length - 1])];
  if (!b) return;
  b.props[k] = e.target.type === 'number' ? Number(e.target.value) : e.target.value;
  render();
});

function resetScript(){
  script = { children: [] };
  render();
}

function download(name, text, type){
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([text], { type }));
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

window.__lycoBlocks = {
  script: () => clone(script),
  js: () => toJs(script.children, ''),
  blockCount: () => scriptEl.querySelectorAll('.block').length,
  paletteCount: () => palette.querySelectorAll('[data-type]').length,
  reset: resetScript,
  add: (type) => { const b = makeBlock(type); if (b) insertAt('root', script.children.length, b); },
  set: (s) => { script = clone(s); render(); },
};

renderPalette();
render();
</script>
</body>
</html>"#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_has_scratch_bits() {
        let html = blocks_html();
        for needle in [
            "积木编程",
            "when_start",
            "run_command",
            "if",
            "repeat",
            "toJs",
        ] {
            assert!(html.contains(needle), "缺少 {needle}");
        }
    }

    #[test]
    fn page_is_self_contained() {
        let html = blocks_html();
        assert!(!html.contains("http://"), "不应引用外部 http 资源");
        assert!(!html.contains("<script src"), "不应引用外部脚本");
    }
}
