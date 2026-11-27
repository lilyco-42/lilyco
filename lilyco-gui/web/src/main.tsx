import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { ArrowDownIcon, ExternalLinkIcon, GlobeIcon, KeyboardIcon, LoaderCircleIcon, PlayIcon, TerminalIcon } from "lucide-react";
import { Alert, AlertDescription } from "@/ui/alert";
import { Button, buttonVariants } from "@/ui/button";
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from "@/ui/card";
import { Dialog, DialogClose, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "@/ui/dropdown-menu";
import { NativeSelect, NativeSelectOption } from "@/ui/native-select";
import { Skeleton } from "@/ui/skeleton";
import { TooltipProvider } from "@/ui/tooltip";
import { ArgumentField } from "./fields";
import { argumentValue, cliPreview, createDrafts } from "./arguments";
import { Language, translate, useLanguage, useText } from "./i18n";
import { Copy, RunOutput } from "./output";
import { useRun } from "./run";
import { fetchCatalog } from "./transport";
import { ViewNode } from "./views";
import type { UiCatalog, UiCli, UiCommand } from "./contract";
import { ThemeMenu } from "./theme";
import "./theme.css";

function App() {
  const {mode, locale, choose} = useLanguage();
  const [catalog, setCatalog] = useState<UiCatalog>();
  const [error, setError] = useState("");
  const t = translate(locale);

  useEffect(() => {
    const controller = new AbortController();
    void fetchCatalog(controller.signal).then(setCatalog).catch(failure => {
      if (!controller.signal.aborted) setError(String(failure instanceof Error ? failure.message : failure));
    });
    return () => controller.abort();
  }, []);
  useEffect(() => { document.documentElement.lang = locale; }, [locale]);
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
      <div className="mx-auto flex max-w-6xl flex-wrap items-center justify-between gap-2 px-4 py-3 sm:px-6 lg:px-8">
        <span className="text-sm font-semibold">Lilyco</span>
        <div className="flex gap-1">
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button variant="ghost" size="icon" aria-label={t("language")} title={t("language")} />}>
              <GlobeIcon aria-hidden="true" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuRadioGroup value={mode} onValueChange={choose}>
                <DropdownMenuRadioItem value="system" closeOnClick className="min-h-11">{t("language_system")}</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="zh-CN" closeOnClick className="min-h-11" lang="zh-CN">{t("language_chinese")}</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="en" closeOnClick className="min-h-11" lang="en">{t("language_english")}</DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>
          <ThemeMenu />
          <Shortcuts canCancel={catalog?.can_cancel ?? false} />
        </div>
      </div>
    </header>
    <main className="mx-auto grid w-full max-w-6xl min-w-0 gap-6 p-4 pt-6 pb-[max(2rem,env(safe-area-inset-bottom))] sm:px-6 sm:pt-10 lg:px-8">
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
  const about = t(`app.${command.name}.about`);
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
        {about && <p className="mt-1 text-sm text-muted-foreground wrap-anywhere">{about}</p>}
      </div>
      {catalog.commands.length > 1 && <NativeSelect aria-label={t("switch_command")} value={selected}
        className="w-full sm:max-w-60" disabled={blocked} onChange={event => select(event.target.value)}>
        {catalog.commands.map(item => <NativeSelectOption key={item.name} value={item.name}>{item.name}</NativeSelectOption>)}
      </NativeSelect>}
    </div>
    <CommandForm key={command.name} command={command} cli={catalog.cli} controls={{
      canCancel: catalog.can_cancel, setBlocked, canSelect: (name: string) => names.has(name) && name !== selected && !blocked, select,
    }} />
  </>;
}

type Control = { canCancel: boolean; setBlocked: (blocked: boolean) => void; canSelect: (name: string) => boolean; select: (name: string) => void };

