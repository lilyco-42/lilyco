<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { ArrowDown, ClipboardList, Pencil, Plus, Search, Trash2 } from "@lucide/vue";
import { list, remove, save, title, type Collection, type Fields, type Row } from "@/lib/api";
import { text } from "@/strings";

const props = defineProps<{ collection: Collection }>();
type Action = { kind: "create" } | { kind: "edit" | "delete"; row: Row };

const rows = ref<Row[]>([]);
const next = ref<number | null>(null);
const filter = ref("");
const error = ref("");
const loading = ref(false);
const busy = ref(false);
const draftError = ref("");
const draft = ref<Fields>({});
const action = ref<Action>();
const dialog = ref<HTMLDialogElement>();
const shown = computed(() => rows.value.filter((row) => Object.values(row.data).some((value) =>
  String(value).toLowerCase().includes(filter.value.trim().toLowerCase()))));

async function load(after?: number) {
  if (loading.value) return;
  loading.value = true;
  error.value = "";
  try {
    const page = await list(props.collection.path, after);
    rows.value = after === undefined ? page.items : [...rows.value, ...page.items];
    next.value = page.next;
  } catch (failure) {
    error.value = message(failure);
  } finally {
    loading.value = false;
  }
}

function message(error: unknown) {
  return error instanceof Error ? error.message : text("network_error");
}

async function edit(change: Action) {
  action.value = change;
  draftError.value = "";
  draft.value = change.kind === "create"
    ? Object.fromEntries(props.collection.fields.map(({ name, kind }) => [name, kind === "boolean" ? false : kind === "integer" ? 0 : ""]))
    : { ...change.row.data };
  await nextTick();
  dialog.value?.showModal();
}

function close() {
  dialog.value?.close();
  action.value = undefined;
}

async function apply() {
  const change = action.value;
  if (busy.value || !change) return;
  busy.value = true;
  draftError.value = "";
  try {
    const path = props.collection.path;
    if (change.kind === "delete") await remove(path, change.row.id);
    else await save(path, draft.value, change.kind === "edit" ? change.row.id : undefined);
    close();
    await load();
  } catch (failure) {
    draftError.value = message(failure);
  } finally {
    busy.value = false;
  }
}

onMounted(() => void load());
</script>

<template>
  <section class="collection card" aria-labelledby="collection-title">
    <div class="collection-head">
      <div class="collection-title"><span class="collection-icon"><ClipboardList :size="20" /></span><div><h2 id="collection-title">{{ title(collection.name) }}</h2><code>{{ collection.path }}</code></div></div>
      <button class="rapid-button" @click="edit({ kind: 'create' })"><Plus :size="16" />{{ text("new") }}</button>
    </div>
    <div class="toolbar">
      <label class="search"><Search :size="16" /><input v-model="filter" type="search" :placeholder="text('search')" :aria-label="text('search')" /></label>
      <span class="muted">{{ rows.length }} {{ text("loaded") }}</span>
    </div>
    <div v-if="error" class="inline-error" role="alert">{{ error }} <button class="rapid-button secondary" @click="load()">{{ text("retry") }}</button></div>
    <p v-else-if="loading && rows.length === 0" class="empty" role="status">{{ text("loading") }}</p>
    <div v-else-if="shown.length === 0" class="empty"><ClipboardList :size="28" /><strong>{{ text(filter ? "no_matches" : "no_records") }}</strong><span v-if="!filter">{{ text("empty_help") }}</span></div>
    <div v-if="shown.length" class="record-list">
      <article v-for="row in shown" :key="row.id" class="record">
        <div class="record-values"><div v-for="field in collection.fields" :key="field.name"><small>{{ title(field.name) }}</small><strong>{{ field.kind === "boolean" ? text(row.data[field.name] ? "yes" : "no") : row.data[field.name] }}</strong></div></div>
        <div class="record-actions">
          <button class="rapid-button ghost icon" :aria-label="text('edit') + ' ' + row.id" @click="edit({ kind: 'edit', row })"><Pencil :size="16" /></button>
          <button class="rapid-button ghost icon" :aria-label="text('delete') + ' ' + row.id" @click="edit({ kind: 'delete', row })"><Trash2 :size="16" /></button>
        </div>
      </article>
    </div>
    <div v-if="next !== null" class="collection-foot"><button class="rapid-button secondary" :disabled="loading" @click="load(next ?? undefined)"><ArrowDown :size="15" />{{ text("load_more") }}</button></div>
    <dialog v-if="action" ref="dialog" class="editor card" aria-labelledby="editor-title" @cancel.prevent="!busy && close()">
      <form @submit.prevent="apply">
        <div class="editor-header"><h2 id="editor-title">{{ text(action.kind === "delete" ? "delete_heading" : action.kind === "edit" ? "edit" : "new") }}</h2></div>
        <p v-if="action.kind === 'delete'" class="muted">{{ text("delete_description") }}</p>
        <template v-else>
          <label v-for="field in collection.fields" :key="field.name" class="field" :class="{ checkbox: field.kind === 'boolean' }">
            <input v-if="field.kind === 'boolean'" v-model="draft[field.name]" type="checkbox" />
            <span>{{ title(field.name) }}</span>
            <input v-if="field.kind === 'integer'" v-model.number="draft[field.name]" type="number" required />
            <input v-if="field.kind === 'string'" v-model="draft[field.name]" type="text" required />
          </label>
        </template>
        <p v-if="draftError" class="inline-error" role="alert">{{ draftError }}</p>
        <div class="editor-footer">
          <button type="button" class="rapid-button secondary" :disabled="busy" @click="close">{{ text("cancel") }}</button>
          <button type="submit" class="rapid-button" :class="{ destructive: action.kind === 'delete' }" :disabled="busy">{{ text(action.kind === "delete" ? "delete" : "save") }}</button>
        </div>
      </form>
    </dialog>
  </section>
</template>
