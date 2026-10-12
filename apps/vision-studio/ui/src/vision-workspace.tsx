import { useState } from 'react';
import { Download, Moon, Sun, ImagePlus, Trash2, LoaderCircle, CircleCheck, ShieldCheck } from 'lucide-react';
import { Button } from './components/ui/button';
import { Badge } from './components/ui/badge';
import { Card, CardContent, CardHeader, CardTitle } from './components/ui/card';
import { Separator } from './components/ui/separator';
import { visionTools, visionFileUrl, type VisionParams, type VisionTool } from './vision-tools';
import { useVisionWorkspace } from './use-vision-workspace';
import { VisionCropPreview } from './vision-crop-preview';

const fieldClass = 'mt-1 w-full min-w-0 rounded-md border bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-ring';
const labels: Record<string, string> = { region: '区域 X1,Y1,X2,Y2', scale: '放大倍率（SVG 填 0 自动）', width: '宽度 px', height: '高度 px', top: '结果数量', tolerance: '颜色聚类容差', grid: '对比网格', exclude_tol: '指定背景色容差', exclude_color: '排除颜色（可选 #ffffff）', candidates: '候选色（可选，逗号分隔）' };

export function VisionWorkspace() {
  const workspace = useVisionWorkspace();
  const [tool, setTool] = useState<VisionTool>('crop');
  const [params, setParams] = useState<VisionParams>({...visionTools[0].defaults});
  const [dark, setDark] = useState(() => localStorage.getItem('vision-theme') === 'dark');
  const selected = visionTools.find(x => x.id === tool)!;
  const job = workspace.job;
  const complete = job?.state === 'done';
  const palette = (job?.result?.colors ?? []) as {hex: string; percent: number}[];
  function switchTool(id: VisionTool) {
    setTool(id); setParams({...visionTools.find(x => x.id === id)!.defaults}); workspace.resetResult();
  }
  function theme() { const next = !dark; setDark(next); localStorage.setItem('vision-theme', next ? 'dark' : 'light'); }
  const html = tool === 'html-screenshot';
  return <div className={`${dark ? 'dark ' : ''}min-h-screen bg-background text-foreground`}>
    <a href="#workspace" className="sr-only focus:not-sr-only focus:p-3">跳到工作区</a>
    <header className="flex flex-wrap items-center justify-between gap-3 border-b px-4 py-4 md:px-8">
      <div className="flex items-center gap-3"><span className="rounded-lg bg-primary p-2 text-primary-foreground"><ImagePlus size={22}/></span><div><h1 className="text-lg font-semibold tracking-tight">图片工坊</h1><p className="text-xs text-muted-foreground">Lain42 · Vision Studio</p></div></div>
      <div className="flex items-center gap-3"><Badge variant="secondary"><ShieldCheck/>本机处理</Badge><Button aria-label="切换主题" variant="ghost" size="icon" onClick={theme}>{dark ? <Sun/> : <Moon/>}</Button></div>
    </header>
    <main id="workspace" className="mx-auto grid max-w-[1600px] gap-5 p-4 lg:grid-cols-[220px_minmax(0,1fr)_300px] lg:p-6">
      <aside className="min-w-0"><p className="mb-3 text-xs font-medium uppercase tracking-widest text-muted-foreground">选择工具</p><nav aria-label="图像工具" className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-1">{visionTools.map(item => <Button key={item.id} variant={item.id === tool ? 'secondary' : 'ghost'} aria-pressed={item.id === tool} className="justify-start text-xs sm:text-sm" disabled={workspace.busy} onClick={() => switchTool(item.id)}><item.icon/>{item.title}</Button>)}</nav><p className="mt-5 text-xs leading-relaxed text-muted-foreground">不上传图片，不需要订阅。支持 PNG、JPEG、WebP。单文件 16 MB，图片不超过 1600 万像素。</p></aside>
      <section className="min-w-0 space-y-4">
        <div><Badge variant="outline">{String(visionTools.findIndex(x => x.id === tool) + 1).padStart(2, '0')} / 08</Badge><h2 className="mt-2 text-2xl font-semibold tracking-tight">{selected.title}</h2><p className="mt-1 text-sm text-muted-foreground">{selected.description}</p></div>
        <Card size="sm"><CardHeader className="vision-source-header"><CardTitle className="text-base">原始文件</CardTitle><Button variant="ghost" size="sm" disabled={workspace.pending || !workspace.original} onClick={workspace.clear}><Trash2/>清空</Button></CardHeader><CardContent className="space-y-3">
          <label htmlFor="original" className={`block cursor-pointer rounded-lg border border-dashed focus-within:ring-2 focus-within:ring-ring px-4 py-3 text-sm ${workspace.busy ? 'opacity-50' : 'hover:bg-muted'}`}><ImagePlus className="mb-2"/><strong className="block break-all">{workspace.original?.name ?? (html ? '选择本地 HTML' : '选择图片')}</strong><span className="text-xs text-muted-foreground">{workspace.original ? `${(workspace.original.size / 1024).toFixed(1)} KB · 可清空后替换` : '点击选择，不会改动原文件'}</span><input id="original" className="sr-only" type="file" disabled={workspace.busy} accept={html ? '.html,.htm' : 'image/png,image/jpeg,image/webp'} onChange={e => {void workspace.select(e.target.files?.[0] ?? null); e.target.value = '';}}/></label>
          {tool === 'pixel-diff' && <label htmlFor="rebuilt" className="block cursor-pointer rounded-lg border border-dashed focus-within:ring-2 focus-within:ring-ring p-3 text-sm"><strong className="block break-all">{workspace.rebuilt?.name ?? '选择第二张图片'}</strong><input id="rebuilt" className="sr-only" type="file" disabled={workspace.busy || !workspace.original} accept="image/png,image/jpeg,image/webp" onChange={e => {void workspace.select(e.target.files?.[0] ?? null, true); e.target.value = '';}}/></label>}
          {workspace.preview && (tool === 'crop' ? <VisionCropPreview key={workspace.preview} src={workspace.preview} region={String(params.region)} disabled={workspace.busy} onRegion={region => setParams({...params, region})}/> : <div className="studio-checker flex min-h-40 items-center justify-center rounded-lg border p-3"><img id="originalPreview" src={workspace.preview} alt="原图预览" className="max-h-64 max-w-full object-contain"/></div>)}
          {html && <p className="text-xs text-muted-foreground">只打开你信任的 HTML。它的脚本与外部资源会由独立浏览器执行；不会使用个人浏览器的登录状态，也不在此界面执行。</p>}
        </CardContent></Card>
        <Card id="gallery" size="sm"><CardHeader><CardTitle className="text-base">成果预览</CardTitle></CardHeader><CardContent>
          {complete ? <div className="space-y-4">
            {tool === 'image-info' && job.result && <dl className="grid grid-cols-2 gap-3 text-sm">{[['尺寸', `${job.result.width} × ${job.result.height} px`], ['格式', String(job.result.format).toUpperCase()], ['文件大小', `${(Number(job.result.file_size_bytes) / 1024).toFixed(1)} KB`]].map(([label, value]) => <div key={label} className="rounded-lg border p-3"><dt className="text-xs text-muted-foreground">{label}</dt><dd className="mt-1 font-medium">{value}</dd></div>)}</dl>}
            {job.files.filter(file => /\.(png|svg)$/.test(file.name)).map(file => <figure className="studio-checker rounded-lg border p-3" key={file.name}><img alt={'处理结果 ' + file.name} src={visionFileUrl(job, file.name)} className="mx-auto max-h-80 max-w-full object-contain"/><figcaption className="mt-2 text-xs text-muted-foreground">{file.name}</figcaption></figure>)}
            {palette.length > 0 && <div aria-label="提取的主色" className="grid grid-cols-2 gap-2 sm:grid-cols-3">{palette.map((color, i) => <div className="flex items-center gap-2 rounded-lg border p-2" key={i}><svg width="28" height="28" aria-hidden="true"><rect width="28" height="28" rx="6" fill={/^#[0-9a-f]{6}$/i.test(color.hex) ? color.hex : 'transparent'}/></svg><span className="text-xs"><strong>{color.hex}</strong><br/>{Number(color.percent).toFixed(1)}%</span></div>)}</div>}
            {job.result && <details className="min-w-0"><summary className="cursor-pointer text-sm">查看处理报告</summary><pre id="report" className="mt-2 max-h-60 overflow-auto rounded-md bg-muted p-3 text-xs">{JSON.stringify(job.result, null, 2)}</pre></details>}
          </div> : <div className="flex min-h-36 flex-col items-center justify-center gap-2 text-center text-sm text-muted-foreground"><selected.icon className="size-8 opacity-50"/><p>选择文件，处理后在这里查看真实结果。</p></div>}
        </CardContent></Card>
      </section>
      <aside className="min-w-0"><Card><CardHeader><CardTitle className="text-base">处理参数</CardTitle></CardHeader><CardContent className="space-y-4">
        {Object.keys(params).map(key => typeof params[key] === 'boolean' ? <label key={key} className="flex items-center gap-2 text-sm"><input type="checkbox" checked={Boolean(params[key])} disabled={workspace.busy} onChange={e => setParams({...params, [key]: e.target.checked})}/>{key === 'color' ? '保留颜色' : '使用多边形路径'}</label> : key === 'mode' ? <label key={key} className="block text-sm">背景提取方式<select className={fieldClass} value={String(params[key])} disabled={workspace.busy} onChange={e => setParams({...params, [key]: e.target.value})}><option value="color">按边缘颜色</option><option value="dark">去除黑色背景</option></select></label> : <label key={key} className="block text-sm">{labels[key] ?? key}<input className={fieldClass} id={`param-${key}`} type={typeof params[key] === 'number' ? 'number' : 'text'} value={String(params[key])} disabled={workspace.busy} onChange={e => setParams({...params, [key]: typeof params[key] === 'number' ? Number(e.target.value) : e.target.value})}/></label>)}
        {Object.keys(params).length === 0 && <p className="text-sm text-muted-foreground">无需参数，直接读取图片信息。</p>}
        {tool === 'crop' && <p className="text-xs text-muted-foreground">区域使用原图像素坐标：左上角与右下角。超出图片的边界会由引擎收紧。</p>}
        {tool === 'extract-foreground' && <p className="text-xs text-muted-foreground">填写排除颜色时使用指定容差；不填则由边缘估计背景。黑色模式使用引擎固定的亮度阈值。</p>}
        {html && workspace.capabilities && !workspace.capabilities.html_browser && <p role="alert" className="text-sm text-destructive">需要先安装 Chrome 或 Edge，其他工具可以直接使用。</p>}
        <Separator/>
        <Button id="render" className="w-full" disabled={workspace.busy || !workspace.original || !workspace.capabilities?.engine || (html && !workspace.capabilities?.html_browser) || (tool === 'pixel-diff' && !workspace.rebuilt)} onClick={() => workspace.run(tool, params)}>{workspace.busy ? <LoaderCircle className="animate-spin"/> : <selected.icon/>}开始处理</Button>
        {job && ['queued', 'running'].includes(job.state) && <Button variant="outline" className="w-full" onClick={workspace.cancel}>取消处理</Button>}
        <div id="status" aria-live="polite" className="break-words text-sm">{workspace.error || job?.error ? <p role="alert" className="text-destructive">{workspace.error || job?.error}</p> : complete ? <p className="flex items-center gap-2 text-primary"><CircleCheck/>处理完成</p> : job?.state === 'cancelled' ? '已取消，可修改参数后重新处理' : workspace.busy ? '正在本机处理…' : '等待开始'}</div>
        {workspace.capabilities && !workspace.capabilities.engine && <p role="alert" className="text-sm text-destructive">引擎未安装，请下载完整桌面包。</p>}
        {complete && <div className="space-y-2"><Button id="downloadZip" render={<a href={visionFileUrl(job, 'export.zip')} download/>} className="w-full"><Download/>下载全部成果</Button>{job.files.map(file => <a key={file.name} href={visionFileUrl(job, file.name)} download className="block break-all text-sm text-primary underline-offset-4 hover:underline">{file.name} · {(file.size / 1024).toFixed(1)} KB</a>)}</div>}
      </CardContent></Card><p className="mt-4 text-xs leading-relaxed text-muted-foreground">文件只保留在这次应用会话中。关闭或清空后会移除，请先下载成果。矢量化和背景提取的质量取决于输入图片。</p></aside>
    </main>
  </div>;
}
