import { useState } from 'react';
import { Crop, Download, Film, LoaderCircle, Moon, RefreshCw, Scissors, ShieldCheck, SlidersHorizontal, Sun, Trash2 } from 'lucide-react';
import { Badge } from './components/ui/badge';
import { Button } from './components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from './components/ui/card';
import { Separator } from './components/ui/separator';
import { mediaFileUrl, mediaSize, mediaTime, type MediaParams, type MediaTool } from './media-api';
import { useMediaWorkspace } from './use-media-workspace';

const tools = [
  {id:'compress', title:'压缩视频', description:'把视频变小一些，分享和交付更轻松。', icon:SlidersHorizontal, defaults:{format:'mp4', crf:28}},
  {id:'convert', title:'转换格式', description:'转换为常用的 MP4 或适合网页的 WebM。', icon:RefreshCw, defaults:{format:'mp4'}},
  {id:'resize', title:'调整尺寸', description:'只填一边就能等比缩放，也可以指定宽高。', icon:Crop, defaults:{format:'mp4', width:1280}},
  {id:'trim', title:'截取片段', description:'填写开始位置和时长，保留你需要的那一段。', icon:Scissors, defaults:{format:'mp4', start:0, duration:5}},
] as const;
const field = 'mt-2 w-full min-w-0 rounded-lg border bg-background px-3 py-2.5 text-sm outline-none focus:ring-2 focus:ring-ring';
const download = 'inline-flex min-h-11 items-center justify-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring';

