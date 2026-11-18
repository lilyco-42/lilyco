//! Android Layout Editor 式原型设计器：GrapesJS 底座（vendor 内嵌）+ 蓝图/约束/设备/对齐/模板。

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
    /// 约束锚点：`left-top` / `center-middle` / `right-bottom` …
    #[serde(default)]
    pub anchor: Option<String>,
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
        r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Lilyco Ultra UI — 原型设计器（GrapesJS）</title>
<style>
:root{
  --canvas:#f6f7f9;--surface:#fff;--surface2:#f1f3f5;--hair:#e0e3e8;
  --ink:#16191d;--muted:#5f6b7a;--accent:#2563eb;
  --mono:ui-monospace,"SF Mono",Consolas,monospace;
  --ui:system-ui,-apple-system,"PingFang SC","Microsoft YaHei",sans-serif;
}
@media(prefers-color-scheme:dark){:root{
  --canvas:#0d1014;--surface:#151a20;--surface2:#1b2128;--hair:#28313b;
  --ink:#e7eaee;--muted:#9aa6b4;--accent:#6ea1ff;
}}
*{box-sizing:border-box}
html,body{height:100%}
body{margin:0;background:var(--canvas);color:var(--ink);font:14px/1.6 var(--ui);overflow:hidden}
header{display:flex;align-items:center;gap:8px;padding:8px 12px;border-bottom:1px solid var(--hair);background:var(--surface);flex-wrap:wrap}
h1{margin:0;font-size:15px;font-weight:650}
.badge{font:11px var(--mono);color:var(--accent);border:1px solid var(--hair);border-radius:999px;padding:1px 8px}
.tools{margin-left:auto;display:flex;gap:6px;flex-wrap:wrap;align-items:center}
button{height:30px;padding:0 10px;border:1px solid var(--hair);border-radius:8px;background:var(--surface);color:var(--ink);cursor:pointer;font-size:13px}
button:hover{border-color:var(--accent);color:var(--accent)}
button.primary{background:var(--accent);border-color:var(--accent);color:#fff}
select{height:30px;border:1px solid var(--hair);border-radius:8px;background:var(--surface);color:var(--ink);font-size:13px;padding:0 6px}
main{display:grid;grid-template-columns:170px 1fr 290px;height:calc(100vh - 49px)}
aside{border-right:1px solid var(--hair);background:var(--surface);overflow:auto}
aside.right{border-right:0;border-left:1px solid var(--hair);display:grid;grid-template-rows:1fr 1fr;min-height:0}
.panel-h{padding:6px 10px;border-bottom:1px solid var(--hair);color:var(--muted);font-size:11.5px;position:sticky;top:0;background:var(--surface);z-index:2}
#gjs{position:relative;overflow:auto;background:var(--surface2);padding:16px}
#blocks,#layers,#styles{min-height:0;overflow:auto}
#styles{border-top:1px solid var(--hair)}
.gjs-blocks-c{display:grid;gap:4px;padding:8px}
.gjs-block{width:auto!important;min-height:auto!important;padding:6px 8px!important;border:1px solid var(--hair)!important;border-radius:8px!important;background:var(--surface2)!important;color:var(--ink)!important;font-size:12.5px!important;box-shadow:none!important}
.gjs-block:hover{border-color:var(--accent)!important;color:var(--accent)!important}
.gjs-layer{font-size:12px}
.gjs-sm-sector{font-size:12px}
#status{padding:4px 12px;color:var(--muted);font:11.5px var(--mono);border-top:1px solid var(--hair);background:var(--surface)}
#preview{position:fixed;inset:49px 0 0 0;background:var(--surface);border:0;z-index:50}
[hidden]{display:none!important}
</style>
</head>
<body>
<header>
  <h1>原型设计器 <span class="badge">GrapesJS · Layout Editor</span></h1>
  <div class="tools">
    <button id="viewBtn" onclick="toggleBlueprint()">蓝图</button>
    <select id="deviceSel" onchange="setDevice(this.value)">
      <option value="phone">手机 390×844</option>
      <option value="tablet">平板 820×1180</option>
      <option value="custom">自定义 600×900</option>
    </select>
    <button onclick="alignLeft()">左对齐</button>
    <button onclick="distributeH()">水平分布</button>
    <button onclick="loadTemplate()">登录页模板</button>
    <button onclick="saveDoc()">保存</button>
    <button onclick="loadDoc()">加载</button>
    <button onclick="exportJson()">导出 JSON</button>
    <button class="primary" onclick="exportHtml()">导出 HTML</button>
    <button onclick="togglePreview()">预览</button>
  </div>
</header>
<main>
  <aside><div class="panel-h">组件</div><div id="blocks"></div></aside>
  <section><div id="gjs"></div></section>
  <aside class="right">
    <div><div class="panel-h">组件树</div><div id="layers"></div></div>
    <div><div class="panel-h">属性 / 约束</div><div id="styles"></div></div>
  </aside>
</main>
<div id="status">就绪 · 拖组件到画布 · 选中后可设锚点</div>
<iframe id="preview" hidden></iframe>

<script src="/assets/grapesjs.min.js"></script>
<script>
const KIND = {
  '标题':   { w: 280, h: 48,  inner: '<h2 style="margin:0;font-size:22px;line-height:1.3">标题</h2>' },
  '文本':   { w: 280, h: 44,  inner: '<p style="margin:0;color:#5f6b7a">这是一段文本</p>' },
  '按钮':   { w: 140, h: 46,  inner: '<button style="border:0;border-radius:8px;background:#2563eb;color:#fff;padding:10px 16px;font:inherit">按钮</button>' },
  '输入框': { w: 280, h: 74,  inner: '<label style="display:block;font-size:12px;color:#5f6b7a;margin-bottom:4px">标签</label><input style="width:100%;padding:8px;border:1px solid #e0e3e8;border-radius:6px" placeholder="请输入">' },
  '数字框': { w: 280, h: 74,  inner: '<label style="display:block;font-size:12px;color:#5f6b7a;margin-bottom:4px">数量</label><input type="number" value="0" style="width:100%;padding:8px;border:1px solid #e0e3e8;border-radius:6px">' },
  '选择框': { w: 280, h: 74,  inner: '<label style="display:block;font-size:12px;color:#5f6b7a;margin-bottom:4px">选项</label><select style="width:100%;padding:8px;border:1px solid #e0e3e8;border-radius:6px"><option>A</option><option>B</option></select>' },
  '复选框': { w: 180, h: 40,  inner: '<label style="display:flex;gap:8px;align-items:center;font-size:13px"><input type="checkbox">同意</label>' },
  '文本域': { w: 280, h: 110, inner: '<label style="display:block;font-size:12px;color:#5f6b7a;margin-bottom:4px">备注</label><textarea rows="3" style="width:100%;padding:8px;border:1px solid #e0e3e8;border-radius:6px"></textarea>' },
  '图片':   { w: 220, h: 140, inner: '<div style="display:grid;place-items:center;height:100%;border:1px dashed #e0e3e8;border-radius:8px;color:#5f6b7a;font-size:12px">图片</div>' },
  '分隔线': { w: 280, h: 14,  inner: '<hr style="border:0;border-top:1px solid #e0e3e8;margin:6px 0">' },
  '进度条': { w: 280, h: 46,  inner: '<div style="height:8px;border-radius:999px;background:#e7eaee;overflow:hidden"><div style="height:100%;width:50%;background:#2563eb"></div></div>' },
  '链接':   { w: 120, h: 32,  inner: '<a href="#" style="color:#2563eb">链接</a>' },
  '计算器': { w: 220, h: 64,  inner: '<div style="display:grid;place-items:center;height:100%;border:1px dashed #e0e3e8;border-radius:8px;color:#5f6b7a;font-size:12px">计算器</div>' },
  '卡片':   { w: 360, h: 480, inner: '<div style="position:absolute;inset:0;border:1px solid #e0e3e8;border-radius:12px;background:#fff"></div>' },
};

function boxHtml(kind, props, anchor, x, y, w, h, children){
  const k = KIND[kind] || { w: 200, h: 40, inner: '<div>' + kind + '</div>' };
  const pw = w || k.w, ph = h || k.h;
  const p = props || {};
  let inner = k.inner;
  if (p['内容'] && kind === '标题') inner = '<h2 style="margin:0;font-size:22px;line-height:1.3">' + p['内容'] + '</h2>';
  if (p['内容'] && kind === '文本') inner = '<p style="margin:0;color:#5f6b7a">' + p['内容'] + '</p>';
  if (p['文本'] && kind === '按钮') inner = '<button style="border:0;border-radius:8px;background:#2563eb;color:#fff;padding:10px 16px;font:inherit">' + p['文本'] + '</button>';
  if (p['标签'] && (kind === '输入框' || kind === '数字框')) inner = inner.replace('>标签<', '>' + p['标签'] + '<');
  return '<div data-kind="' + kind + '" data-id="' + (p['id'] || '') + '" data-anchor="' + (anchor || 'left-top') + '" '
    + 'data-props="' + JSON.stringify(p).replace(/"/g, '&quot;') + '" '
    + 'style="position:absolute;left:' + (x == null ? 20 : x) + 'px;top:' + (y == null ? 20 : y) + 'px;width:' + pw + 'px;height:' + ph + 'px;box-sizing:border-box;padding:4px">'
    + inner + (children || '') + '</div>';
}

const BLOCKS = Object.keys(KIND).map(kind => ({
  id: kind,
  label: kind,
  category: '组件',
  content: boxHtml(kind, {}, 'left-top', 20, 20),
  attributes: { title: kind },
}));

const editor = grapesjs.init({
  container: '#gjs',
  height: '100%',
  width: 'auto',
  storageManager: false,
  fromElement: false,
  canvas: { styles: [], scripts: [] },
  cssIcons: '',
  deviceManager: {
    devices: [
      { id: 'phone', name: '手机 390×844', width: '390px', height: '844px', widthMedia: '390px' },
      { id: 'tablet', name: '平板 820×1180', width: '820px', height: '1180px', widthMedia: '820px' },
      { id: 'custom', name: '自定义 600×900', width: '600px', height: '900px', widthMedia: '600px' },
    ],
  },
  blockManager: { appendTo: '#blocks', blocks: BLOCKS },
  layerManager: { appendTo: '#layers' },
  styleManager: { appendTo: '#styles' },
  panels: { defaults: [] },
  selectorManager: { componentFirst: true },
});

editor.on('canvas:frame:load', (args) => {
  const win = (args && args.window) || args;
  if (!win || !win.document || !win.document.head) return;
  const style = win.document.createElement('style');
  style.textContent = [
    'body{margin:0;font-family:system-ui,sans-serif;background:#fff}',
    'body.blueprint [data-kind]{background:transparent!important;color:transparent!important;border:1px dashed #6ea1ff!important}',
    'body.blueprint [data-kind] *{visibility:hidden}',
    'body.blueprint [data-kind]::after{content:attr(data-kind) " " attr(data-anchor);visibility:visible;position:absolute;left:2px;top:2px;color:#2563eb;font:10px ui-monospace;background:#fff;padding:0 4px}',
    '[data-kind]::before{content:"";position:absolute;left:-3px;top:-3px;width:7px;height:7px;border-radius:50%;background:#2563eb;box-shadow:0 0 0 1px #fff;z-index:6}',
    'body.blueprint::after{content:"";position:absolute;left:50%;top:0;bottom:0;border-left:1px dashed #2563eb;opacity:.35;pointer-events:none}',
    '[data-kind] .anchor-dot{position:absolute;width:7px;height:7px;border-radius:50%;background:#2563eb;border:1px solid #fff;z-index:5}',
    '[data-kind] .cl{position:absolute;border-top:1px dashed #2563eb;opacity:.7}',
  ].join('');
  win.document.head.appendChild(style);
});

editor.setDevice('phone');

function canvasBody(){ const f = editor.Canvas.getFrameEl(); return f && f.contentDocument ? f.contentDocument.body : null; }
function canvasWidth(){ const f = editor.Canvas.getFrameEl(); return f ? f.clientWidth : 390; }
function attr(c, k){ return (c.getAttributes() || {})[k] || ''; }
function compStyle(c){ const el = c.getEl(); return el ? el.ownerDocument.defaultView.getComputedStyle(el) : null; }

function applyAnchors(){
  const frame = editor.Canvas.getFrameEl();
  const width = frame ? frame.clientWidth : 390;
  const height = frame ? frame.clientHeight : 844;
  const walk = (comps, availW, availH) => {
    for (const c of comps) {
      if (!attr(c, 'data-kind')) { walk(c.components(), availW, availH); continue; }
      const a = (attr(c, 'data-anchor') || 'left-top').split('-');
      const cs = compStyle(c); if (!cs) continue;
      const w = parseFloat(cs.width) || 200;
      const h = parseFloat(cs.height) || 40;
      let left = parseFloat(cs.left) || 20;
      let top = parseFloat(cs.top) || 20;
      if (a[0] === 'right') left = availW - w - 20;
      if (a[0] === 'center') left = Math.round((availW - w) / 2);
      if (a[1] === 'bottom') top = availH - h - 20;
      if (a[1] === 'center') top = Math.round((availH - h) / 2);
      c.setStyle({ ...c.getStyle(), position: 'absolute', left: left + 'px', top: top + 'px' });
      walk(c.components(), w, h);
    }
  };
  walk(editor.getWrapper().components(), width, height);
}

const DEF_PROPS = {
  '标题': { '内容': '标题', '级别': 2 },
  '文本': { '内容': '这是一段文本' },
  '按钮': { '文本': '按钮', '动作': 'click', '样式': 'primary' },
  '输入框': { '标签': '标签', '变量': 'input', '占位符': '请输入' },
  '数字框': { '标签': '数量', '变量': 'num', '默认': 0 },
  '选择框': { '标签': '选项', '变量': 'sel', '选项': ['A', 'B'] },
  '复选框': { '标签': '同意', '变量': 'cb', '默认': false },
  '文本域': { '标签': '备注', '变量': 'ta', '行数': 3 },
  '图片': { '链接': '', '宽度': '100%' },
  '分隔线': {},
  '进度条': { '进度': 0.5, '标签': '进度' },
  '链接': { '文本': '链接', '链接': '#' },
  '计算器': { '变量': 'calc', '模式': 'standard' },
};

function setDevice(id){
  editor.setDevice(id);
  const dev = editor.Devices.get(id);
  const w = dev ? (dev.get('width') || '390px') : '390px';
  const frame = editor.Canvas.getFrameEl();
  if (frame) {
    frame.style.width = w;
    frame.style.margin = '0 auto';
    frame.style.background = '#fff';
    frame.style.transition = 'width .2s ease';
  }
  editor.refresh({ tools: true });
  setTimeout(() => { applyAnchors(); say('设备：' + id + '（' + w + '）'); }, 160);
}

let blueprint = false;
function toggleBlueprint(){
  const b = canvasBody();
  if (!b) return;
  blueprint = !blueprint;
  b.classList.toggle('blueprint', blueprint);
  document.getElementById('viewBtn').textContent = blueprint ? '设计' : '蓝图';
  say(blueprint ? '蓝图视图：只显示轮廓与锚点' : '设计视图');
}

function say(msg){ document.getElementById('status').textContent = msg; }

function multiSelect(){
  return [...editor.getWrapper().components()].filter(c => c.getStyle() && (editor.getSelected() === c || c.get('selected')));
}

function selectedOrAll(){
  const sel = editor.getSelected() ? [editor.getSelected()] : [];
  if (sel.length) return sel;
  const deep = (comps) => {
    const kids = [...comps].filter(c => attr(c, 'data-kind'));
    if (kids.length >= 2) return kids;
    for (const c of kids) { const r = deep(c.components()); if (r.length >= 2) return r; }
    return kids;
  };
  const found = deep(editor.getWrapper().components());
  return found.length ? found : [...editor.getWrapper().components()];
}

function alignLeft(){
  const list = selectedOrAll().filter(c => compStyle(c));
  if (list.length < 2) { say('至少选中 1 个元素（未选中则对齐全部）'); }
  const lefts = list.map(c => parseFloat(compStyle(c).left) || 20);
  const min = Math.min(...lefts);
  for (const c of list) c.setStyle({ ...c.getStyle(), left: min + 'px' });
  say('左对齐到 ' + min + 'px');
}

function distributeH(){
  const list = selectedOrAll().filter(c => compStyle(c)).sort((a, b) => (parseFloat(compStyle(a).left) || 0) - (parseFloat(compStyle(b).left) || 0));
  if (list.length < 3) { say('水平分布至少需要 3 个元素'); return; }
  const first = parseFloat(compStyle(list[0]).left) || 20;
  const lastC = list[list.length - 1];
  const last = parseFloat(compStyle(lastC).left) || 20;
  const step = (last - first) / (list.length - 1);
  list.forEach((c, i) => c.setStyle({ ...c.getStyle(), left: Math.round(first + step * i) + 'px' }));
  say('水平分布：' + list.length + ' 个元素');
}

function loadTemplate(){
  const kids = [
    { kind: '标题', x: 16, y: 24, w: 300, h: 48, props: { '内容': '登录 Lilyco' }, anchor: 'center-top' },
    { kind: '文本', x: 16, y: 88, w: 316, h: 40, props: { '内容': '使用邮箱登录' }, anchor: 'center-top' },
    { kind: '输入框', x: 16, y: 144, w: 316, h: 74, props: { '标签': '邮箱', '占位符': 'you@example.com' }, anchor: 'center-top' },
    { kind: '输入框', x: 16, y: 232, w: 316, h: 74, props: { '标签': '密码', '占位符': '至少 8 位' }, anchor: 'center-top' },
    { kind: '按钮', x: 16, y: 326, w: 316, h: 46, props: { '文本': '登录' }, anchor: 'center-top' },
    { kind: '文本', x: 16, y: 388, w: 316, h: 36, props: { '内容': '邮箱或密码错误' }, anchor: 'center-top' },
  ];
  const card = boxHtml('卡片', { '标题': '登录卡片' }, 'center-top', 0, 0, 360, 460,
    kids.map(e => boxHtml(e.kind, e.props, e.anchor, e.x, e.y, e.w, e.h)).join(''));
  editor.setComponents(card);
  setTimeout(() => { applyAnchors(); say('已载入登录页模板（卡片 + 6 元素）'); }, 120);
}

function toDesign(){
  const els = [];
  const walk = (comps, ox, oy) => {
    [...comps].forEach((c) => {
      if (!attr(c, 'data-kind')) { walk(c.components(), ox, oy); return; }
      const cs = compStyle(c);
      if (!cs) return;
      const attrs = c.getAttributes() || {};
      const kind = attrs['data-kind'] || '文本';
      const left = Math.round(ox + (parseFloat(cs.left) || 0));
      const top = Math.round(oy + (parseFloat(cs.top) || 0));
      let props = {};
      try { props = JSON.parse((attrs['data-props'] || '{}').replace(/&quot;/g, '"')); } catch (_) {}
      props = Object.assign({}, DEF_PROPS[kind] || {}, props);
      if (kind !== '卡片') els.push({
        id: attrs['data-id'] || ('el' + els.length),
        type: kind,
        x: left,
        y: top,
        w: Math.round(parseFloat(cs.width) || 200),
        h: Math.round(parseFloat(cs.height) || 40),
        color: null,
        anchor: attrs['data-anchor'] || 'left-top',
        props,
      });
      walk(c.components(), left, top);
    });
  };
  walk(editor.getWrapper().components(), 0, 0);
  return { title: 'Lilyco 原型', size: 'medium', elements: els };
}

async function saveDoc(){
  const r = await fetch('/api/designer/save', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(toDesign()) });
  const d = await r.json();
  say(d.ok ? '已保存到 ' + d.path : '保存失败: ' + JSON.stringify(d));
}

async function loadDoc(){
  const r = await fetch('/api/designer/load');
  if (!r.ok) { say('还没有保存的设计稿'); return; }
  const d = await r.json();
  if (!d.ok) { say('加载失败'); return; }
  applyDesign(d.doc);
}

function applyDesign(doc){
  editor.setComponents((doc.elements || []).map(e => boxHtml(e.type, Object.assign({}, e.props || {}, { id: e.id || '' }), e.anchor, e.x, e.y, e.w, e.h)));
  setTimeout(() => { say('已加载 ' + (doc.elements || []).length + ' 个元素'); }, 120);
}

async function exportHtml(){
  const r = await fetch('/api/designer/export', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(toDesign()) });
  const d = await r.json();
  if (!d.ok) { say('导出失败: ' + JSON.stringify(d)); return; }
  download('prototype.html', d.html, 'text/html');
}

