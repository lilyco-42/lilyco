//! 拖拽式 UI 原型设计器（Axure 式）：左侧组件库、画布、属性面板、图层、预览、导出。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::spec::{LayoutMode, Position, UiSpec, WindowSize};

/// 设计稿（设计器持久化的唯一格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignDoc {
    pub title: String,
    #[serde(default = "default_size")]
    pub size: String,
    #[serde(default)]
    pub elements: Vec<DesignElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignElement {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub props: Value,
}

fn default_size() -> String {
    "medium".into()
}

impl Default for DesignDoc {
    fn default() -> Self {
        Self {
            title: "Lilyco 原型".into(),
            size: default_size(),
            elements: Vec::new(),
        }
    }
}

impl DesignDoc {
    /// 转换为 UiSpec：元素来自 props + `类型`，位置来自 x/y/w/h/color
    pub fn to_spec(&self) -> Result<UiSpec, String> {
        let mut elements = Vec::with_capacity(self.elements.len());
        let mut positions = Vec::with_capacity(self.elements.len());
        for el in &self.elements {
            let mut obj = match &el.props {
                Value::Object(map) => map.clone(),
                _ => serde_json::Map::new(),
            };
            obj.insert("类型".into(), Value::String(el.kind.clone()));
            elements.push(
                serde_json::from_value::<crate::spec::ElementSpec>(Value::Object(obj))
                    .map_err(|e| format!("元素 {} 转换为 {} 失败: {e}", el.id, el.kind))?,
            );
            positions.push(Position {
                x: el.x,
                y: el.y,
                w: el.w,
                h: el.h,
                color: el.color.clone(),
            });
        }
        Ok(UiSpec {
            window: crate::spec::WindowSpec {
                title: self.title.clone(),
                size: WindowSize::中等,
                elements,
                layout: LayoutMode::Absolute,
                positions,
            },
        })
    }
}