export function MediaWorkspace() {
  const workspace = useMediaWorkspace();
  const [tool, setTool] = useState<MediaTool>('compress');
  const [params, setParams] = useState<MediaParams>({...tools[0].defaults});
  const [dark, setDark] = useState(() => localStorage.getItem('media-theme') === 'dark');
  const [previewFailed, setPreviewFailed] = useState(false);
  const selected = tools.find(item => item.id === tool)!;
  const job = workspace.job;
  const complete = job?.state === 'done';
  const output = complete ? job.files.find(file => /\.(mp4|webm)$/.test(file.name)) : undefined;
  const input = workspace.source;
  const progress = job?.progress;
  const percentage = progress && typeof progress.progress === 'number' && progress.total
    ? Math.min(100, Math.max(0, Math.round(progress.progress / progress.total * 100))) : null;
  function changeTool(id: MediaTool) {
    setTool(id); setParams({...tools.find(item => item.id === id)!.defaults}); workspace.resetResult();
  }
  function parameter(name: string, value: string) {
    const next = {...params};
    if (value === '') delete next[name]; else next[name] = Number(value);
    setParams(next); workspace.resetResult();
  }
  function theme() {
    const next = !dark; setDark(next); localStorage.setItem('media-theme', next ? 'dark' : 'light');
  }
  return <div className={`${dark ? 'dark ' : ''}min-h-screen bg-background text-foreground`}>
    <a href="#workspace" className="sr-only focus:not-sr-only focus:p-3">跳到工作区</a>
    <header className="flex flex-wrap items-center justify-between gap-3 border-b px-4 py-4 md:px-8">
      <div className="flex min-w-0 items-center gap-3"><img src="/assets/app-icon.png" alt="" width="44" height="44"/><div><h1 className="text-lg font-semibold tracking-tight">视频工坊</h1><p className="text-xs text-muted-foreground">Lain42 · Media Studio</p></div></div>
      <div className="flex items-center gap-2"><Badge variant="secondary"><ShieldCheck/>本机处理</Badge><Button aria-label="切换主题" variant="ghost" size="icon" onClick={theme}>{dark ? <Sun/> : <Moon/>}</Button></div>
    </header>
    <main id="workspace" className="mx-auto grid max-w-[1500px] gap-5 p-4 lg:grid-cols-[190px_minmax(0,1fr)_300px] lg:p-6">
      <aside className="min-w-0"><p className="mb-3 text-xs font-medium tracking-widest text-muted-foreground">从一个目标开始</p>
        <nav aria-label="视频工具" className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-1">{tools.map(item => <Button key={item.id} variant={tool === item.id ? 'secondary' : 'ghost'} aria-pressed={tool === item.id} className="justify-start" disabled={workspace.busy} onClick={() => changeTool(item.id)}><item.icon/>{item.title}</Button>)}</nav>
        <p className="mt-5 text-xs leading-relaxed text-muted-foreground">不上传云端，不改动原文件。支持 MP4、MOV、M4V、WebM、MKV；单个文件最多 256 MB。</p>
      </aside>
      <section className="min-w-0 space-y-4">
        <div><Badge variant="outline">视频处理 · {String(tools.findIndex(item => item.id === tool) + 1).padStart(2, '0')} / 04</Badge><h2 className="mt-2 text-2xl font-semibold tracking-tight">{selected.title}</h2><p className="mt-1 text-sm leading-relaxed text-muted-foreground">{selected.description}</p></div>
        <Card size="sm"><CardHeader className="media-card-header"><CardTitle className="text-base">原始视频</CardTitle><Button variant="ghost" size="sm" disabled={workspace.pending || !input} onClick={() => void workspace.clear()}><Trash2/>移除文件</Button></CardHeader><CardContent className="space-y-3">
          <label htmlFor="media-file" className="block cursor-pointer rounded-lg border border-dashed p-4 text-sm focus-within:ring-2 focus-within:ring-ring hover:bg-muted"><Film className="mb-2"/><strong className="block break-all">{input?.name ?? '选择一个视频'}</strong><span className="mt-1 block text-xs leading-relaxed text-muted-foreground">{workspace.pending && !input ? '正在读取视频信息…' : '点击选择 · 文件只会复制到本机临时工作区'}</span><input id="media-file" className="sr-only" type="file" accept=".mp4,.mov,.m4v,.webm,.mkv" disabled={workspace.busy} onChange={event => {setPreviewFailed(false); void workspace.select(event.target.files?.[0] ?? null); event.target.value = '';}}/></label>
          {input && <><video key={input.id} data-testid="source-preview" className="media-video" src={input.preview} controls preload="metadata" onError={() => setPreviewFailed(true)}/><dl className="grid grid-cols-3 gap-2 text-xs"><div><dt className="text-muted-foreground">尺寸</dt><dd className="mt-1">{input.media.width} × {input.media.height}</dd></div><div><dt className="text-muted-foreground">时长</dt><dd className="mt-1">{mediaTime(input.media.duration)}</dd></div><div><dt className="text-muted-foreground">文件大小</dt><dd className="mt-1">{mediaSize(input.media.bytes)}</dd></div></dl></>}
          {previewFailed && <p className="text-xs leading-relaxed text-muted-foreground">当前浏览器不能播放原格式，你仍可转换为 MP4 后预览。</p>}
        </CardContent></Card>
        <Card id="results" size="sm"><CardHeader><CardTitle className="text-base">成果预览</CardTitle></CardHeader><CardContent className="space-y-4">
          {complete && output ? <><video key={job.id} data-testid="output-preview" className="media-video" src={mediaFileUrl(job, output.name)} controls preload="metadata"/><div className="flex flex-wrap items-center justify-between gap-3 text-sm"><span>{mediaSize(output.size)}{input && ` · 原视频 ${mediaSize(input.media.bytes)}`}</span><Badge variant="secondary">已完成</Badge></div><div className="flex flex-wrap gap-2"><a download href={mediaFileUrl(job, output.name)} className={download}><Download size={16}/>下载视频</a><a download href={mediaFileUrl(job, 'export.zip')} className={download}>视频与处理记录</a></div><details className="rounded-lg border p-3"><summary className="cursor-pointer text-sm">查看原始引擎结果</summary><pre className="mt-3 max-h-64 overflow-auto whitespace-pre-wrap break-all text-xs">{JSON.stringify(job.result, null, 2)}</pre></details></> :
            <div className="flex min-h-36 flex-col items-center justify-center gap-2 rounded-lg border border-dashed p-5 text-center text-sm text-muted-foreground"><Film size={28}/><p>{workspace.busy ? '正在处理，完成后可以预览和下载。' : '选择视频，调整参数，再生成一份新的成果。'}</p></div>}
          <p className="text-xs leading-relaxed text-muted-foreground">处理文件临时保存在本机，关闭应用后清理。请先下载需要保留的成果。</p>
        </CardContent></Card>
      </section>
      <aside className="min-w-0 lg:pt-24"><Card size="sm"><CardHeader><CardTitle className="text-base">输出设置</CardTitle></CardHeader><CardContent className="space-y-5">
        <fieldset disabled={workspace.busy} className="min-w-0 space-y-4"><legend className="sr-only">{selected.title}参数</legend>
          <label className="block text-sm">输出格式<select aria-label="输出格式" className={field} value={String(params.format)} onChange={event => {setParams({...params, format:event.target.value}); workspace.resetResult();}}><option value="mp4">MP4 · H.264 / AAC</option><option value="webm">WebM · VP9 / Opus</option></select></label>
          {tool === 'compress' && <label className="block text-sm">压缩质量<input aria-label="压缩质量" type="number" min="20" max="40" step="1" className={field} value={params.crf ?? 28} onChange={event => parameter('crf', event.target.value)}/><span className="mt-2 block text-xs leading-relaxed text-muted-foreground">20 更清晰，40 更小。实际体积取决于素材，输出也可能比原文件大。</span></label>}
          {tool === 'resize' && <div className="grid grid-cols-2 gap-3">{[['width','宽度'],['height','高度']].map(([name,label]) => <label key={name} className="block text-sm">{label} px<input aria-label={label} className={field} type="number" min="2" max="3840" step="2" placeholder="自动" value={params[name] ?? ''} onChange={event => parameter(name, event.target.value)}/></label>)}</div>}
          {tool === 'trim' && <div className="space-y-4">{[['start','开始秒数'],['duration','片段时长（秒）']].map(([name,label]) => <label key={name} className="block text-sm">{label}<input aria-label={label} className={field} type="number" min={name === 'start' ? '0' : '0.1'} max="7200" step="0.1" value={params[name] ?? ''} onChange={event => parameter(name, event.target.value)}/></label>)}</div>}
        </fieldset>
        <Separator/>
        {workspace.ready === false && <p role="alert" className="text-sm leading-relaxed text-destructive">视频运行时未就绪，请使用包含原引擎和 FFmpeg 的完整桌面包。</p>}
        {(workspace.error || job?.error) && <p role="alert" className="break-words text-sm leading-relaxed text-destructive">{workspace.error || job?.error}</p>}
        <div role="status" aria-live="polite" className="text-sm text-muted-foreground">{workspace.busy ? <span className="flex items-center gap-2"><LoaderCircle className="animate-spin" size={16}/>{percentage === null ? '处理中…' : `已处理 ${percentage}%`}</span> : job?.state === 'cancelled' ? '已取消，本次半成品已清理。' : complete ? '处理完成，可以下载。' : '准备好后，生成新视频。'}</div>
        {workspace.busy && percentage !== null && <progress className="media-progress" max="100" value={percentage} aria-label="视频处理进度"/>}
        <Button className="w-full min-h-11" disabled={!input || workspace.busy || workspace.ready !== true} onClick={() => void workspace.run(tool, params)}><Film/>生成视频</Button>
        {!!job && ['queued','running'].includes(job.state) && <Button className="w-full min-h-11" variant="outline" disabled={workspace.pending} onClick={() => void workspace.cancel()}>取消处理</Button>}
        <p className="text-xs leading-relaxed text-muted-foreground">支持不超过 4K 像素总量、2 小时的视频。每次生成独立文件，不覆盖原件。</p>
      </CardContent></Card></aside>
    </main>
  </div>;
}
