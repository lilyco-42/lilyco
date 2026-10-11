'use strict';
const $ = id => document.getElementById(id);
let tool = 'aic', workspace = null, activeJob = null, capabilities = {}, poll = 0, uploading = false;
const extensions = /\.(pxls|dat|bytes|png|atlas|json|skel|ress)$/i;
async function api(path, options = {}) {
  const response = await fetch('/api/' + path, options);
  const data = await response.json();
  if (!response.ok) throw new Error(typeof data.detail === 'string' ? data.detail : '请求参数无效，请重新选择文件');
  return data;
}
function status(text) { $('status').textContent = text; }
function busy(value) {
  for (const id of ['render', 'inspect', 'sprites', 'textures', 'delete']) $(id).disabled = value || !workspace;
  for (const id of ['files', 'folder', 'aicTab', 'spineTab']) $(id).disabled = value;
  if (!$('entry').value || !capabilities[tool]?.available) for (const id of ['render', 'inspect', 'sprites', 'textures']) $(id).disabled = true;
  $('sprites').hidden = $('textures').hidden = tool !== 'aic';
  if (tool === 'spine' && !capabilities.spine?.available) $('render').disabled = $('inspect').disabled = true;
  $('cancel').hidden = !activeJob;
}
function selectTool(next) {
  tool = next;
  $('aicTab').classList.toggle('active', tool === 'aic');
  $('spineTab').classList.toggle('active', tool === 'spine');
  $('toolTitle').textContent = tool === 'aic' ? 'AIC 角色预览与导出' : 'Spine 动画播放与导出';
  $('toolHelp').textContent = tool === 'aic' ? '选择 .pxls 角色表和同目录的配套贴图。也可以选择包含它们的文件夹。' : '选择 .json / .skel、同名 .atlas 和配套 PNG。首版播放适配 Spine 3.8 引擎。';
  $('engineNotice').textContent = tool === 'spine' && !capabilities.spine?.available ? '当前安装不包含官方 Spine 运行时。请下载带 Spine 3.8 播放器的素材工坊完整包。' : tool === 'spine' ? 'Spine 3.8 播放器已就绪。先读取动画列表，再选择动作预览。' : '';
  $('entry').replaceChildren();
  for (const name of workspace?.entries[tool] || []) $('entry').add(new Option(name, name));
  $('entry').disabled = !$('entry').options.length;
  $('motion').replaceChildren(new Option(tool === 'aic' ? '全部动作' : '先读取动画列表', '*'));
  busy(false);
  if (!$('entry').options.length) for (const id of ['render', 'inspect', 'sprites', 'textures']) $(id).disabled = true;
}
async function upload(list) {
  if (uploading || activeJob) return;
  const files = [...list].filter(f => extensions.test(f.name));
  if (!files.length) return status('没有找到角色文件或配套贴图');
  if (files.length > 1000 || files.some(f => f.size > 128 * 1024 * 1024) || files.reduce((n, f) => n + f.size, 0) > 512 * 1024 * 1024) return status('请选择不超过 1000 个文件，单文件最多 128 MB，总计最多 512 MB');
  uploading = true; busy(true); status('正在加载到本机工作区…');
  const form = new FormData();
  const paths = files.map(f => f.webkitRelativePath || f.name);
  for (const file of files) form.append('files', file, file.name);
  form.append('paths', JSON.stringify(paths));
  try {
    const previous = workspace;
    workspace = await api('workspaces', {method: 'POST', body: form});
    if (previous) await api('workspaces/' + previous.id, {method: 'DELETE'});
    $('selection').textContent = `${workspace.count} 个文件 · ${(workspace.bytes / 1024 / 1024).toFixed(1)} MB`;
    $('gallery').replaceChildren(); $('outputs').replaceChildren(); $('empty').hidden = false;
    $('download').hidden = $('report').hidden = true;
    selectTool(workspace.entries.aic.length ? 'aic' : 'spine');
    status('文件已加载。先读取动画列表，再选择动作预览。');
  } catch (error) { status(error.message); }
  finally { uploading = false; busy(false); $('files').value = $('folder').value = ''; }
}
function showResult(job) {
  const result = job.result || {};
  const motions = tool === 'aic' ? result.files?.flatMap(f => f.poses || []) : result.motions;
  if (motions?.length) {
    $('motion').replaceChildren();
    if (tool === 'aic') $('motion').add(new Option('全部动作', '*'));
    for (const motion of motions) $('motion').add(new Option(motion.title, motion.title));
    if (tool === 'aic') $('motion').selectedIndex = Math.min(1, $('motion').options.length - 1);
  }
  $('gallery').replaceChildren(); $('outputs').replaceChildren();
  const images = job.files.filter(f => /\.(png|gif)$/i.test(f.name));
  // Large exports stay downloadable; don't create thousands of image nodes.
  const previews = [...images.filter(f => /\.gif$/i.test(f.name)), ...images.filter(f => !/\.gif$/i.test(f.name))].slice(0, 24);
  for (const file of previews) {
    const figure = document.createElement('figure'); figure.className = 'preview';
    const img = document.createElement('img'); img.src = fileUrl(job.id, file.name); img.alt = file.name; img.loading = 'lazy';
    const caption = document.createElement('figcaption'); caption.textContent = file.name;
    figure.append(img, caption); $('gallery').append(figure);
  }
  for (const file of job.files.slice(0, 100)) {
    const a = document.createElement('a'); a.href = fileUrl(job.id, file.name); a.download = file.name.split('/').pop();
    a.textContent = `${file.name} · ${(file.size / 1024).toFixed(1)} KB`; $('outputs').append(a);
  }
  $('download').href = fileUrl(job.id, 'export.zip'); $('download').download = 'lain42-assets.zip'; $('download').hidden = false;
  $('reportText').textContent = JSON.stringify(result, null, 2); $('report').hidden = false;
  $('empty').hidden = images.length > 0;
  status(motions?.length ? `已读取 ${motions.length} 个动作，选择一个即可预览。` : `处理完成 · ${job.files.length} 个结果文件${images.length > 24 ? '，预览显示前 24 张' : ''}`);
}
function fileUrl(id, name) { return `/api/jobs/${id}/files/` + name.split('/').map(encodeURIComponent).join('/'); }
async function run(action) {
  if (!workspace || activeJob || uploading) return;
  if (!$('entry').value) return status('这组素材中没有当前工具可读取的角色文件');
  $('download').hidden = true;
  try {
    const job = await api('jobs', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({session: workspace.id, tool, action, entry: $('entry').value, motion: $('motion').value, direction: Number($('direction').value), scale: Number($('scale').value)})});
    activeJob = job.id; busy(true); status('引擎正在处理，文件保留在本机…');
    const check = async () => {
      try {
        const current = await api('jobs/' + job.id);
        if (['done', 'failed', 'cancelled'].includes(current.state)) {
          activeJob = null; busy(false);
          if (current.state === 'done') showResult(current);
          else status(current.error || '已取消处理。可以重新选择动作。');
        } else poll = window.setTimeout(check, 600);
      } catch (error) { activeJob = null; busy(false); status(error.message + '。可删除工作区后重新选择文件。'); }
    };
    await check();
  } catch (error) { activeJob = null; busy(false); status(error.message); }
}
$('files').addEventListener('change', e => upload(e.target.files));
$('folder').addEventListener('change', e => upload(e.target.files));
$('dropzone').addEventListener('dragover', e => {e.preventDefault(); $('dropzone').classList.add('drag');});
$('dropzone').addEventListener('dragleave', () => $('dropzone').classList.remove('drag'));
$('dropzone').addEventListener('drop', e => {e.preventDefault(); $('dropzone').classList.remove('drag'); upload(e.dataTransfer.files);});
$('aicTab').onclick = () => selectTool('aic'); $('spineTab').onclick = () => selectTool('spine');
for (const action of ['inspect', 'render', 'sprites', 'textures']) $(action).onclick = () => run(action);
$('cancel').onclick = async () => {try {if (activeJob) await api('jobs/' + activeJob + '/cancel', {method: 'POST'});} catch (error) {status(error.message);}};
$('delete').onclick = async () => {
  if (!workspace) return;
  try {
    await api('workspaces/' + workspace.id, {method: 'DELETE'}); clearTimeout(poll);
    workspace = activeJob = null; $('selection').textContent = '当前文件和结果已删除';
    $('gallery').replaceChildren(); $('outputs').replaceChildren(); $('empty').hidden = false;
    $('download').hidden = $('report').hidden = true; selectTool(tool); status('工作区已清理，可以选择新素材。');
  } catch (error) {status(error.message);}
};
api('capabilities').then(data => {capabilities = data.tools; selectTool(tool); if (!capabilities.aic?.available) $('engineNotice').textContent = 'AIC 引擎缺失，请下载包含引擎的桌面完整包。';}).catch(error => status(error.message));