pub fn designer_html() -> String {
    String::from(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Lilyco Ultra UI — 原型设计器</title>
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
main{display:grid;grid-template-columns:168px 1fr 260px;height:calc(100vh - 48px)}
aside{border-right:1px solid var(--hair);overflow:auto;background:var(--surface)}
aside.right{border-right:0;border-left:1px solid var(--hair);display:grid;grid-template-rows:1fr auto}
.panel-h{padding:6px 10px;border-bottom:1px solid var(--hair);color:var(--muted);font-size:11.5px;position:sticky;top:0;background:var(--surface)}
#lib{display:grid;gap:4px;padding:8px}
.lib-item{display:flex;align-items:center;gap:8px;padding:6px 8px;border:1px solid var(--hair);border-radius:8px;background:var(--surface2);cursor:grab;font-size:12.5px}
.lib-item:hover{border-color:var(--accent);color:var(--accent)}
.lib-ico{width:16px;text-align:center;color:var(--accent);font-family:var(--mono)}
#stage-wrap{overflow:auto;background:var(--surface2);position:relative}
#stage{position:relative;width:1200px;height:900px;background:var(--surface);
  background-image:linear-gradient(var(--grid) 1px,transparent 1px),linear-gradient(90deg,var(--grid) 1px,transparent 1px);
  background-size:8px 8px;margin:16px;border:1px solid var(--hair);border-radius:12px;transform-origin:0 0}
.el{position:absolute;padding:6px;border:1px dashed transparent;border-radius:8px;overflow:visible}
.el.sel{border-color:var(--accent);box-shadow:0 0 0 2px color-mix(in srgb,var(--accent) 25%,transparent)}
.el .rz{position:absolute;right:6px;bottom:6px;width:10px;height:10px;border-radius:3px;background:var(--accent);cursor:nwse-resize;display:none}
.el.sel .rz{display:block}
.el p,.el h1,.el h2,.el h3{margin:0}
.el h1{font-size:22px}.el h2{font-size:18px}.el h3{font-size:15px}
.el p{font-size:14px;color:var(--muted)}
.el button{border:0;border-radius:8px;background:var(--accent);color:#fff;padding:8px 14px;font:inherit}
.el button.secondary{background:var(--surface2);color:var(--ink);border:1px solid var(--hair)}
.el button.danger{background:var(--danger)}
.el label{display:block;font-size:12px;color:var(--muted);margin-bottom:4px}
.el input,.el select,.el textarea{width:100%;padding:6px 8px;border:1px solid var(--hair);border-radius:6px;background:var(--surface);color:var(--ink);font:inherit}
.el hr{border:0;border-top:1px solid var(--hair);margin:8px 0}
.el .bar{height:8px;border-radius:999px;background:var(--surface2);overflow:hidden}
.el .bar i{display:block;height:100%;background:var(--accent)}
.el img{max-width:100%;border-radius:6px;display:block}
.el .ph{display:grid;place-items:center;height:100%;border:1px dashed var(--hair);border-radius:8px;color:var(--muted);font-size:12px}
#marquee{position:absolute;border:1px solid var(--accent);background:color-mix(in srgb,var(--accent) 12%,transparent);pointer-events:none}
.align{position:absolute;background:var(--accent);pointer-events:none}
.insp{padding:10px;display:grid;gap:8px;font-size:12.5px}
.row{display:grid;grid-template-columns:56px 1fr;gap:8px;align-items:center}
.row input,.row select{width:100%;padding:5px 7px;border:1px solid var(--hair);border-radius:6px;background:var(--surface);color:var(--ink);font:inherit}
.row input[type=color]{padding:0;height:26px}
.layers{border-top:1px solid var(--hair);max-height:220px;overflow:auto}
.layer{display:flex;align-items:center;gap:6px;padding:4px 8px;border-bottom:1px solid var(--hair);cursor:pointer;font-size:12px}
.layer.on{background:color-mix(in srgb,var(--accent) 12%,transparent)}
.layer .n{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.mini{height:20px;padding:0 6px;font-size:11px;line-height:18px}
#preview{width:100%;height:100%;border:0;background:var(--surface)}
.hide{display:none}
</style>
</head>
<body>
<header>
  <h1>原型设计器 <span class="badge">Axure-like</span></h1>
  <div class="tools">
    <button onclick="saveDoc()">保存</button>
    <button onclick="loadDoc()">加载</button>
    <button id="undoBtn" onclick="undo()">撤销</button>
    <button id="redoBtn" onclick="redo()">重做</button>
    <button onclick="exportHtml()">导出 HTML</button>
    <button onclick="exportJson()">导出 JSON</button>
    <button onclick="document.getElementById('importFile').click()">导入 JSON</button>
    <button onclick="zoomBy(-0.1)">－</button>
    <span id="zoomLabel" style="font:12px var(--mono);color:var(--muted)">100%</span>
    <button onclick="zoomBy(0.1)">＋</button>
    <button onclick="zoomFit()">适应</button>
    <button class="primary" onclick="togglePreview()">预览</button>
  </div>
</header>
<input type="file" id="importFile" accept="application/json" hidden>
<main>
  <aside><div class="panel-h">组件</div><div id="lib"></div></aside>
  <section id="stage-wrap"><div id="stage"></div></section>
  <aside class="right">
    <div><div class="panel-h">属性</div><div id="props" class="insp"></div></div>
    <div class="layers"><div class="panel-h">图层</div><div id="layers"></div></div>
  </aside>
</main>
<iframe id="preview" hidden></iframe>
<script>
const LIB = [
  { type: '标题', ico: 'H', w: 260, h: 44, props: { '内容': '标题', '级别': 2 } },
  { type: '文本', ico: '¶', w: 260, h: 44, props: { '内容': '这是一段文本' } },
  { type: '按钮', ico: '▭', w: 120, h: 44, props: { '文本': '按钮', '动作': 'click', '样式': 'primary' } },
  { type: '输入框', ico: '⌨', w: 260, h: 66, props: { '标签': '标签', '变量': 'input', '占位符': '请输入' } },
  { type: '数字框', ico: '#', w: 260, h: 66, props: { '标签': '数量', '变量': 'num', '默认': 0 } },
  { type: '选择框', ico: '▾', w: 260, h: 66, props: { '标签': '选项', '变量': 'sel', '选项': ['A', 'B'] } },
  { type: '复选框', ico: '☑', w: 180, h: 40, props: { '标签': '同意', '变量': 'cb', '默认': false } },
  { type: '文本域', ico: '≣', w: 260, h: 100, props: { '标签': '备注', '变量': 'ta', '行数': 3 } },
  { type: '图片', ico: '▣', w: 220, h: 130, props: { '链接': '', '宽度': '100%' } },
  { type: '分隔线', ico: '—', w: 260, h: 12, props: {} },
  { type: '进度条', ico: '▰', w: 260, h: 44, props: { '进度': 0.5, '标签': '进度' } },
  { type: '链接', ico: '↗', w: 120, h: 32, props: { '文本': '链接', '链接': '#' } },
  { type: '计算器', ico: '∑', w: 220, h: 60, props: { '变量': 'calc', '模式': 'standard' } },
];

const stage = document.getElementById('stage');
const lib = document.getElementById('lib');
const props = document.getElementById('props');
const layers = document.getElementById('layers');
const preview = document.getElementById('preview');

let doc = { title: 'Lilyco 原型', size: 'medium', elements: [] };
let sel = new Set();
let drag = null, resize = null, marquee = null;
let undoStack = [], redoStack = [];
let seq = 1;
let alignHits = 0;
let alignLines = [];
let previewOn = false;
let zoom = 1;

function esc(s){ return String(s == null ? '' : s).replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c])); }
function byId(id){ return doc.elements.find(e => e.id === id); }

