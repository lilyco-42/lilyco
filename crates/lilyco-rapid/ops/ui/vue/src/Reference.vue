<script setup lang="ts">
import { ArrowDownRight, ArrowUpRight, Radio } from "@lucide/vue";
import { text } from "@/strings";
import type { Operation } from "@/lib/api";
import TryRequest from "./TryRequest.vue";

defineProps<{ operations: Operation[] }>();
</script>

<template>
  <section class="reference">
    <div class="reference-heading">
      <div><span class="collection-icon"><Radio :size="20" /></span><h1>{{ text("routes") }}</h1><p>{{ text("route_description") }}</p></div>
      <a class="outline-link" href="/openapi.json" target="_blank" rel="noreferrer">{{ text("schema") }} <ArrowUpRight :size="16" /></a>
    </div>
    <div class="endpoint-list card">
      <details v-for="operation in operations" :key="operation.id">
        <summary><span class="method" :class="operation.method">{{ operation.method.toUpperCase() }}</span><code>{{ operation.path }}</code><ArrowDownRight :size="16" /></summary>
        <template v-if="operation.body !== undefined"><h2>{{ text("request_body") }}</h2><pre>{{ JSON.stringify(operation.body, null, 2) }}</pre></template>
        <TryRequest :operation="operation" />
        <h2>{{ text("response") }}</h2><pre>{{ JSON.stringify(operation.responses, null, 2) }}</pre>
      </details>
    </div>
  </section>
</template>