function CommandForm({command, controls, cli}: {command: UiCommand; controls: Control; cli?: UiCli | null}) {
  const [drafts, setDrafts] = useState(() => createDrafts(command.args, location.search));
  const [files, setFiles] = useState(0);
  const {state, run, cancel, cancellable, cancelSent} = useRun();
  const form = useRef<HTMLFormElement>(null);
  const stop = useRef<HTMLButtonElement>(null);
  const t = useText();
  const running = state.phase === "running";
  const blocked = running || files > 0;
  const args = Object.fromEntries(command.args.map(definition => [definition.name, argumentValue(definition.kind, drafts[definition.name])]));
  const preview = cli ? cliPreview(cli, command.name, args) : null;

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

  return <div className="grid min-w-0 gap-5">
    <div className="grid min-w-0 items-start gap-5 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
      <Card className="min-w-0">
        <form ref={form} id="command-form" aria-labelledby="parameters-heading" noValidate
          onSubmit={event => {event.preventDefault(); if (!blocked) void run({cmd: command.name, args});}}>
          <CardHeader>
            <CardTitle><h2 id="parameters-heading">{t("parameters")}</h2></CardTitle>
          </CardHeader>
          {command.args.length > 0 && <CardContent className="mt-5 grid min-w-0 items-start gap-4 sm:grid-cols-2">
            {command.args.map(definition => <ArgumentField key={definition.name} arg={definition}
              label={t(`app.${command.name}.arg.${definition.name}`)} value={drafts[definition.name]} uploading={files > 0}
              onChange={update => setDrafts(previous => ({...previous, [definition.name]: update(previous[definition.name])}))}
              onBusy={delta => setFiles(count => count + delta)} />)}
          </CardContent>}
          <CardFooter className="mt-5 flex flex-wrap gap-2 border-t">
            <Button type="submit" size="lg" className="min-h-11 px-4" disabled={blocked}
              aria-keyshortcuts="Control+Enter Meta+Enter" aria-busy={running}>
              {running ? <LoaderCircleIcon aria-hidden="true" className="motion-safe:animate-spin" /> : <PlayIcon aria-hidden="true" />}
              {t(running ? "running" : "run")}
            </Button>
            {controls.canCancel && running && <Button ref={stop} variant="outline"
              aria-keyshortcuts="Control+Shift+Enter Meta+Shift+Enter"
              disabled={!cancellable} onClick={() => void cancel()}>{t("cancel")}</Button>}
            {state.phase !== "idle" && <a href="#run-output" className={buttonVariants({variant: "ghost", className: "min-h-11 lg:hidden"})}>
              {t("jump_to_output")} <ArrowDownIcon aria-hidden="true" />
            </a>}
          </CardFooter>
        </form>
      </Card>
      <div className="grid min-w-0 gap-4">
        <RunOutput state={state} cancelSent={cancelSent} />
        {preview !== null && <Card size="sm">
          <CardHeader className="flex flex-wrap items-center justify-between gap-2">
            <CardTitle><h2 className="flex items-center gap-2"><TerminalIcon className="size-4" aria-hidden="true" />{t("preview")}</h2></CardTitle>
            <Copy value={preview} label={t("copy_cli")} />
          </CardHeader>
          <CardContent><pre className="max-h-48 overflow-auto rounded-md bg-muted p-3 text-xs whitespace-pre-wrap wrap-anywhere">{preview}</pre></CardContent>
        </Card>}
      </div>
    </div>
    {state.views.size > 0 && <div className="grid min-w-0 gap-4">
      {[...state.views.entries()].map(([id, component]) =>
        <ViewNode key={id} component={component} actions={controls} />)}
    </div>}
  </div>;
}

function AgentAccess() {
  const t = useText();
  const links = [
    {href: "/llms.txt", name: "llms.txt", description: "llm_guide"},
    {href: "/.well-known/agent.json", name: "Agent Card", description: "agent_card"},
    {href: "/openapi.json", name: "OpenAPI", description: "api_schema"},
  ];
  return <footer className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 border-t pt-3">
    <div className="text-sm font-medium" id="agent-access-heading">{t("ai_access")}</div>
    <nav aria-labelledby="agent-access-heading" className="flex flex-wrap gap-1">{links.map(link =>
      <a key={link.href} href={link.href} target="_blank" rel="noopener" title={t(link.description)}
        className={buttonVariants({variant: "ghost", className: "min-h-11"})}>{link.name}<ExternalLinkIcon aria-hidden="true" /></a>)}</nav>
  </footer>;
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<App />);
