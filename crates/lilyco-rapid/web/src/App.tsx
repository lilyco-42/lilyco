import { useEffect, useState, type FormEvent } from "react";
import {
  ArrowDownRight, ArrowRight, ArrowUpRight, Box, Braces, CircleDot,
  Database, Layers3, LayoutGrid, Orbit, Radio, Sparkles,
} from "lucide-react";
import { catalog, databaseReady, exampleFor, title, tryOperation, type Catalog, type Operation } from "@/lib/api";
import { Button } from "@/components/button";
import { Workbench } from "@/components/Workbench";
import { text } from "@/strings";

type Tab = "workspace" | "api";
type Connection = "checking" | "api_online" | "api_offline";

export function App() {
  const [data, setData] = useState<Catalog>();
  const [ready, setReady] = useState<boolean>();
  const [connection, setConnection] = useState<Connection>("checking");
  const [attempt, setAttempt] = useState(0);
  const [tab, setTab] = useState<Tab>(window.location.hash === "#api" ? "api" : "workspace");
  const [selection, setSelection] = useState("");

  useEffect(() => {
    let active = true;
    void databaseReady().then((value) => { if (active) setReady(value); });
    catalog().then((value) => {
      if (!active) return;
      setData(value);
      setConnection("api_online");
      document.title = value.name + " · " + text("studio");
    }).catch(() => {
      if (active) setConnection("api_offline");
    });
    return () => { active = false; };
  }, [attempt]);

  useEffect(() => {
    const onHash = () => setTab(window.location.hash === "#api" ? "api" : "workspace");
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const selected = data?.collections.find(({ name }) => name === selection) ?? data?.collections[0];
  const nav = [
    { id: "workspace", icon: LayoutGrid },
    { id: "api", icon: Braces },
  ] as const;

  return (
    <div className="shell">
      <header className="topbar">
        <a className="brand" href="#workspace" aria-label={text("studio")}><span className="brand-mark"><Box size={20} /></span>
          <strong>{text("brand")}<span>.</span></strong><span className="brand-divider" /><span className="brand-product">{text("studio")}</span></a>
        <span className={"connection " + connection} role="status"><i />{text(connection)}</span>
      </header>

      <aside className="sidebar">
        <div className="project">
          <span className="project-icon"><Layers3 size={19} /></span>
          <div><strong>{data?.name ?? text("studio")}</strong><span>{text("workspace")}</span></div>
        </div>
        <nav className="primary-nav" aria-label={text("workspace")}>
          {nav.map(({ id, icon: Icon }) => <a key={id} href={"#" + id}
            aria-current={tab === id ? "page" : undefined}><Icon size={17} />{text(id)}</a>)}
        </nav>
        {Boolean(data?.collections.length) && (
          <div className="resource-nav">
            <h2>{text("resources")}</h2>
            {data?.collections.map((collection) => (
              <button key={collection.name} className={selected?.name === collection.name && tab === "workspace" ? "selected" : ""}
                onClick={() => { setSelection(collection.name); window.location.hash = "workspace"; }}>
                <CircleDot size={15} />{title(collection.name)}
              </button>
            ))}
          </div>
        )}
        <div className="sidebar-footer"><Orbit size={18} /> <span>{text("runtime")}</span></div>
      </aside>

      <main className="main">
        <p className="breadcrumb">{text("workspace")} <span>/</span> {text(tab === "api" ? "api" : "overview")}</p>
        {connection === "api_offline" && <div className="inline-error offline" role="alert">
          {text("network_error")} <Button variant="secondary" onClick={() => {
            setConnection("checking"); setAttempt((previous) => previous + 1);
          }}>{text("retry")}</Button>
        </div>}
        {tab === "workspace" ? <>
          <section className="hero" aria-labelledby="hero-title">
            <div className="hero-illustration" aria-hidden="true">
              <div className="orbit orbit-outer" /><div className="orbit orbit-mid" /><div className="orbit orbit-inner" />
              <span className="hero-core"><Box size={45} strokeWidth={1.15} /></span>
              <Sparkles className="star star-one" size={22} /><Sparkles className="star star-two" size={14} />
              <span className="satellite" />
            </div>
            <div className="hero-content">
              <p><span />{text("dashboard")}</p>
              <h1 id="hero-title">{data ? title(data.name) : text("launch")}</h1>
              <div>{text("description")}</div>
              <a href="#api" className="hero-link">{text("view_api")} <ArrowRight size={15} /></a>
            </div>
          </section>
          <div className="metrics" aria-label={text("overview")}>
            <Metric icon={Layers3} label={text("resources")} value={String(data?.collections.length ?? "—")} />
            <Metric icon={Braces} label={text("contract")} value={data?.version ? "v" + data.version : "—"} />
            <Metric icon={Database} label={data?.storage ?? text("storage")} value={text(ready === undefined ? "checking" : ready ? "storage_ready" : "storage_offline")} />
          </div>
          {selected && <Workbench key={selected.path} collection={selected} />}
        </> : <Reference operations={data?.operations ?? []} />}
      </main>
    </div>
  );
}

type MetricProps = { icon: typeof Box; label: string; value: string };
function Metric({ icon: Icon, label, value }: MetricProps) {
  return <div className="metric card"><span className="metric-icon"><Icon size={17} /></span>
    <div><small>{label}</small><strong>{value}</strong></div></div>;
}

function Reference({ operations }: { operations: Operation[] }) {
  return (
    <section className="reference">
      <div className="reference-heading">
        <div><span className="collection-icon"><Radio size={20} /></span>
          <h1>{text("routes")}</h1><p>{text("route_description")}</p></div>
        <a className="outline-link" href="/openapi.json" target="_blank" rel="noreferrer">
          {text("schema")} <ArrowUpRight size={16} />
        </a>
      </div>
      <div className="endpoint-list card">
        {operations.map((operation) => <details key={operation.id}>
          <summary><span className={"method " + operation.method}>{operation.method.toUpperCase()}</span>
            <code>{operation.path}</code><ArrowDownRight size={16} /></summary>
          {operation.body !== undefined && <><h2>{text("request_body")}</h2>
            <pre>{JSON.stringify(operation.body, null, 2)}</pre></>}
          <TryRequest operation={operation} />
          <h2>{text("response")}</h2><pre>{JSON.stringify(operation.responses, null, 2)}</pre>
        </details>)}
      </div>
    </section>
  );
}

function TryRequest({ operation }: { operation: Operation }) {
  const [values, setValues] = useState<Record<string, string>>({});
  const [example, setExample] = useState(() => exampleFor(operation.body));
  const [response, setResponse] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setResponse("");
    try { setResponse(await tryOperation(operation, values, example)); }
    catch (error) { setResponse(error instanceof Error ? error.message : text("network_error")); }
    finally { setBusy(false); }
  }

  return <form className="try-form" onSubmit={(event) => void submit(event)}>
    {operation.params.map((parameter) => {
      const key = parameter.location + ":" + parameter.name;
      return <label key={key} className="field">
        <span>{parameter.name === "id" ? text("id") : title(parameter.name)}{" "}<small>{text(parameter.location === "path" ? "path_parameter" : "query_parameter")}</small></span>
        <input required={parameter.required} inputMode={parameter.kind === "integer" ? "numeric" : "text"}
          value={values[key] ?? ""} onChange={(event) => {
            const value = event.target.value;
            setValues((previous) => ({ ...previous, [key]: value }));
          }} />
      </label>;
    })}
    {operation.body !== undefined && <label className="field">
      <span>{text("try_json")}</span><textarea required value={example} rows={4}
        onChange={(event) => setExample(event.target.value)} /></label>}
    <Button type="submit" variant="secondary" disabled={busy}>{text(busy ? "loading" : "try_send")}</Button>
    {response && <pre role="status">{response}</pre>}
  </form>;
}
