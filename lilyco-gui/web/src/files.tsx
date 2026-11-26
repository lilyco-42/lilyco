import { useRef, useState, type DragEvent } from "react";
import { Button } from "@/ui/button";
import { Input } from "@/ui/input";
import { useText } from "./i18n";
import { config, request } from "./transport";

function encodeFile(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",", 2)[1]);
    reader.onerror = () => reject(reader.error ?? new Error(file.name));
    reader.readAsDataURL(file);
  });
}

type FileInputProps = {
  id: string;
  value: string;
  required?: boolean;
  mustExist: boolean;
  onChange: (path: string) => void;
  onBusy: (delta: number) => void;
};

export function FileInput({ id, value, required, mustExist, onChange, onBusy }: FileInputProps) {
  const t = useText();
  const fileInput = useRef<HTMLInputElement>(null);
  const pending = useRef(false);
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [uploaded, setUploaded] = useState("");
  const [feedback, setFeedback] = useState<{key: string; values?: Record<string, string>}>({key: ""});

  async function transfer(work: () => Promise<void>, failure: string) {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    onBusy(1);
    try { await work(); }
    catch (error) { setFeedback({key: failure, values: {reason: String(error instanceof Error ? error.message : error)}}); }
    finally {
      pending.current = false;
      setBusy(false);
      onBusy(-1);
    }
  }

  function upload(files: FileList | null) {
    if (!files?.length || pending.current) return;
    if (files.length > 1) { setFeedback({key: "single_file"}); return; }
    const file = files[0];
    if (file.size > Number(config("upload-limit"))) {
      const reason = t("oversize", {name: file.name, actual: file.size.toLocaleString(), limit: Number(config("upload-limit")).toLocaleString()});
      setFeedback({key: "upload_failed", values: {reason}});
      return;
    }
    void transfer(async () => {
      const size = new Intl.NumberFormat(document.documentElement.lang, {style: "unit", unit: "byte"}).format(file.size);
      setFeedback({key: "uploading", values: {name: file.name, size}});
      const result = await request("/upload", {name: file.name, data_b64: await encodeFile(file)});
      if (typeof result.path !== "string" || !result.path) throw new Error(t("http_no_reason"));
      onChange(result.path);
      setUploaded(file.name);
      setFeedback({key: "uploaded"});
    }, "upload_failed");
  }

  function choosePath() {
    void transfer(async () => {
      setFeedback({key: "picking"});
      const result = await request("/pick", {});
      if (result.error) throw new Error(result.error);
      if (result.path) {
        onChange(result.path);
        setUploaded("");
        setFeedback({key: "picked", values: {path: result.path}});
      } else setFeedback({key: "pick_cancelled"});
    }, "pick_failed");
  }

  function drag(event: DragEvent<HTMLDivElement>) {
    if (!event.dataTransfer.types.includes("Files")) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = busy ? "none" : "copy";
    setDragging(!busy);
  }

  const status = feedback.key && t(feedback.key, feedback.values);
  return <div className="grid min-w-0 gap-2">
    <Input id={id} type="text" value={value} required={required} readOnly={busy}
      aria-describedby={id + "-status"} className="h-11" spellCheck={false}
      onChange={event => { onChange(event.target.value); setUploaded(""); setFeedback({key: ""}); }} />
    <div className={`grid min-w-0 gap-3 rounded-lg border border-dashed p-3 ${dragging ? "border-ring bg-accent" : "border-input"}`}
      aria-busy={busy} onDragOver={drag}
      onDragLeave={event => {
        if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setDragging(false);
      }}
      onDrop={event => { event.preventDefault(); setDragging(false); upload(event.dataTransfer.files); }}>
      <input ref={fileInput} type="file" hidden onChange={event => { upload(event.target.files); event.target.value = ""; }} />
      <div className="text-sm text-muted-foreground">{t(dragging ? "drop_active" : mustExist ? "drop_required" : "drop_optional")}</div>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" disabled={busy} onClick={() => fileInput.current?.click()}>{t("browse")}</Button>
        {config("native-picker") === "1" && <Button variant="outline" disabled={busy} onClick={choosePath}>{t("native")}</Button>}
        {uploaded && <Button variant="ghost" className="max-w-full"
          aria-label={t("remove_file", {name: uploaded})} disabled={busy}
          onClick={() => { onChange(""); setUploaded(""); setFeedback({key: ""}); document.getElementById(id)?.focus(); }}>
          <span className="truncate">{uploaded}</span><span aria-hidden="true">×</span>
        </Button>}
      </div>
    </div>
    <div id={id + "-status"} className={`text-sm break-all ${feedback.key.endsWith("_failed") || feedback.key === "single_file" ? "text-destructive" : "text-muted-foreground"}`} role="status">{status}</div>
  </div>;
}
