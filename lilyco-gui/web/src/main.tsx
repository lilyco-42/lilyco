import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { KeyboardIcon, MoonIcon, SunIcon } from "lucide-react";
import { Alert, AlertDescription } from "@/ui/alert";
import { Button } from "@/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/ui/card";
import { Dialog, DialogClose, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/ui/dialog";
import { NativeSelect, NativeSelectOption } from "@/ui/native-select";
import { Skeleton } from "@/ui/skeleton";
import { TooltipProvider } from "@/ui/tooltip";
import { ArgumentField } from "./fields";
import { argumentValue, cliPreview, createDrafts } from "./arguments";
import { Language, preference, setPreference, translate, useText, type Locale } from "./i18n";
import { Copy, RunOutput } from "./output";
import { useRun } from "./run";
import { fetchCatalog } from "./transport";
import { ViewNode } from "./views";
import type { UiCatalog, UiCommand } from "./contract";
import "./theme.css";

function App() {
  const [locale, setLocale] = useState<Locale>(preference("lilyco-language") === "en" ? "en" : "zh-CN");
  const [catalog, setCatalog] = useState<UiCatalog>();
  const [error, setError] = useState("");
  const [dark, setDark] = useState(() => {
    const saved = preference("lilyco-theme");
    return saved ? saved === "dark" : matchMedia("(prefers-color-scheme: dark)").matches;
  });
  const t = translate(locale);

  useEffect(() => {
    const controller = new AbortController();
    void fetchCatalog(controller.signal).then(setCatalog).catch(failure => {
      if (!controller.signal.aborted) setError(String(failure instanceof Error ? failure.message : failure));
    });
    return () => controller.abort();
  }, []);
  useEffect(() => { document.documentElement.lang = locale; }, [locale]);
  useEffect(() => { document.documentElement.classList.toggle("dark", dark); }, [dark]);
  useEffect(() => {
    const blockFiles = (event: DragEvent) => {
      if (event.dataTransfer?.types.includes("Files")) event.preventDefault();
    };
    window.addEventListener("dragover", blockFiles);
    window.addEventListener("drop", blockFiles);
    return () => { window.removeEventListener("dragover", blockFiles); window.removeEventListener("drop", blockFiles); };
  }, []);

  return <Language.Provider value={t}><TooltipProvider>
    <header className="border-b">
      <div className="mx-auto flex max-w-5xl flex-wrap items-center justify-between gap-2 px-4 py-3 sm:px-6">
        <span className="text-sm font-semibold">Lilyco</span>
        <div className="flex gap-1">
          <Button variant="ghost" aria-label={t("language")} onClick={() => {
            const next = locale === "en" ? "zh-CN" : "en";
            setLocale(next); setPreference("lilyco-language", next);
          }}>{t("switch_language")}</Button>
          <Button variant="ghost" size="icon" aria-label={t("theme")} aria-pressed={dark} onClick={() => {
            setDark(!dark); setPreference("lilyco-theme", dark ? "light" : "dark");
          }}>{dark ? <SunIcon /> : <MoonIcon />}</Button>
          <Shortcuts canCancel={catalog?.can_cancel ?? false} />
        </div>
      </div>
    </header>
    <main className="mx-auto grid w-full max-w-5xl min-w-0 gap-6 p-4 pb-[max(2rem,env(safe-area-inset-bottom))] sm:px-6 sm:pt-8">
      {error ? <Alert variant="destructive" role="alert"><AlertDescription>{t("request_failed", {reason: error})}</AlertDescription></Alert>
        : catalog ? <Workspace catalog={catalog} /> : <Skeleton className="h-36 w-full" aria-label={t("loading_catalog")} />}
      <AgentAccess />
    </main>
  </TooltipProvider></Language.Provider>;
}

function Shortcuts({canCancel}: {canCancel: boolean}) {
  const t = useText();
  return <Dialog>
    <DialogTrigger render={<Button variant="ghost" size="icon" aria-label={t("shortcuts")} />}><KeyboardIcon /></DialogTrigger>
    <DialogContent showCloseButton={false}>
      <DialogHeader><DialogTitle>{t("shortcuts")}</DialogTitle></DialogHeader>
      <dl className="grid gap-3 text-sm">
        <div className="flex flex-wrap justify-between gap-2"><dt>{t("run")}</dt><dd><kbd>Ctrl / ⌘ + Enter</kbd></dd></div>
        {canCancel && <div className="flex flex-wrap justify-between gap-2"><dt>{t("focus_cancel")}</dt><dd><kbd>Ctrl / ⌘ + Shift + Enter</kbd></dd></div>}
        <div className="flex flex-wrap justify-between gap-2"><dt>{t("dismiss_dialog")}</dt><dd><kbd>Esc</kbd></dd></div>
      </dl>
      <DialogFooter><DialogClose render={<Button variant="outline" />}>{t("close")}</DialogClose></DialogFooter>
    </DialogContent>
  </Dialog>;
}

function Workspace({catalog}: {catalog: UiCatalog}) {
  const [selected, setSelected] = useState(catalog.selected);
  const [blocked, setBlocked] = useState(false);
  const t = useText();
  const command = catalog.commands.find(item => item.name === selected);
  if (!command) return null;
  const names = new Set(catalog.commands.map(item => item.name));
  function select(next: string) {
    if (blocked || !names.has(next) || next === selected) return;
    const url = new URL(location.pathname, location.origin);
    url.searchParams.set("cmd", next);
    history.replaceState(null, "", url);
    setSelected(next);
  }

  return <>
    <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start">
      <div className="min-w-0">
        <h1 className="text-2xl font-semibold tracking-tight wrap-anywhere sm:text-3xl">{command.name}</h1>
        {command.about && <p className="mt-1 text-sm text-muted-foreground wrap-anywhere">{command.about}</p>}
      </div>
      {catalog.commands.length > 1 && <NativeSelect aria-label={t("switch_command")} value={selected}
        className="w-full sm:max-w-60" disabled={blocked} onChange={event => select(event.target.value)}>
        {catalog.commands.map(item => <NativeSelectOption key={item.name} value={item.name}>{item.name}</NativeSelectOption>)}
      </NativeSelect>}
    </div>
    <CommandForm key={command.name} command={command} controls={{
      canCancel: catalog.can_cancel, setBlocked, canSelect: (name: string) => names.has(name) && name !== selected && !blocked, select,
    }} />
  </>;
}

type Control = { canCancel: boolean; setBlocked: (blocked: boolean) => void; canSelect: (name: string) => boolean; select: (name: string) => void };

function CommandForm({command, controls}: {command: UiCommand; controls: Control}) {
  const [drafts, setDrafts] = useState(() => createDrafts(command.args, location.search));
  const [files, setFiles] = useState(0);
  const {state, run, cancel, cancellable, cancelSent} = useRun();
  const form = useRef<HTMLFormElement>(null);
  const stop = useRef<HTMLButtonElement>(null);
  const t = useText();
  const running = state.phase === "running";
  const blocked = running || files > 0;
  const args = Object.fromEntries(command.args.map(definition => [definition.name, argumentValue(definition.kind, drafts[definition.name])]));
  const preview = cliPreview(command.name, args);

  useEffect(() => { document.title = t("document_title", {command: command.name}); }, [command.name, t]);
  useEffect(() => { controls.setBlocked(blocked); }, [blocked, controls.setBlocked]);
  useEffect(() => {
    function shortcut(event: KeyboardEvent) {
      if (event.defaultPrevented || event.isComposing || event.repeat || event.altKey || event.ctrlKey === event.metaKey || event.key !== "Enter") return;
      if (document.querySelector("[aria-modal=true]")) return;
      if (event.shiftKey) {
        if (stop.current && !stop.current.disabled) { event.preventDefault(); stop.current.focus(); }
      } else if (!blocked) { event.preventDefault(); form.current?.requestSubmit(); }
    }
    window.addEventListener("keydown", shortcut);
    return () => window.removeEventListener("keydown", shortcut);
  }, [blocked]);

  return <>
    <Card aria-labelledby="parameters-heading">
      <CardHeader>
        <CardTitle><h2 id="parameters-heading">{t("parameters")}</h2></CardTitle>
      </CardHeader>
      <form ref={form} id="command-form" noValidate onSubmit={event => {event.preventDefault(); if (!blocked) void run({cmd: command.name, args});}}>
        <CardContent className="grid min-w-0 gap-4">
          {command.args.map(definition => <ArgumentField key={definition.name} arg={definition} value={drafts[definition.name]}
            onChange={value => setDrafts(previous => ({...previous, [definition.name]: value}))}
            onBusy={delta => setFiles(count => count + delta)} />)}
        </CardContent>
        <CardFooter className="mt-4 flex flex-wrap gap-2">
          <Button type="submit" size="lg" disabled={blocked}
            aria-keyshortcuts="Control+Enter Meta+Enter" aria-busy={running}>{t(running ? "running" : "run")}</Button>
          {controls.canCancel && running && <Button ref={stop} variant="outline"
            aria-keyshortcuts="Control+Shift+Enter Meta+Shift+Enter"
            disabled={!cancellable} onClick={() => void cancel()}>{t("cancel")}</Button>}
        </CardFooter>
      </form>
    </Card>
    <Card size="sm">
      <CardHeader className="flex flex-wrap items-center justify-between gap-2">
        <CardTitle><h2>{t("preview")}</h2></CardTitle><Copy value={preview} label={t("copy_cli")} />
      </CardHeader>
      <CardContent><pre className="rounded-md bg-muted p-3 text-xs whitespace-pre-wrap wrap-anywhere">{preview}</pre></CardContent>
    </Card>
    <RunOutput state={state} cancelSent={cancelSent} />
    <div className="grid min-w-0 gap-4">
      {command.views.filter(slot => !state.views.has(slot.id)).map(slot =>
        <Skeleton key={slot.id} className="h-12" aria-label={slot.title ?? slot.id} />)}
      {[...state.views.entries()].map(([id, component]) =>
        <ViewNode key={id} component={component} actions={controls} />)}
    </div>
  </>;
}

function AgentAccess() {
  const t = useText();
  const links = [
    {href: "/llms.txt", name: "llms.txt", description: "llm_guide"},
    {href: "/.well-known/agent.json", name: "Agent Card", description: "agent_card"},
    {href: "/openapi.json", name: "OpenAPI", description: "api_schema"},
  ];
  return <Card size="sm">
    <CardHeader><CardTitle><h2>{t("ai_access")}</h2></CardTitle><CardDescription>{t("footer")}</CardDescription></CardHeader>
    <CardContent className="flex flex-wrap gap-2">{links.map(link =>
      <Button key={link.href} variant="outline" nativeButton={false} title={t(link.description)}
        render={<a href={link.href} target="_blank" rel="noopener" />}>{link.name}</Button>)}</CardContent>
  </Card>;
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<App />);
