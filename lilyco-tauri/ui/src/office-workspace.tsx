import { useEffect, useState } from "react";
import { AlertCircle, ArrowRight, BookOpen, Check, Copy, FileSearch, FileText, FolderOpen, LoaderCircle, Moon, ShieldCheck, Sun, Trash2, X } from "lucide-react";
import { Button } from "./components/ui/button";
import { Badge } from "./components/ui/badge";
import { Card, CardContent } from "./components/ui/card";
import { Separator } from "./components/ui/separator";
import { commandLabels, fileName } from "./office-bridge";
import { OfficeResultView } from "./office-result-view";
import { useOfficeWorkspace } from "./use-office-workspace";

export function OfficeWorkspace() {
  const work = useOfficeWorkspace();
  const [dark, setDark] = useState(() => {
    try { const saved = localStorage.getItem("office-theme"); return saved ? saved === "dark" : matchMedia("(prefers-color-scheme: dark)").matches; }
    catch { return false; }
  });
  const [tab, setTab] = useState<"readable" | "json">("readable");
  const [copy, setCopy] = useState("");
  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
    try { localStorage.setItem("office-theme", dark ? "dark" : "light"); } catch { /* Theme stays in this window. */ }
  }, [dark]);
  useEffect(() => { setCopy(""); setTab("readable"); }, [work.result]);

  async function copyJson() {
    if (!work.result) return;
    try { await navigator.clipboard.writeText(JSON.stringify(work.result.value, null, 2)); setCopy("已复制完整 JSON"); }
    catch { setCopy("无法自动复制，请打开原始 JSON 后选中内容复制。"); }
  }

  return <div className="min-h-screen bg-background text-foreground">
    <a href="#workspace" className="sr-only focus:not-sr-only focus:absolute focus:z-50 focus:rounded focus:bg-primary focus:p-3 focus:text-primary-foreground">跳到文件工作区</a>
    <header className="flex min-h-20 items-center justify-between gap-4 border-b border-border px-4 sm:px-7">
      <div className="flex min-w-0 items-center gap-3"><img src="./app-icon.png" width="46" height="46" alt="" className="shrink-0" /><div><h1 className="text-lg font-semibold tracking-tight">lilyco office</h1><p className="text-xs text-muted-foreground">文档透视 · 本机工作台</p></div></div>
      <div className="flex shrink-0 items-center gap-2"><Badge variant="secondary" className="hidden gap-1.5 sm:inline-flex"><ShieldCheck /> 本地读取</Badge><Button variant="ghost" size="icon-lg" aria-label={dark ? "切换浅色主题" : "切换深色主题"} onClick={() => setDark(!dark)}>{dark ? <Sun /> : <Moon />}</Button></div>
    </header>
    <div className="mx-auto grid max-w-[1600px] md:grid-cols-[220px_minmax(0,1fr)]">
      <aside aria-label="读取工具" className="border-b border-border bg-sidebar p-4 md:min-h-[calc(100vh-80px)] md:border-r md:border-b-0">
        <p className="mb-3 px-2 text-xs font-medium tracking-wider text-muted-foreground">问这份文件</p>
        <nav className="grid grid-cols-2 gap-1 md:grid-cols-1">
          {work.commands.map(item => <Button key={item.name} variant={work.command === item.name ? "secondary" : "ghost"} className="min-h-11 justify-start whitespace-normal px-3 text-left text-sm" disabled={work.busy} aria-pressed={work.command === item.name} onClick={() => work.setCommand(item.name)}><FileSearch className="shrink-0" />{commandLabels[item.name] || item.name}</Button>)}
        </nav>
        <Separator className="my-6" />
        <div className="flex items-center justify-between gap-2 px-2"><h2 className="text-xs font-medium text-muted-foreground">本次打开</h2>{work.recent.length > 0 && <Button variant="ghost" size="icon-sm" aria-label="清空本次文件记录" disabled={work.busy} onClick={work.forgetAll}><Trash2 /></Button>}</div>
        <p className="px-2 pt-2 text-xs leading-5 text-muted-foreground">只保留在当前窗口，可随时移除。</p>
        <div className="mt-3 space-y-1">{work.recent.map(path => <div key={path} className="flex min-w-0 items-center"><Button variant="ghost" className="min-h-10 min-w-0 flex-1 justify-start px-2" title={path} disabled={work.busy} onClick={() => work.selectFile(path)}><span className="truncate text-xs">{fileName(path)}</span></Button><Button variant="ghost" size="icon-sm" aria-label={`移除记录 ${fileName(path)}`} disabled={work.busy} onClick={() => work.forget(path)}><X /></Button></div>)}</div>
      </aside>
      <main id="workspace" className="min-w-0 space-y-6 p-4 sm:p-7 lg:p-10">
        <section aria-labelledby="intro"><p className="mb-2 text-xs font-medium tracking-[.12em] text-primary">OFFICE / INSPECT</p><h2 id="intro" className="text-2xl font-semibold tracking-tight sm:text-3xl">看清文件里的内容</h2><p className="mt-3 max-w-2xl text-base leading-7 text-muted-foreground">选择文档、表格、幻灯片或 PDF，读取正文与结构。文件留在本机，原件不会被修改。</p></section>
        <Card className={`rounded-xl border bg-card shadow-none ${work.dragging ? "border-primary" : "border-border"}`}>
          <CardContent className="space-y-5">
            <div className="flex flex-wrap items-center justify-between gap-4">
              <div className="flex min-w-0 flex-1 items-start gap-3"><span className="rounded-lg bg-muted p-3 text-primary"><FileText className="size-6" /></span><div className="min-w-0"><h3 className="break-all text-base font-medium" data-testid="selected-file">{work.file ? fileName(work.file) : "拖入文件，或点击选择"}</h3><p className="mt-1 break-all text-xs leading-5 text-muted-foreground">{work.file || "Word · Excel · PowerPoint · ODF · PDF"}</p></div></div>
              <Button variant="outline" className="min-h-11" disabled={work.busy} onClick={work.pickFile}><FolderOpen />{work.file ? "更换文件" : "选择文件"}</Button>
            </div>
            <Separator />
            <div className="flex flex-wrap items-center justify-between gap-4"><div className="min-w-0 flex-1"><p className="text-sm font-medium">{commandLabels[work.command] || work.command}</p><p className="mt-1 text-sm leading-6 text-muted-foreground">{work.commands.find(item => item.name === work.command)?.hint || "选择文件后会推荐相应的读取工具。"}</p></div><div className="flex gap-2">{work.file && <Button variant="ghost" className="min-h-11" disabled={work.busy} onClick={work.clear}>清除</Button>}<Button className="min-h-11 px-5" disabled={!work.file || work.busy || !work.commands.some(item => item.name === work.command)} onClick={work.readFile}>{work.busy ? <><LoaderCircle className="animate-spin" />读取中</> : <>读取文件<ArrowRight /></>}</Button></div></div>
          </CardContent>
        </Card>
        {work.error && <div role="alert" className="flex items-start gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-4 text-sm leading-7"><AlertCircle className="mt-1 size-5 shrink-0 text-destructive" /><p className="min-w-0 break-words">{work.error}</p></div>}
        <section aria-label="读取结果" className="min-w-0 rounded-xl border border-border bg-card">
          <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-4 py-4 sm:px-6"><div className="flex items-center gap-2"><BookOpen className="size-4 text-muted-foreground" /><h2 className="text-sm font-medium">读取结果</h2></div>{work.result && <div className="flex items-center gap-3"><span className="text-xs text-muted-foreground">{(work.result.elapsed / 1000).toFixed(2)} 秒</span><Button variant="outline" className="min-h-10" onClick={copyJson}><Copy />复制 JSON</Button></div>}</div>
          {!work.result ? <div className="flex min-h-64 flex-col items-center justify-center gap-4 p-6 text-center"><span className="rounded-full bg-muted p-4 text-muted-foreground">{work.busy ? <LoaderCircle className="size-7 animate-spin" /> : <FileSearch className="size-7" />}</span><div><p className="text-base font-medium">{work.busy ? "正在读取文件" : "你的下一份文件，清楚可见"}</p><p className="mt-2 max-w-sm text-sm leading-7 text-muted-foreground">{work.busy ? "完成后会在这里显示真实读取结果。" : "从左侧选择想查看的内容，再点击读取文件。"}</p></div></div> : <div className="min-w-0 p-4 sm:p-6">
            <p className="mb-4 break-all text-xs text-muted-foreground">{fileName(work.result.file)} · {commandLabels[work.result.command] || work.result.command}</p>
            <div className="mb-5 flex gap-1" role="group" aria-label="结果显示方式"><Button variant={tab === "readable" ? "secondary" : "ghost"} className="min-h-10" aria-pressed={tab === "readable"} onClick={() => setTab("readable")}>可读视图</Button><Button variant={tab === "json" ? "secondary" : "ghost"} className="min-h-10" aria-pressed={tab === "json"} onClick={() => setTab("json")}>原始 JSON</Button></div>
            <div data-testid="office-result" className="min-w-0">{tab === "readable" ? <OfficeResultView value={work.result.value} /> : <pre className="max-h-[70vh] overflow-auto rounded-lg bg-muted p-4 text-xs leading-6 whitespace-pre-wrap break-all">{JSON.stringify(work.result.value, null, 2)}</pre>}</div>
          </div>}
          {copy && <p role="status" className="flex items-center gap-2 border-t border-border px-6 py-3 text-sm text-muted-foreground"><Check className="size-4" />{copy}</p>}
        </section>
        <footer className="flex flex-wrap justify-between gap-3 text-xs leading-6 text-muted-foreground"><p>仅在当前窗口读取；不上传，不执行文档宏。</p><p>Lyco © lilyco-42 · 软件与角色资产许可分别适用</p></footer>
      </main>
    </div>
  </div>;
}