function renderLib(){
  lib.innerHTML = LIB.map(item =>
    '<div class="lib-item" draggable="true" data-type="' + esc(item.type) + '"><span class="lib-ico">' + esc(item.ico) + '</span>' + esc(item.type) + '</div>'
  ).join('');
}

function elHtml(e){
  const p = e.props || {};
  const inner = (() => {
    switch (e.type) {
      case '标题': return '<h2>' + esc(p['内容']) + '</h2>';
      case '文本': return '<p>' + esc(p['内容']) + '</p>';
      case '按钮': return '<button class="' + esc(p['样式'] || 'primary') + '">' + esc(p['文本']) + '</button>';
      case '输入框': return '<label>' + esc(p['标签']) + '</label><input placeholder="' + esc(p['占位符'] || '') + '">';
      case '数字框': return '<label>' + esc(p['标签']) + '</label><input type="number" value="' + esc(p['默认'] || 0) + '">';
      case '选择框': return '<label>' + esc(p['标签']) + '</label><select>' + (p['选项'] || []).map(o => '<option>' + esc(o) + '</option>').join('') + '</select>';
      case '复选框': return '<label style="display:flex;gap:6px;align-items:center"><input type="checkbox" style="width:auto">' + esc(p['标签']) + '</label>';
      case '文本域': return '<label>' + esc(p['标签']) + '</label><textarea rows="' + esc(p['行数'] || 3) + '"></textarea>';
      case '图片': return p['链接'] ? '<img src="' + esc(p['链接']) + '">' : '<div class="ph">图片</div>';
      case '分隔线': return '<hr>';
      case '进度条': return '<div class="bar"><i style="width:' + Math.round((p['进度'] || 0) * 100) + '%"></i></div>';
      case '链接': return '<a href="' + esc(p['链接'] || '#') + '">' + esc(p['文本']) + '</a>';
      case '计算器': return '<div class="ph">计算器</div>';
      default: return '<div class="ph">' + esc(e.type) + '</div>';
    }
  })();
  const style = 'left:' + e.x + 'px;top:' + e.y + 'px;width:' + e.w + 'px;height:' + e.h + 'px;' + (e.color ? 'color:' + esc(e.color) + ';' : '');
  return '<div class="el" data-id="' + esc(e.id) + '" style="' + style + '">' + inner + '<span class="rz" data-resize="1"></span></div>';
}

