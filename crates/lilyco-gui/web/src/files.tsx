import { useRef, useState, type DragEvent } from "react";
import { FileIcon, UploadIcon, XIcon } from "lucide-react";
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
  const [uploaded, setUploaded] = useState<{name: string; path: string}>();
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
      setUploaded({name: file.name, path: result.path});
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
        setUploaded(undefined);
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
  const attachment = uploaded?.path === value ? uploaded : undefined;
  const path = <Input id={id} type="text" value={value} required={required} readOnly={busy}
    aria-describedby={id + "-status"} className="h-11" spellCheck={false}
    onChange={event => { onChange(event.target.value); setUploaded(undefined); setFeedback({key: ""}); }} />;

  return <div className="grid min-w-0 gap-2">
    {!mustExist && path}
    <div className={`grid min-w-0 gap-3 rounded-lg border border-dashed p-3 sm:p-4 ${dragging ? "border-ring bg-accent" : "border-input bg-muted/35"}`}
      aria-busy={busy} onDragOver={drag}
      onDragLeave={event => {
        if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setDragging(false);
      }}
      onDrop={event => { event.preventDefault(); setDragging(false); upload(event.dataTransfer.files); }}>
      <input ref={fileInput} type="file" hidden onChange={event => { upload(event.target.files); event.target.value = ""; }} />
      <div className="flex min-w-0 items-center gap-2.5 text-sm text-muted-foreground">
        <UploadIcon className="size-4 shrink-0" aria-hidden="true" />
        <span>{t(dragging ? "drop_active" : mustExist ? "drop_required" : "drop_optional")}</span>
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <Button variant={mustExist ? "secondary" : "outline"} disabled={busy}
          onClick={() => fileInput.current?.click()}>{t("browse")}</Button>
        {config("native-picker") === "1" && <Button variant="ghost" disabled={busy} title={t("native_title")}
          onClick={choosePath}>{t("native")}</Button>}
      </div>
      {attachment && <div className="flex min-w-0 items-center gap-2 rounded-md border bg-background px-2.5">
        <FileIcon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate text-sm font-medium" title={attachment.name}>{attachment.name}</span>
        <Button variant="ghost" size="icon-sm" aria-label={t("remove_file", {name: attachment.name})} disabled={busy}
          onClick={() => { onChange(""); setUploaded(undefined); setFeedback({key: ""}); document.getElementById(id)?.focus(); }}>
          <XIcon aria-hidden="true" />
        </Button>
      </div>}
    </div>
    {mustExist && path}
    <div id={id + "-status"} className={`empty:sr-only text-sm wrap-anywhere ${feedback.key.endsWith("_failed") || feedback.key === "single_file" ? "text-destructive" : "text-muted-foreground"}`} role="status">{status}</div>
  </div>;
}