function exportJson(){ download('prototype.design.json', JSON.stringify(toDesign(), null, 2), 'application/json'); }

function download(name, text, type){
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([text], { type }));
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

const preview = document.getElementById('preview');
let previewOn = false;
function togglePreview(){
  previewOn = !previewOn;
  preview.hidden = !previewOn;
  if (previewOn) preview.srcdoc = editor.getHtml() + '<style>' + editor.getCss() + '</style>';
}

window.__lycoDesigner = {
  doc: () => toDesign(),
  device: () => editor.getDevice(),
  setDevice,
  blueprint: () => blueprint,
  toggleBlueprint,
  togglePreview,
  loadTemplate,
  alignLeft,
  distributeH,
  applyDesign,
  compCount: () => editor.getWrapper().components().length,
  blockCount: () => document.querySelectorAll('#blocks .gjs-block').length,
  canvasSize: () => { const f = editor.Canvas.getFrameEl(); return { w: f.clientWidth, h: f.clientHeight }; },
  anchorOf: (i) => attr([...editor.getWrapper().components()][i], 'data-anchor'),
  tree: () => {
    const walk = (comps) => {
      const out = [];
      for (const c of comps) {
        if (attr(c, 'data-kind')) out.push({ kind: attr(c, 'data-kind'), anchor: attr(c, 'data-anchor'), kids: walk(c.components()) });
        else out.push(...walk(c.components()));
      }
      return out;
    };
    return walk(editor.getWrapper().components());
  },
  elementLeft: (kind) => {
    const c = [...editor.getWrapper().components()].find(x => attr(x, 'data-kind') === kind);
    const cs = c && compStyle(c);
    return cs ? Math.round(parseFloat(cs.left) || 0) : null;
  },
  firstStyle: () => { const c = [...editor.getWrapper().components()][0]; const cs = compStyle(c); return cs ? { left: cs.left, top: cs.top, w: cs.width, h: cs.height } : null; },
};

editor.onReady(() => {
  setTimeout(() => {
    try { setDevice('phone'); loadTemplate(); say('就绪 · 已载入登录页模板'); }
    catch (e) { say('模板加载失败: ' + e.message); }
  }, 400);
});
</script>
</body>
</html>"##,
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
                anchor: Some("center-top".into()),
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
        assert!(
            err.contains("btn1") && err.contains("不存在的类型"),
            "{err}"
        );
    }

    #[test]
    fn empty_doc_is_allowed() {
        let spec = DesignDoc::default().to_spec().unwrap();
        assert!(spec.window.elements.is_empty());
    }

    #[test]
    fn anchor_is_optional_and_serialized() {
        let d = doc();
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["elements"][0]["anchor"], "center-top");
    }
}