function renderStage(){
  stage.innerHTML = doc.elements.map(elHtml).join('');
  for (const id of sel) {
    const el = stage.querySelector('[data-id="' + id + '"]');
    if (el) el.classList.add('sel');
  }
  if (marquee) {
    const m = document.createElement('div');
    m.id = 'marquee';
    m.style.left = marquee.x1 + 'px'; m.style.top = marquee.y1 + 'px';
    m.style.width = Math.abs(marquee.x2 - marquee.x1) + 'px';
    m.style.height = Math.abs(marquee.y2 - marquee.y1) + 'px';
    stage.appendChild(m);
  }
  renderProps();
  renderLayers();
  drawAlignLines();
  updateButtons();
}

function drawAlignLines(){
  for (const old of Array.from(stage.querySelectorAll('.align'))) old.remove();
  if (!alignLines.length) return;
  const html = alignLines.map(l => l.type === 'v'
    ? '<div class="align" style="left:' + l.x + 'px;top:0;width:1px;height:100%"></div>'
    : '<div class="align" style="top:' + l.y + 'px;left:0;height:1px;width:100%"></div>'
  ).join('');
  stage.insertAdjacentHTML('beforeend', html);
}

function renderProps(){
  const one = sel.size === 1 ? byId([...sel][0]) : null;
  if (!one) { props.innerHTML = '<div class="muted">选中一个元素后编辑属性</div>'; return; }
  const textKey = ({ '标题': '内容', '文本': '内容', '按钮': '文本', '输入框': '标签', '数字框': '标签', '选择框': '标签', '复选框': '标签', '文本域': '标签', '链接': '文本' })[one.type];
  const rows = [];
  if (textKey) rows.push(row('文本', '<input data-k="text" value="' + esc(one.props[textKey]) + '">'));
  rows.push(row('X', '<input type="number" data-k="x" value="' + one.x + '">'));
  rows.push(row('Y', '<input type="number" data-k="y" value="' + one.y + '">'));
  rows.push(row('宽', '<input type="number" data-k="w" value="' + one.w + '">'));
  rows.push(row('高', '<input type="number" data-k="h" value="' + one.h + '">'));
  rows.push(row('颜色', '<input type="color" data-k="color" value="' + (one.color || '#16191d') + '">'));
  props.innerHTML = rows.join('') + '<div class="muted" style="margin-top:6px">' + esc(one.id) + ' · ' + esc(one.type) + '</div>';
}

function row(label, input){
  return '<div class="row"><span>' + esc(label) + '</span>' + input + '</div>';
}

function renderLayers(){
  layers.innerHTML = doc.elements.slice().reverse().map(e =>
    '<div class="layer' + (sel.has(e.id) ? ' on' : '') + '" data-layer="' + esc(e.id) + '"><span class="n">' + esc(e.type) + '</span><span class="muted">' + esc(e.id) + '</span>'
    + '<button class="mini" data-up="' + esc(e.id) + '" title="上移">↑</button>'
    + '<button class="mini" data-down="' + esc(e.id) + '" title="下移">↓</button></div>'
  ).join("");
}

function updateButtons(){
  document.getElementById('undoBtn').disabled = undoStack.length === 0;
  document.getElementById('redoBtn').disabled = redoStack.length === 0;
}

function snapshot(){ undoStack.push(JSON.stringify(doc)); if (undoStack.length > 80) undoStack.shift(); redoStack = []; updateButtons(); }

function undo(){
  const prev = undoStack.pop();
  if (!prev) return;
  redoStack.push(JSON.stringify(doc));
  doc = JSON.parse(prev);
  sel.clear();
  renderStage();
}

function redo(){
  const next = redoStack.pop();
  if (!next) return;
  undoStack.push(JSON.stringify(doc));
  doc = JSON.parse(next);
  sel.clear();
  renderStage();
}

