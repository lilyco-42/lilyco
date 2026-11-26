import { useEffect, useRef, useState } from "react";
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
  if (state.phase === "idle") return null;
  const caption = t(state.phase === "done" ? "run_done" : state.phase === "error" ? "run_failed" : cancelSent ? "cancel_sent" : "running");
  const result = state.phase === "done" ? JSON.stringify(state.result, null, 2) ?? "null" : null;
  return <Card aria-labelledby="output-heading">
    <CardHeader><CardTitle><h2 id="output-heading">{t("output")}</h2></CardTitle></CardHeader>
    <CardContent className="grid min-w-0 gap-4">
      <div className="text-sm font-medium" role="status">{caption}</div>
      <Progress aria-label={t("progress")} value={state.percent === null ? null : state.percent * 100} />
      {state.phase === "error" && <Alert variant="destructive" role="alert"><AlertDescription>{state.message || t("stream_lost")}</AlertDescription></Alert>}
      <pre role="log" aria-label={t("output")} tabIndex={0} className="max-h-64 overflow-auto rounded-md bg-muted p-3 text-xs whitespace-pre-wrap wrap-anywhere">
        {state.log.join("\n") || t("waiting_output")}
      </pre>
      {result !== null && <section className="grid min-w-0 gap-2">
        <div className="flex items-center justify-between gap-2"><h3 className="text-sm font-medium">{t("result")}</h3><Copy value={result} label={t("copy")} /></div>
        <pre role="region" aria-label={t("run_result")} tabIndex={0} className="max-h-96 overflow-auto rounded-md bg-muted p-3 text-xs whitespace-pre-wrap wrap-anywhere">{result}</pre>
      </section>}
    </CardContent>
  </Card>;
}
