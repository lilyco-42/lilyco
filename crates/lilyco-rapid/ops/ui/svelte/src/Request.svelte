<script lang="ts">
  import { exampleFor, title, tryOperation, type Operation } from "@/lib/api";
  import { text } from "@/strings";

  let { operation }: { operation: Operation } = $props();
  let inputs = $state<Record<string, string>>({});
  let draft = $state<string>();
  let response = $state("");
  let busy = $state(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    busy = true;
    response = "";
    try { response = await tryOperation(operation, inputs, draft ?? exampleFor(operation.body)); }
    catch (error) { response = error instanceof Error ? error.message : text("network_error"); }
    finally { busy = false; }
  }
</script>

<form class="try-form" onsubmit={submit}>
  {#each operation.params as parameter (parameter.location + ":" + parameter.name)}
    {@const name = parameter.location + ":" + parameter.name}
    <label class="field">
      <span>{parameter.name === "id" ? text("id") : title(parameter.name)} <small>{text(parameter.location === "path" ? "path_parameter" : "query_parameter")}</small></span>
      <input bind:value={inputs[name]} required={parameter.required} inputmode={parameter.kind === "integer" ? "numeric" : "text"} />
    </label>
  {/each}
  {#if operation.body !== undefined}
    <label class="field"><span>{text("try_json")}</span>
      <textarea rows="4" required value={draft ?? exampleFor(operation.body)} oninput={(event) => draft = event.currentTarget.value}></textarea>
    </label>
  {/if}
  <button class="rapid-button secondary" type="submit" disabled={busy}>{text(busy ? "loading" : "try_send")}</button>
  {#if response}<pre role="status">{response}</pre>{/if}
</form>