function addAt(type, x, y){
  const item = LIB.find(i => i.type === type);
  if (!item) return;
  snapshot();
  const id = type + (seq++);
  doc.elements.push({ id, type, x: Math.round(x / 8) * 8, y: Math.round(y / 8) * 8, w: item.w, h: item.h, color: null, props: JSON.parse(JSON.stringify(item.props)) });
  sel = new Set([id]);
  renderStage();
}

lib.addEventListener('dragstart', e => {
  const it = e.target.closest('[data-type]');
  if (it) e.dataTransfer.setData('text/lilyco-type', it.dataset.type);
});
lib.addEventListener('click', e => {
  const it = e.target.closest('[data-type]');
  if (it && !libDragMoved) addAt(it.dataset.type, 32, 32 + doc.elements.length * 24);
});

// 指针拖拽（HTML5 DnD 在 headless CDP 里不稳，这里自实现）
let libDrag = null, libDragMoved = false;
lib.addEventListener('pointerdown', e => {
  const it = e.target.closest('[data-type]');
  if (!it) return;
  libDrag = { type: it.dataset.type, x: e.clientX, y: e.clientY };
  libDragMoved = false;
  it.setPointerCapture(e.pointerId);
});
lib.addEventListener('pointermove', e => {
  if (!libDrag) return;
  if (Math.hypot(e.clientX - libDrag.x, e.clientY - libDrag.y) > 4) libDragMoved = true;
});
lib.addEventListener('pointerup', e => {
  if (!libDrag) return;
  const r = stage.getBoundingClientRect();
  const inside = e.clientX > r.left && e.clientX < r.right && e.clientY > r.top && e.clientY < r.bottom;
  if (libDragMoved && inside) addAt(libDrag.type, e.clientX - r.left - 40, e.clientY - r.top - 20);
  libDrag = null;
  setTimeout(() => { libDragMoved = false; }, 0);
});
stage.addEventListener('dragover', e => e.preventDefault());
stage.addEventListener('drop', e => {
  e.preventDefault();
  const type = e.dataTransfer.getData('text/lilyco-type');
  if (!type) return;
  const r = stage.getBoundingClientRect();
  addAt(type, e.clientX - r.left - 40, e.clientY - r.top - 20);
});

stage.addEventListener('pointerdown', e => {
  const handle = e.target.closest('[data-resize]');
  const el = e.target.closest('[data-id]');
  const r = stage.getBoundingClientRect();
  if (handle && el) {
    const one = byId(el.dataset.id);
    snapshot();
    resize = { id: one.id, sx: e.clientX, sy: e.clientY, w0: one.w, h0: one.h };
  } else if (el) {
    const id = el.dataset.id;
    if (!sel.has(id)) sel = e.shiftKey ? new Set([...sel, id]) : new Set([id]);
    const one = byId(id);
    snapshot();
    drag = { id, sx: e.clientX, sy: e.clientY, origins: [...sel].map(sid => { const o = byId(sid); return { id: sid, x: o.x, y: o.y }; }) };
    renderStage();
  } else {
    sel.clear();
    marquee = { x1: e.clientX - r.left, y1: e.clientY - r.top, x2: e.clientX - r.left, y2: e.clientY - r.top };
    renderStage();
  }
  stage.setPointerCapture(e.pointerId);
});

stage.addEventListener('pointermove', e => {
  if (drag) {
    const dx = e.clientX - drag.sx, dy = e.clientY - drag.sy;
    for (const o of drag.origins) {
      const one = byId(o.id);
      one.x = Math.round((o.x + dx) / 8) * 8;
      one.y = Math.round((o.y + dy) / 8) * 8;
    }
    alignHits += snap(drag.id);
    renderStage();
    drawAlignLines();
  } else if (resize) {
    const one = byId(resize.id);
    one.w = Math.max(40, Math.round((resize.w0 + e.clientX - resize.sx) / 8) * 8);
    one.h = Math.max(24, Math.round((resize.h0 + e.clientY - resize.sy) / 8) * 8);
    renderStage();
  } else if (marquee) {
    const r = stage.getBoundingClientRect();
    marquee.x2 = e.clientX - r.left; marquee.y2 = e.clientY - r.top;
    renderStage();
  }
});

