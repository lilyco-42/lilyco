//! Builder 页面 — 可视化 JSON 编辑器 + 实时预览

use crate::spec;

pub fn builder_html() -> String {
    let example_raw = spec::default_example_json();
    let example = html_escape(&example_raw);
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Lilyco Ultra UI — Excel 风格 JSON → React</title>
  <style>{css}</style>
</head>
<body>
  <div class="header">
    <h1>Lilyco Ultra UI<span class="badge">Excel → React</span></h1>
    <div class="toolbar">
      <button class="btn ghost" onclick="loadExample()">载入示例</button>
      <button class="btn ghost" onclick="exportHtml()">导出 HTML</button>
      <button class="btn primary" onclick="render()">渲染预览</button>
    </div>
  </div>
  <div class="main">
    <div class="editor-panel">
      <div class="panel-h">JSON 编辑器 <span class="hint">Ctrl+Enter 渲染</span></div>
      <div class="editor-area"><textarea id="editor" spellcheck="false">{example}</textarea></div>
      <div id="errorBar" class="error-bar"></div>
    </div>
    <div class="preview-panel">
      <div class="panel-h">React 预览 <span id="statusText" class="hint">就绪</span></div>
      <iframe id="preview" class="preview-frame" sandbox="allow-scripts allow-same-origin allow-popups"></iframe>
    </div>
  </div>
  <script>{js}</script>
</body>
</html>"#,
        css = BUILDER_CSS, example = example, js = BUILDER_JS,
    )
}

const BUILDER_CSS: &str = r#"
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#0f172a;color:#e2e8f0;height:100vh;overflow:hidden}
.header{background:#1e293b;padding:12px 24px;display:flex;align-items:center;justify-content:space-between;border-bottom:1px solid #334155}
.header h1{font-size:18px;font-weight:600}.badge{background:#3b82f6;color:#fff;padding:2px 10px;border-radius:12px;font-size:12px;margin-left:8px}
.main{display:flex;height:calc(100vh - 50px)}
.editor-panel{width:45%;display:flex;flex-direction:column;border-right:1px solid #334155}
.preview-panel{width:55%;display:flex;flex-direction:column;background:#f0f2f5}
.panel-h{padding:10px 16px;background:#1e293b;font-size:13px;font-weight:600;color:#94a3b8;display:flex;align-items:center;justify-content:space-between;border-bottom:1px solid #334155}
.hint{color:#64748b;font-weight:400;font-size:12px}
.editor-area{flex:1;padding:16px;overflow:hidden}
textarea{width:100%;height:100%;background:#1e293b;color:#a5f3fc;border:1px solid #334155;border-radius:8px;padding:16px;font-family:'Cascadia Code',Consolas,monospace;font-size:14px;line-height:1.6;resize:none;outline:none}
textarea:focus{border-color:#3b82f6}
.preview-frame{flex:1;border:none;background:#fff}
.toolbar{display:flex;gap:8px}
.btn{padding:6px 16px;border:none;border-radius:6px;font-size:13px;cursor:pointer;font-weight:500;transition:all .2s}
.btn.primary{background:#3b82f6;color:#fff}.btn.primary:hover{background:#2563eb}
.btn.ghost{background:transparent;color:#94a3b8;border:1px solid #475569}.btn.ghost:hover{color:#e2e8f0;border-color:#64748b}
.error-bar{padding:8px 16px;background:#7f1d1d;color:#fecaca;font-size:13px;display:none}
.error-bar.show{display:block}
"#;

const BUILDER_JS: &str = r##"
const editor = document.getElementById('editor');
const preview = document.getElementById('preview');
const errorBar = document.getElementById('errorBar');
const statusText = document.getElementById('statusText');

async function render() {
  const json = editor.value.trim();
  if (!json) { showError('请输入 JSON'); return; }
  let parsed;
  try { parsed = JSON.parse(json); } catch(e) { showError('JSON 语法错误: ' + e.message); return; }
  statusText.textContent = '渲染中...';
  errorBar.classList.remove('show');
  try {
    const res = await fetch('/api/render', {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(parsed)});
    if (!res.ok) { const err = await res.json(); showError(err.error||'渲染失败'); statusText.textContent='错误'; return; }
    const html = await res.text();
    preview.srcdoc = html;
    statusText.textContent = '已渲染';
  } catch(e) { showError('请求失败: ' + e.message); statusText.textContent='错误'; }
}

function showError(msg) { errorBar.textContent = msg; errorBar.classList.add('show'); }

async function loadExample() {
  const res = await fetch('/api/example');
  const data = await res.json();
  editor.value = JSON.stringify(data, null, 2);
  render();
}

async function exportHtml() {
  const json = editor.value.trim();
  if (!json) { showError('请先输入 JSON'); return; }
  let parsed; try { parsed = JSON.parse(json); } catch(e) { showError('JSON 语法错误: ' + e.message); return; }
  const res = await fetch('/api/render', {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(parsed)});
  if (!res.ok) { const err = await res.json(); showError(err.error); return; }
  const html = await res.text();
  const blob = new Blob([html], {type:'text/html'});
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = 'lilyco-ui.html';
  a.click();
  URL.revokeObjectURL(a.href);
}

editor.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') { e.preventDefault(); render(); }
});
render();
"##;

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
