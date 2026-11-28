<script lang="ts">
  import { onMount, tick } from "svelte";
  import { list, remove, save, title, type Collection, type Fields, type Row } from "@/lib/api";
  import { text } from "@/strings";

  let { collection }: { collection: Collection } = $props();
  type Action = { kind: "create" } | { kind: "edit" | "delete"; row: Row };

  let rows = $state<Row[]>([]);
  let next = $state<number | null>(null);
  let filter = $state("");
  let error = $state("");
  let loading = $state(false);
  let busy = $state(false);
  let draftError = $state("");
  let draft = $state<Fields>({});
  let action = $state<Action>();
  let dialog = $state<HTMLDialogElement>();
  const shown = $derived(rows.filter((row) => Object.values(row.data).some((value) =>
    String(value).toLowerCase().includes(filter.trim().toLowerCase()))));

  function message(error: unknown) {
    return error instanceof Error ? error.message : text("network_error");
  }

  async function load(after?: number) {
    if (loading) return;
    loading = true;
    error = "";
    try {
      const page = await list(collection.path, after);
      rows = after === undefined ? page.items : [...rows, ...page.items];
      next = page.next;
    } catch (failure) { error = message(failure); }
    finally { loading = false; }
  }

  async function edit(change: Action) {
    action = change;
    draftError = "";
    draft = change.kind === "create"
      ? Object.fromEntries(collection.fields.map(({ name, kind }) => [name, kind === "boolean" ? false : kind === "integer" ? 0 : ""]))
      : { ...change.row.data };
    await tick();
    dialog?.showModal();
  }

  function close() {
    dialog?.close();
    action = undefined;
  }

  async function apply(event: SubmitEvent) {
    event.preventDefault();
    const change = action;
    if (busy || !change) return;
    busy = true;
    draftError = "";
    try {
      if (change.kind === "delete") await remove(collection.path, change.row.id);
      else await save(collection.path, draft, change.kind === "edit" ? change.row.id : undefined);
      close();
      await load();
    } catch (failure) { draftError = message(failure); }
    finally { busy = false; }
  }

  function cancel(event: Event) {
    event.preventDefault();
    if (!busy) close();
  }

  onMount(() => { void load(); });
</script>

<section class="collection card" aria-labelledby="collection-title">
  <div class="collection-head">
    <div class="collection-title"><div><h2 id="collection-title">{title(collection.name)}</h2><code>{collection.path}</code></div></div>
    <button class="rapid-button" onclick={() => edit({ kind: "create" })}>{text("new")}</button>
  </div>
  <div class="toolbar">
    <label class="search"><input bind:value={filter} type="search" placeholder={text("search")} aria-label={text("search")} /></label>
    <span class="muted">{rows.length} {text("loaded")}</span>
  </div>
  {#if error}
    <div class="inline-error" role="alert">{error} <button class="rapid-button secondary" onclick={() => load()}>{text("retry")}</button></div>
  {:else if loading && rows.length === 0}
    <p class="empty" role="status">{text("loading")}</p>
  {:else if shown.length === 0}
    <div class="empty"><strong>{text(filter ? "no_matches" : "no_records")}</strong>{#if !filter}<span>{text("empty_help")}</span>{/if}</div>
  {/if}
  {#if shown.length}
    <div class="record-list">
      {#each shown as row (row.id)}
        <article class="record">
          <div class="record-values">
            {#each collection.fields as field (field.name)}
              <div><small>{title(field.name)}</small><strong>{field.kind === "boolean" ? text(row.data[field.name] ? "yes" : "no") : row.data[field.name]}</strong></div>
            {/each}
          </div>
          <div class="record-actions">
            <button class="rapid-button ghost sm" aria-label={text("edit") + " " + row.id} onclick={() => edit({ kind: "edit", row })}>{text("edit")}</button>
            <button class="rapid-button ghost sm" aria-label={text("delete") + " " + row.id} onclick={() => edit({ kind: "delete", row })}>{text("delete")}</button>
          </div>
        </article>
      {/each}
    </div>
  {/if}
  {#if next !== null}<div class="collection-foot"><button class="rapid-button secondary" disabled={loading} onclick={() => load(next ?? undefined)}>{text("load_more")}</button></div>{/if}
  {#if action}
    <dialog bind:this={dialog} class="editor card" aria-labelledby="editor-title" oncancel={cancel}>
      <form onsubmit={apply}>
        <div class="editor-header"><h2 id="editor-title">{text(action.kind === "delete" ? "delete_heading" : action.kind === "edit" ? "edit" : "new")}</h2></div>
        {#if action.kind === "delete"}
          <p class="muted">{text("delete_description")}</p>
        {:else}
          {#each collection.fields as field (field.name)}
            <label class="field" class:checkbox={field.kind === "boolean"} for={"edit-" + field.name}>
              {#if field.kind === "boolean"}
                <input id={"edit-" + field.name} type="checkbox" checked={Boolean(draft[field.name])} onchange={(event) => draft[field.name] = event.currentTarget.checked} />
              {/if}
              <span>{title(field.name)}</span>
              {#if field.kind === "integer"}
                <input id={"edit-" + field.name} type="number" required value={draft[field.name]} oninput={(event) => draft[field.name] = event.currentTarget.valueAsNumber} />
              {:else if field.kind === "string"}
                <input id={"edit-" + field.name} type="text" required value={draft[field.name]} oninput={(event) => draft[field.name] = event.currentTarget.value} />
              {/if}
            </label>
          {/each}
        {/if}
        {#if draftError}<p class="inline-error" role="alert">{draftError}</p>{/if}
        <div class="editor-footer">
          <button type="button" class="rapid-button secondary" disabled={busy} onclick={close}>{text("cancel")}</button>
          <button type="submit" class="rapid-button" class:destructive={action.kind === "delete"} disabled={busy}>{text(action.kind === "delete" ? "delete" : "save")}</button>
        </div>
      </form>
    </dialog>
  {/if}
</section>