stage.addEventListener('pointerup', () => {
  if (marquee) {
    const x1 = Math.min(marquee.x1, marquee.x2), x2 = Math.max(marquee.x1, marquee.x2);
    const y1 = Math.min(marquee.y1, marquee.y2), y2 = Math.max(marquee.y1, marquee.y2);
    sel = new Set(doc.elements.filter(e => e.x < x2 && e.x + e.w > x1 && e.y < y2 && e.y + e.h > y1).map(e => e.id));
  }
  drag = null; resize = null; marquee = null;
  alignLines = [];
  renderStage();
  if (previewOn) renderPreview();
});

function snap(activeId){
  const active = byId(activeId);
  if (!active) return 0;
  let hits = 0;
  for (const other of doc.elements) {
    if (other.id === activeId) continue;
    if (Math.abs(other.x - active.x) < 8) { active.x = other.x; hits++; alignLines.push({ type: 'v', x: other.x }); }
    if (Math.abs(other.y - active.y) < 8) { active.y = other.y; hits++; alignLines.push({ type: 'h', y: other.y }); }
  }
  if (hits) drawAlignLines();
  return hits;
}

function zoomBy(d){
  zoom = Math.max(0.25, Math.min(3, Math.round((zoom + d) * 100) / 100));
  applyView();
}
function zoomFit(){ zoom = 1; applyView(); }
function applyView(){
  stage.style.transform = 'scale(' + zoom + ')';
  const label = document.getElementById('zoomLabel');
  if (label) label.textContent = Math.round(zoom * 100) + '%';
}

function importJson(text){
  try {
    const d = JSON.parse(text);
    if (!d || !Array.isArray(d.elements)) throw new Error('缺少 elements 数组');
    snapshot();
    doc = d;
    sel.clear();
    renderStage();
  } catch (err) {
    alert('导入失败: ' + err.message);
  }
}

props.addEventListener('change', e => {
  const k = e.target.dataset.k;
  if (!k || sel.size !== 1) return;
  const one = byId([...sel][0]);
  snapshot();
  const v = e.target.value;
  if (k === 'x') one.x = Number(v);
  else if (k === 'y') one.y = Number(v);
  else if (k === 'w') one.w = Math.max(40, Number(v));
  else if (k === 'h') one.h = Math.max(24, Number(v));
  else if (k === 'color') one.color = v;
  else if (k === 'text') {
    const key = ({ '标题': '内容', '文本': '内容', '按钮': '文本', '输入框': '标签', '数字框': '标签', '选择框': '标签', '复选框': '标签', '文本域': '标签', '链接': '文本' })[one.type] || '内容';
    one.props[key] = v;
  }
  renderStage();
  if (previewOn) renderPreview();
});

layers.addEventListener('click', e => {
  const up = e.target.closest('[data-up]');
  const down = e.target.closest('[data-down]');
  if (up) { e.stopPropagation(); moveLayer(up.dataset.up, 1); return; }
  if (down) { e.stopPropagation(); moveLayer(down.dataset.down, -1); return; }
  const l = e.target.closest('[data-layer]');
  if (!l) return;
  sel = new Set([l.dataset.layer]);
  renderStage();
});

function moveLayer(id, dir){
  const i = doc.elements.findIndex(e => e.id === id);
  const j = i + dir;
  if (i < 0 || j < 0 || j >= doc.elements.length) return;
  snapshot();
  const tmp = doc.elements[i];
  doc.elements[i] = doc.elements[j];
  doc.elements[j] = tmp;
  renderStage();
}

