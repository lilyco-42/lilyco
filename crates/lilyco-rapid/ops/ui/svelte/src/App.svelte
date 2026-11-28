<script lang="ts">
  import { onMount } from "svelte";
  import { catalog, databaseReady, title, type Catalog } from "@/lib/api";
  import { text } from "@/strings";
  import Request from "./Request.svelte";
  import Workbench from "./Workbench.svelte";

  let data = $state<Catalog>();
  let ready = $state<boolean>();
  let connection = $state<"checking" | "api_online" | "api_offline">("checking");
  let tab = $state("workspace");
  let selection = $state("");
  const selected = $derived(data?.collections.find(({ name }) => name === selection) ?? data?.collections[0]);

  function locationChanged() {
    tab = location.hash === "#api" ? "api" : "workspace";
  }

  async function load() {
    connection = "checking";
    const status = databaseReady();
    try {
      data = await catalog();
      connection = "api_online";
      document.title = data.name + " · " + text("studio");
    } catch { connection = "api_offline"; }
    finally { ready = await status; }
  }

  function select(name: string) {
    selection = name;
    location.hash = "workspace";
  }

  onMount(() => {
    locationChanged();
    void load();
  });
</script>

<svelte:window onhashchange={locationChanged} />

<div class="shell">
  <header class="topbar">
    <a class="brand" href="#workspace" aria-label={text("studio")}>
      <strong>{text("brand")}<span>.</span></strong><span class="brand-divider"></span>
      <span class="brand-product">{text("studio")}</span>
    </a>
    <span class={"connection " + connection} role="status"><i></i>{text(connection)}</span>
  </header>
  <aside class="sidebar">
    <div class="project"><div><strong>{data?.name ?? text("studio")}</strong><span>{text("workspace")}</span></div></div>
    <nav class="primary-nav" aria-label={text("workspace")}>
      <a href="#workspace" aria-current={tab === "workspace" ? "page" : undefined}>{text("workspace")}</a>
      <a href="#api" aria-current={tab === "api" ? "page" : undefined}>{text("api")}</a>
    </nav>
    {#if data?.collections.length}
      <div class="resource-nav">
        <h2>{text("resources")}</h2>
        {#each data.collections as collection (collection.path)}
          <button class:selected={tab === "workspace" && collection.path === selected?.path} onclick={() => select(collection.name)}>{title(collection.name)}</button>
        {/each}
      </div>
    {/if}
  </aside>
  <main class="main">
    <p class="breadcrumb">{text("workspace")} <span>/</span> {text(tab === "api" ? "api" : "overview")}</p>
    {#if connection === "api_offline"}
      <div class="inline-error offline" role="alert">
        {text("network_error")} <button class="rapid-button secondary" onclick={load}>{text("retry")}</button>
      </div>
    {/if}
    {#if tab === "api"}
      <section class="reference">
        <div class="reference-heading">
          <div><h1>{text("routes")}</h1><p>{text("route_description")}</p></div>
          <a class="outline-link" href="/openapi.json" target="_blank" rel="noreferrer">{text("schema")}</a>
        </div>
        <div class="endpoint-list card">
          {#each data?.operations ?? [] as operation (operation.id)}
            <details>
              <summary><span class={"method " + operation.method}>{operation.method.toUpperCase()}</span><code>{operation.path}</code></summary>
              {#if operation.body !== undefined}<h2>{text("request_body")}</h2><pre>{JSON.stringify(operation.body, null, 2)}</pre>{/if}
              <Request {operation} />
              <h2>{text("response")}</h2><pre>{JSON.stringify(operation.responses, null, 2)}</pre>
            </details>
          {/each}
        </div>
      </section>
    {:else}
      <section class="hero" aria-labelledby="hero-title">
        <div class="hero-illustration" aria-hidden="true">
          <div class="orbit orbit-outer"></div><div class="orbit orbit-mid"></div><div class="orbit orbit-inner"></div>
          <span class="satellite"></span>
        </div>
        <div class="hero-content">
          <p><span></span>{text("dashboard")}</p>
          <h1 id="hero-title">{data ? title(data.name) : text("launch")}</h1>
          <div>{text("description")}</div>
          <a class="hero-link" href="#api">{text("view_api")}</a>
        </div>
      </section>
      <div class="metrics" aria-label={text("overview")}>
        <div class="metric card"><div><small>{text("resources")}</small><strong>{data?.collections.length ?? "—"}</strong></div></div>
        <div class="metric card"><div><small>{text("contract")}</small><strong>{data ? "v" + data.version : "—"}</strong></div></div>
        <div class="metric card"><div><small>{data?.storage ?? text("storage")}</small><strong>{text(ready === undefined ? "checking" : ready ? "storage_ready" : "storage_offline")}</strong></div></div>
      </div>
      {#if selected}
        {#key selected.path}<Workbench collection={selected} />{/key}
      {/if}
    {/if}
  </main>
</div>
