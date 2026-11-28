<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { ArrowRight, Box, Braces, CircleDot, Database, Layers3, LayoutGrid, Orbit } from "@lucide/vue";
import { catalog, databaseReady, title, type Catalog } from "@/lib/api";
import { text } from "@/strings";
import Reference from "./Reference.vue";
import Workbench from "./Workbench.vue";

const data = ref<Catalog>();
const ready = ref<boolean>();
const connection = ref<"checking" | "api_online" | "api_offline">("checking");
const selection = ref("");
const tab = ref<"workspace" | "api">("workspace");
const selected = computed(() => data.value?.collections.find(({ name }) => name === selection.value) ?? data.value?.collections[0]);

function syncTab() {
  tab.value = window.location.hash === "#api" ? "api" : "workspace";
}

async function load() {
  connection.value = "checking";
  const health = databaseReady();
  try {
    data.value = await catalog();
    connection.value = "api_online";
    document.title = data.value.name + " · " + text("studio");
  } catch {
    connection.value = "api_offline";
  } finally {
    ready.value = await health;
  }
}

function select(name: string) {
  selection.value = name;
  window.location.hash = "workspace";
}

onMounted(() => {
  syncTab();
  window.addEventListener("hashchange", syncTab);
  void load();
});
onUnmounted(() => window.removeEventListener("hashchange", syncTab));
</script>

<template>
  <div class="shell">
    <header class="topbar">
      <a class="brand" href="#workspace" :aria-label="text('studio')">
        <span class="brand-mark"><Box :size="20" /></span>
        <strong>{{ text("brand") }}<span>.</span></strong><span class="brand-divider" />
        <span class="brand-product">{{ text("studio") }}</span>
      </a>
      <span class="connection" :class="connection" role="status"><i />{{ text(connection) }}</span>
    </header>

    <aside class="sidebar">
      <div class="project">
        <span class="project-icon"><Layers3 :size="19" /></span>
        <div><strong>{{ data?.name ?? text("studio") }}</strong><span>{{ text("workspace") }}</span></div>
      </div>
      <nav class="primary-nav" :aria-label="text('workspace')">
        <a href="#workspace" :aria-current="tab === 'workspace' ? 'page' : undefined"><LayoutGrid :size="17" />{{ text("workspace") }}</a>
        <a href="#api" :aria-current="tab === 'api' ? 'page' : undefined"><Braces :size="17" />{{ text("api") }}</a>
      </nav>
      <div v-if="data?.collections.length" class="resource-nav">
        <h2>{{ text("resources") }}</h2>
        <button v-for="collection in data.collections" :key="collection.name" :class="{ selected: tab === 'workspace' && selected?.name === collection.name }" @click="select(collection.name)">
          <CircleDot :size="15" />{{ title(collection.name) }}
        </button>
      </div>
      <div class="sidebar-footer"><Orbit :size="18" /><span>{{ text("runtime") }}</span></div>
    </aside>

    <main class="main">
      <p class="breadcrumb">{{ text("workspace") }} <span>/</span> {{ text(tab === "api" ? "api" : "overview") }}</p>
      <div v-if="connection === 'api_offline'" class="inline-error offline" role="alert">
        {{ text("network_error") }} <button class="rapid-button secondary" @click="load">{{ text("retry") }}</button>
      </div>
      <template v-if="tab === 'workspace'">
        <section class="hero" aria-labelledby="hero-title">
          <div class="hero-illustration" aria-hidden="true">
            <div class="orbit orbit-outer" /><div class="orbit orbit-mid" /><div class="orbit orbit-inner" />
            <span class="hero-core"><Box :size="45" :stroke-width="1.15" /></span><span class="satellite" />
          </div>
          <div class="hero-content">
            <p><span />{{ text("dashboard") }}</p>
            <h1 id="hero-title">{{ data ? title(data.name) : text("launch") }}</h1>
            <div>{{ text("description") }}</div>
            <a href="#api" class="hero-link">{{ text("view_api") }} <ArrowRight :size="15" /></a>
          </div>
        </section>
        <div class="metrics" :aria-label="text('overview')">
          <div class="metric card"><span class="metric-icon"><Layers3 :size="17" /></span><div><small>{{ text("resources") }}</small><strong>{{ data?.collections.length ?? "—" }}</strong></div></div>
          <div class="metric card"><span class="metric-icon"><Braces :size="17" /></span><div><small>{{ text("contract") }}</small><strong>{{ data ? "v" + data.version : "—" }}</strong></div></div>
          <div class="metric card"><span class="metric-icon"><Database :size="17" /></span><div><small>{{ data?.storage ?? text("storage") }}</small><strong>{{ text(ready === undefined ? "checking" : ready ? "storage_ready" : "storage_offline") }}</strong></div></div>
        </div>
        <Workbench v-if="selected" :key="selected.path" :collection="selected" />
      </template>
      <Reference v-else :operations="data?.operations ?? []" />
    </main>
  </div>
</template>