function detailHtml(){
  const els = doc.elements.map(e => {
    const style = 'position:absolute;left:' + e.x + 'px;top:' + e.y + 'px;width:' + e.w + 'px;height:' + e.h + 'px;' + (e.color ? 'color:' + esc(e.color) + ';' : '');
    const inner = (() => {
      const p = e.props || {};
      switch (e.type) {
        case '标题': return '<h2>' + esc(p['内容']) + '</h2>';
        case '文本': return '<p>' + esc(p['内容']) + '</p>';
        case '按钮': return '<button>' + esc(p['文本']) + '</button>';
        case '输入框': return '<label>' + esc(p['标签']) + '</label><input placeholder="' + esc(p['占位符'] || '') + '">';
        case '数字框': return '<label>' + esc(p['标签']) + '</label><input type="number">';
        case '选择框': return '<label>' + esc(p['标签']) + '</label><select>' + (p['选项'] || []).map(o => '<option>' + esc(o) + '</option>').join('') + '</select>';
        case '复选框': return '<label><input type="checkbox">' + esc(p['标签']) + '</label>';
        case '文本域': return '<label>' + esc(p['标签']) + '</label><textarea></textarea>';
        case '图片': return '<div class="ph">图片</div>';
        case '分隔线': return '<hr>';
        case '进度条': return '<div class="bar"><i style="width:' + Math.round((p['进度'] || 0) * 100) + '%"></i></div>';
        case '链接': return '<a>' + esc(p['文本']) + '</a>';
        default: return '<div class="ph">' + esc(e.type) + '</div>';
      }
    })();
    return '<div style="' + style + '">' + inner + '</div>';
  }).join('');
  return '<!DOCTYPE html><html><head><meta charset="utf-8"><style>body{margin:0;font-family:system-ui,sans-serif;background:#f6f7f9}'
    + '#w{position:relative;width:1200px;height:900px;background:#fff;margin:0 auto}'
    + '#w h2{font-size:22px;margin:0}#w p{color:#5f6b7a;margin:0}'
    + '#w button{border:0;border-radius:8px;background:#2563eb;color:#fff;padding:8px 14px;font:inherit}'
    + '#w label{display:block;font-size:12px;color:#5f6b7a}#w input,#w select,#w textarea{width:100%;padding:6px;border:1px solid #e0e3e8;border-radius:6px}'
    + '#w .bar{height:8px;background:#e7eaee;border-radius:999px;overflow:hidden}#w .bar i{display:block;height:100%;background:#2563eb}'
    + '#w .ph{display:grid;place-items:center;height:100%;border:1px dashed #e0e3e8;color:#5f6b7a}'
    + '</style></head><body><div id="w"><h3>' + esc(doc.title) + '</h3>' + els + '</div></body></html>';
}

function renderPreview(){
  const html = detailHtml();
  window.__lastPreviewHtml = html;
  preview.srcdoc = html;
  try {
    const d = preview.contentDocument;
    if (d) { d.open(); d.write(html); d.close(); }
  } catch (_) {}
}

function togglePreview(){
  previewOn = !previewOn;
  preview.hidden = !previewOn;
  if (previewOn) { renderPreview(); stage.parentElement.style.display = 'none'; }
  else { stage.parentElement.style.display = ''; }
  preview.style.position = 'absolute';
  preview.style.inset = '0';
}

async function saveDoc(){
  const r = await fetch('/api/designer/save', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(doc) });
  const d = await r.json();
  alert(d.ok ? '已保存到 ' + d.path : '保存失败: ' + JSON.stringify(d));
}

async function loadDoc(){
  const r = await fetch('/api/designer/load');
  if (!r.ok) { alert('还没有保存的设计稿'); return; }
  const d = await r.json();
  if (!d.ok) { alert('加载失败'); return; }
  undoStack.push(JSON.stringify(doc));
  doc = d.doc;
  sel.clear();
  renderStage();
}

async function exportHtml(){
  const r = await fetch('/api/designer/export', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(doc) });
  const d = await r.json();
  if (!d.ok) { alert('导出失败: ' + JSON.stringify(d)); return; }
  download('prototype.html', d.html, 'text/html');
}

