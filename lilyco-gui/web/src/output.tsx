import { useEffect, useRef, useState } from "react";
import { CircleAlertIcon, CircleCheckIcon, LoaderCircleIcon, ScrollTextIcon } from "lucide-react";
import { Button } from "@/ui/button";
import { Alert, AlertDescription } from "@/ui/alert";
import { Card, CardContent, CardHeader, CardTitle } from "@/ui/card";
import { Progress } from "@/ui/progress";
import { useText } from "./i18n";
import type { useRun } from "./run";

export function Copy({ value, label }: {value: string; label: string}) {
  const [status, setStatus] = useState("");
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const t = useText();
  useEffect(() => () => clearTimeout(timer.current), []);
  async function copy() {
    let key = "copied";
    try { await navigator.clipboard.writeText(value); } catch { key = "copy_failed"; }
    setStatus(key);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setStatus(""), 1200);
  }
  return <Button type="button" variant="outline" className="shrink-0" onClick={copy} aria-live="polite">{status ? t(status) : label}</Button>;
}

export function RunOutput({state, cancelSent}: { state: ReturnType<typeof useRun>["state"]; cancelSent: boolean }) {
  const t = useText();
  const idle = state.phase === "idle";
  const running = state.phase === "running";
  const caption = idle ? "" : t(state.phase === "done" ? "run_done" : state.phase === "error" ? "run_failed" : cancelSent ? "cancel_sent" : "running");
  const Icon = running ? LoaderCircleIcon : state.phase === "done" ? CircleCheckIcon : CircleAlertIcon;
  const result = state.phase === "done" ? JSON.stringify(state.result, null, 2) ?? "null" : null;
  return <Card role="region" id="run-output" tabIndex={-1} aria-labelledby="output-heading" className="min-w-0 focus-visible:ring-2 focus-visible:ring-ring">
    <CardHeader>
      <CardTitle><h2 id="output-heading">{t("output")}</h2></CardTitle>
      <div className="flex items-center gap-2 text-sm text-muted-foreground empty:sr-only" role="status">
        {caption && <><Icon aria-hidden="true" className={running ? "size-4 shrink-0 motion-safe:animate-spin" : "size-4 shrink-0"} />{caption}</>}
      </div>
    </CardHeader>
    <CardContent className="grid min-w-0 gap-4">
      {idle && <div className="grid justify-items-center gap-2 rounded-lg border border-dashed px-4 py-7 text-center text-sm text-muted-foreground">
        <ScrollTextIcon aria-hidden="true" className="size-5" />
        <p>{t("output_empty")}</p>
      </div>}
      {running && <Progress aria-label={t("progress")} value={state.percent === null ? null : state.percent * 100} />}
      {state.phase === "error" && <Alert variant="destructive" role="alert"><AlertDescription>{state.message || t("stream_lost")}</AlertDescription></Alert>}
      {result !== null && <section className="grid min-w-0 gap-2">
        <div className="flex items-center justify-between gap-2"><h3 className="text-sm font-medium">{t("result")}</h3><Copy value={result} label={t("copy")} /></div>
        <pre role="region" aria-label={t("run_result")} tabIndex={0} className="max-h-96 overflow-auto rounded-md bg-muted p-3 text-xs whitespace-pre-wrap wrap-anywhere">{result}</pre>
      </section>}
      {(running || state.log.length > 0) && <section className="grid min-w-0 gap-2">
        <h3 className="text-sm font-medium" id="log-heading">{t("run_log")}</h3>
        <pre role="log" aria-labelledby="log-heading" tabIndex={0} className="max-h-48 overflow-auto rounded-md bg-muted p-3 text-xs whitespace-pre-wrap wrap-anywhere">
          {state.log.join("\n") || t("waiting_output")}
        </pre>
      </section>}
    </CardContent>
  </Card>;
}
