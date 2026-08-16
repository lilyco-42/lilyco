//! # Lilyco Ultra UI — Generator
//!
//! 将 UiSpec 转换为完整的 React HTML 页面。
//! 生成的页面使用 CDN React 18 + Babel Standalone，无需构建步骤。

use crate::spec::UiSpec;

/// 将 UiSpec 转换为完整的 React HTML 页面
pub fn generate_react_html(spec: &UiSpec) -> String {
    let spec_json = serde_json::to_string(spec).unwrap();
    let title = html_escape(&spec.window.title);
    format!(
        r###"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{title}</title>
  <script crossorigin src="https://unpkg.com/react@18/umd/react.production.min.js"></script>
  <script crossorigin src="https://unpkg.com/react-dom@18/umd/react-dom.production.min.js"></script>
  <script src="https://unpkg.com/@babel/standalone/babel.min.js"></script>
  <style>{css}</style>
</head>
<body>
  <div id="root"></div>
  <script type="text/babel" data-presets="react">
    const SPEC = {spec_json};
    {react_app}
  </script>
</body>
</html>"###,
        title = title,
        css = CSS,
        spec_json = spec_json,
        react_app = REACT_APP,
    )
}

const CSS: &str = r#"
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#f0f2f5;min-height:100vh;color:#1e293b;display:flex;align-items:flex-start;justify-content:center;padding:40px 20px}
.window{background:#fff;border-radius:16px;box-shadow:0 4px 24px rgba(0,0,0,.08);padding:32px;width:100%}
.window.small{max-width:400px}.window.medium{max-width:640px}.window.large{max-width:960px}.window.fullscreen{max-width:100%;min-height:100vh;border-radius:0}
.wtitle{font-size:24px;font-weight:700;margin-bottom:24px;color:#0f172a}
.el{margin:10px 0}.el-text{color:#475569;font-size:15px;line-height:1.6}
.el-heading{color:#0f172a;font-weight:700}.el-heading.h1{font-size:28px;margin:8px 0 16px}.el-heading.h2{font-size:22px;margin:8px 0 12px}.el-heading.h3{font-size:18px;margin:8px 0 10px}.el-heading.h4{font-size:16px;margin:8px 0 8px}.el-heading.h5{font-size:15px;margin:8px 0 8px}.el-heading.h6{font-size:14px;margin:8px 0 8px;color:#64748b}
.el-btn{display:inline-flex;align-items:center;gap:6px;padding:10px 24px;border:none;border-radius:8px;font-size:15px;font-weight:500;cursor:pointer;transition:all .2s;background:#3b82f6;color:#fff;margin:4px 8px 4px 0}
.el-btn:hover{background:#2563eb;transform:translateY(-1px);box-shadow:0 4px 12px rgba(59,130,246,.3)}
.el-btn.secondary{background:#e2e8f0;color:#475569}.el-btn.secondary:hover{background:#cbd5e1}
.el-btn.danger{background:#ef4444}.el-btn.danger:hover{background:#dc2626}
.el-field{margin:14px 0}.el-label{display:block;font-size:14px;font-weight:500;color:#334155;margin-bottom:6px}
.el-input,.el-select,.el-textarea{width:100%;padding:10px 12px;border:1px solid #cbd5e1;border-radius:8px;font-size:15px;outline:none;transition:border-color .2s;font-family:inherit}
.el-input:focus,.el-select:focus,.el-textarea:focus{border-color:#3b82f6;box-shadow:0 0 0 3px rgba(59,130,246,.1)}
.el-textarea{resize:vertical}.el-checkbox{display:flex;align-items:center;gap:8px;cursor:pointer;padding:8px 0}
.el-checkbox input{width:18px;height:18px;cursor:pointer;accent-color:#3b82f6}
.el-divider{border:none;border-top:1px solid #e2e8f0;margin:20px 0}
.el-image{border-radius:8px;display:block}.el-link{color:#3b82f6;text-decoration:none;font-size:15px}.el-link:hover{text-decoration:underline}
.el-pw{margin:14px 0}.el-pt{width:100%;height:8px;background:#e2e8f0;border-radius:4px;overflow:hidden}.el-pb{height:100%;background:#3b82f6;border-radius:4px;transition:width .3s}
.el-toast{position:fixed;top:24px;right:24px;padding:16px 24px;background:#1e293b;color:#fff;border-radius:12px;box-shadow:0 8px 32px rgba(0,0,0,.2);z-index:9999;max-width:400px;animation:slideIn .3s ease;font-size:14px}
.el-toast .tt{font-weight:600;margin-bottom:4px}.el-toast .td{color:#94a3b8;font-family:monospace;font-size:13px;word-break:break-all}
@keyframes slideIn{from{transform:translateX(120%);opacity:0}to{transform:translateX(0);opacity:1}}
"#;

const REACT_APP: &str = r##"
function App() {
  const [values, setValues] = React.useState({});
  const [toast, setToast] = React.useState(null);
  const timer = React.useRef(null);
  const w = SPEC.窗口;

  const update = (k, v) => setValues(p => ({ ...p, [k]: v }));
  const showToast = (action) => {
    if (timer.current) clearTimeout(timer.current);
    setToast({ action: action || '(未命名)', data: JSON.stringify(values, null, 2), time: new Date().toLocaleTimeString('zh-CN') });
    timer.current = setTimeout(() => setToast(null), 4000);
  };

  const renderEl = (el, i) => {
    switch (el.类型) {
      case '文本': return <p key={i} className="el el-text">{el.内容}</p>;
      case '标题': { const T = 'h' + (el.级别||2); return <T key={i} className={'el el-heading h' + (el.级别||2)}>{el.内容}</T>; }
      case '按钮': return <button key={i} className={'el-btn ' + (el.样式||'')} onClick={() => showToast(el.动作)}>{el.文本}</button>;
      case '输入框': return (
        <div key={i} className="el el-field"><label className="el-label">{el.标签}</label>
        <input className="el-input" value={values[el.变量] ?? el.默认 ?? ''} placeholder={el.占位符||''} onChange={e => update(el.变量, e.target.value)} /></div>
      );
      case '数字框': return (
        <div key={i} className="el el-field"><label className="el-label">{el.标签}</label>
        <input type="number" className="el-input" value={values[el.变量] ?? el.默认 ?? ''} min={el.最小??''} max={el.最大??''} onChange={e => update(el.变量, Number(e.target.value))} /></div>
      );
      case '选择框': return (
        <div key={i} className="el el-field"><label className="el-label">{el.标签}</label>
        <select className="el-select" value={values[el.变量] ?? el.默认 ?? ''} onChange={e => update(el.变量, e.target.value)}>
        {(el.选项||[]).map((o,j) => <option key={j} value={o}>{o}</option>)}</select></div>
      );
      case '复选框': return (
        <label key={i} className="el el-checkbox"><input type="checkbox" checked={values[el.变量] ?? el.默认 ?? false}
        onChange={e => update(el.变量, e.target.checked)} /><span>{el.标签}</span></label>
      );
      case '文本域': return (
        <div key={i} className="el el-field"><label className="el-label">{el.标签}</label>
        <textarea className="el-textarea" rows={el.行数||4} value={values[el.变量] ?? el.默认 ?? ''} onChange={e => update(el.变量, e.target.value)} /></div>
      );
      case '图片': return <img key={i} className="el el-image" src={el.链接} style={{width:el.宽度||'100%'}} />;
      case '分隔线': return <hr key={i} className="el el-divider" />;
      case '进度条': { const pct = Math.round((el.进度||0)*100); return (
        <div key={i} className="el el-pw">{el.标签 && <label className="el-label">{el.标签} — {pct}%</label>}
        <div className="el-pt"><div className="el-pb" style={{width:pct+'%'}} /></div></div>); }
      case '链接': return <a key={i} className="el el-link" href={el.链接} target="_blank" rel="noopener noreferrer">{el.文本}</a>;
      default: return <div key={i} className="el" style={{color:'#ef4444'}}>未知元素类型: {el.类型}</div>;
    }
  };

  return (<>
    <div className={'window ' + w.大小}>
      <h1 className="wtitle">{w.标题}</h1>
      {(w.元素||[]).map(renderEl)}
    </div>
    {toast && <div className="el-toast"><div className="tt">动作 "{toast.action}" 已触发</div><div className="td">{toast.data}</div><div style={{marginTop:4,color:'#64748b'}}>时间: {toast.time}</div></div>}
  </>);
}
ReactDOM.createRoot(document.getElementById('root')).render(<App />);
"##;

/// HTML 转义
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