function exportJson(){
  download('prototype.design.json', JSON.stringify(doc, null, 2), 'application/json');
}

document.getElementById('importFile').addEventListener('change', async e => {
  const f = e.target.files && e.target.files[0];
  if (!f) return;
  importJson(await f.text());
  e.target.value = '';
});

function download(name, text, type){
  const blob = new Blob([text], { type });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

document.addEventListener('keydown', e => {
  if ((e.key === 'Delete' || e.key === 'Backspace') && sel.size && document.activeElement.tagName !== 'INPUT') {
    e.preventDefault();
    snapshot();
    doc.elements = doc.elements.filter(x => !sel.has(x.id));
    sel.clear();
    renderStage();
  }
  if (e.ctrlKey && e.key.toLowerCase() === 'z') { e.preventDefault(); e.shiftKey ? redo() : undo(); }
});

window.__lycoDesigner = {
  doc: () => JSON.parse(JSON.stringify(doc)),
  selected: () => sel.size,
  alignHits: () => alignHits,
  addAt, undo, redo, saveDoc, loadDoc, exportHtml, togglePreview, renderPreview, moveLayer,
  importJson, zoomBy, zoomFit,
  zoomValue: () => zoom,
  alignCount: () => stage.querySelectorAll('.align').length,
  setDoc: (d) => { doc = d; sel.clear(); renderStage(); },
  elementScreen: (id) => { const r = stage.querySelector('[data-id="' + id + '"]').getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; },
  handleScreen: (id) => { const r = stage.querySelector('[data-id="' + id + '"] [data-resize]').getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; },
  stageRect: () => { const r = stage.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; },
  previewText: () => {
    if (window.__lastPreviewHtml) return window.__lastPreviewHtml;
    const p = document.getElementById('preview');
    if (!p) return '';
    try {
      const body = p.contentDocument && p.contentDocument.body;
      if (body && body.innerHTML) return body.innerHTML;
    } catch (_) {}
    return p.srcdoc || '';
  },
};

renderLib();
renderStage();
loadDoc().catch(() => {});
</script>
</body>
</html>"#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> DesignDoc {
        DesignDoc {
            title: "原型".into(),
            size: "medium".into(),
            elements: vec![DesignElement {
                id: "btn1".into(),
                kind: "按钮".into(),
                x: 24.0,
                y: 32.0,
                w: 120.0,
                h: 44.0,
                color: Some("#ff0000".into()),
                props: serde_json::json!({ "文本": "确定", "动作": "ok", "样式": "primary" }),
            }],
        }
    }

    #[test]
    fn converts_to_absolute_spec() {
        let spec = doc().to_spec().unwrap();
        assert_eq!(spec.window.layout, LayoutMode::Absolute);
        assert_eq!(spec.window.elements.len(), 1);
        assert_eq!(spec.window.positions.len(), 1);
        assert_eq!(spec.window.positions[0].x, 24.0);
        assert_eq!(spec.window.positions[0].color.as_deref(), Some("#ff0000"));
    }

    #[test]
    fn spec_json_uses_documented_keys() {
        let spec = doc().to_spec().unwrap();
        let v = serde_json::to_value(&spec).unwrap();
        assert_eq!(v["窗口"]["布局"], "absolute");
        assert_eq!(v["窗口"]["位置"][0]["x"], 24.0);
        assert_eq!(v["窗口"]["元素"][0]["类型"], "按钮");
    }

    #[test]
    fn invalid_kind_is_reported_with_id() {
        let mut d = doc();
        d.elements[0].kind = "不存在的类型".into();
        let err = d.to_spec().unwrap_err();
        assert!(err.contains("btn1") && err.contains("不存在的类型"), "{err}");
    }

    #[test]
    fn empty_doc_is_allowed() {
        let spec = DesignDoc::default().to_spec().unwrap();
        assert!(spec.window.elements.is_empty());
    }
}