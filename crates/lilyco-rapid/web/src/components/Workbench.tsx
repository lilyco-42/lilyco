import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { ArrowDown, Check, ClipboardList, Pencil, Plus, Search, Trash2, X } from "lucide-react";
import { list, remove, save, title, type Collection, type Fields, type Row, type Value } from "@/lib/api";
import { Button } from "./button";
import { text } from "@/strings";

type Action = { kind: "create" } | { kind: "edit" | "delete"; row: Row };

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : text("network_error");
}

export function Workbench({ collection }: { collection: Collection }) {
  const [rows, setRows] = useState<Row[]>([]);
  const [next, setNext] = useState<number | null>(null);
  const [filter, setFilter] = useState("");
  const [action, setAction] = useState<Action | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const path = collection.path;

  const reload = useCallback(async (after?: number, ignore?: () => boolean) => {
    setLoading(true);
    setError("");
    try {
      const result = await list(path, after);
      if (ignore?.()) return;
      setRows((previous) => after ? [...previous, ...result.items] : result.items);
      setNext(result.next);
    } catch (failure) {
      if (!ignore?.()) setError(errorText(failure));
    } finally {
      if (!ignore?.()) setLoading(false);
    }
  }, [path]);

  useEffect(() => {
    let canceled = false;
    void reload(undefined, () => canceled);
    return () => { canceled = true; };
  }, [reload]);

  const shown = rows.filter((row) => Object.values(row.data).some((value) =>
    String(value).toLowerCase().includes(filter.trim().toLowerCase())));

  async function apply(fields: Fields) {
    if (!action) return;
    if (action.kind === "delete") {
      await remove(path, action.row.id);
      setRows((existing) => existing.filter((row) => row.id !== action.row.id));
    } else {
      const saved = await save(path, fields, action.kind === "edit" ? action.row.id : undefined);
      if (action.kind === "edit") {
        setRows((existing) => existing.map((row) => row.id === saved.id ? saved : row));
      } else if (next === null) {
        setRows((existing) => [...existing, saved]);
      } else {
        await reload();
      }
    }
    setAction(null);
  }

  return (
    <section className="collection card" aria-labelledby="collection-title">
      <div className="collection-head">
        <div className="collection-title">
          <span className="collection-icon"><ClipboardList size={20} /></span>
          <div><h2 id="collection-title">{title(collection.name)}</h2><code>{collection.path}</code></div>
        </div>
        <Button className="min-h-11" onClick={() => setAction({ kind: "create" })}>
          <Plus size={16} /> {text("new")}
        </Button>
      </div>
      <div className="toolbar">
        <label className="search">
          <Search size={16} />
          <input type="search" value={filter} onChange={(event) => setFilter(event.target.value)}
            placeholder={text("search")} aria-label={text("search")} />
        </label>
        <span className="muted">{rows.length} {text("loaded")}</span>
      </div>
      {error && <div className="inline-error" role="alert">{error} <Button variant="secondary" onClick={() => void reload()}>{text("retry")}</Button></div>}
      {loading && rows.length === 0 && <p className="empty" role="status">{text("loading")}</p>}
      {!loading && !error && shown.length === 0 && (
        <div className="empty"><ClipboardList size={28} /><strong>{text(filter ? "no_matches" : "no_records")}</strong>
          {!filter && <span>{text("empty_help")}</span>}</div>
      )}
      {shown.length > 0 && <div className="record-list">
        {shown.map((row) => <article key={row.id} className="record">
          <div className="record-values">{collection.fields.map((field) => <div key={field.name}>
            <small>{title(field.name)}</small><ValueView value={row.data[field.name]} />
          </div>)}</div>
          <div className="record-actions">
            <Button size="icon" variant="ghost" title={text("edit")} aria-label={text("edit") + " " + row.id}
              onClick={() => setAction({ kind: "edit", row })}><Pencil size={16} /></Button>
            <Button size="icon" variant="ghost" title={text("delete")} aria-label={text("delete") + " " + row.id}
              onClick={() => setAction({ kind: "delete", row })}><Trash2 size={16} /></Button>
          </div>
        </article>)}
      </div>}
      {next !== null && <div className="collection-foot"><Button variant="secondary" disabled={loading}
        onClick={() => void reload(next)}><ArrowDown size={15} />{text("load_more")}</Button></div>}
      {action && <Editor action={action} collection={collection} close={() => setAction(null)} apply={apply} />}
    </section>
  );
}

function ValueView({ value }: { value: Value | undefined }) {
  if (typeof value === "boolean") return <span className={"boolean " + (value ? "truthy" : "")}>
    {value ? <Check size={12} /> : null}{text(value ? "yes" : "no")}</span>;
  return <strong>{value}</strong>;
}

type EditorProps = {
  action: Action;
  collection: Collection;
  close: () => void;
  apply: (fields: Fields) => Promise<void>;
};

function Editor({ action, collection, close, apply }: EditorProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [values, setValues] = useState<Fields>(action.kind === "create"
    ? Object.fromEntries(collection.fields.map(({ name, kind }) => [name, kind === "boolean" ? false : kind === "integer" ? 0 : ""]))
    : action.row.data);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const deleting = action.kind === "delete";

  useEffect(() => { dialog.current?.showModal(); }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try { await apply(values); } catch (failure) { setError(errorText(failure)); }
    finally { setBusy(false); }
  }

  return (
    <dialog ref={dialog} className="editor card" aria-labelledby="editor-title"
      onCancel={(event) => { event.preventDefault(); if (!busy) close(); }}>
      <form onSubmit={(event) => void submit(event)}>
        <div className="editor-header">
          <h2 id="editor-title">{text(deleting ? "delete_heading" : action.kind === "edit" ? "edit" : "new")}</h2>
          <Button type="button" variant="ghost" size="icon" aria-label={text("cancel")} disabled={busy} onClick={close}><X size={18} /></Button>
        </div>
        {deleting ? <p className="muted">{text("delete_description")}</p> : collection.fields.map(({ name, kind }) => (
          <label className={"field " + (kind === "boolean" ? "checkbox" : "")} key={name}>
            <span>{title(name)}</span>
            <input type={kind === "boolean" ? "checkbox" : kind === "integer" ? "number" : "text"}
              checked={kind === "boolean" ? Boolean(values[name]) : undefined}
              value={kind === "boolean" ? undefined : String(values[name] ?? "")}
              required={kind !== "boolean"} min={kind === "integer" ? -2147483648 : undefined}
              max={kind === "integer" ? 2147483647 : undefined}
              onChange={(event) => {
                const input = event.currentTarget;
                const value = kind === "boolean" ? input.checked
                  : kind === "integer" && input.value !== "" ? input.valueAsNumber : input.value;
                setValues((previous) => ({ ...previous, [name]: value }));
              }} />
          </label>
        ))}
        {error && <p className="inline-error" role="alert">{error}</p>}
        <div className="editor-footer">
          <Button type="button" variant="secondary" disabled={busy} onClick={close}>{text("cancel")}</Button>
          <Button variant={deleting ? "destructive" : "default"} disabled={busy}>
            {text(busy ? "loading" : deleting ? "delete" : "save")}
          </Button>
        </div>
      </form>
    </dialog>
  );
}
