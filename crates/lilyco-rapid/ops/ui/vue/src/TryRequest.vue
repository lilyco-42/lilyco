<script setup lang="ts">
import { ref } from "vue";
import { exampleFor, title, tryOperation, type Operation } from "@/lib/api";
import { text } from "@/strings";

const props = defineProps<{ operation: Operation }>();
const values = ref<Record<string, string>>({});
const example = ref(exampleFor(props.operation.body));
const response = ref("");
const busy = ref(false);

async function send() {
  if (busy.value) return;
  busy.value = true;
  response.value = "";
  try { response.value = await tryOperation(props.operation, values.value, example.value); }
  catch (error) { response.value = error instanceof Error ? error.message : text("network_error"); }
  finally { busy.value = false; }
}
</script>

<template>
  <form class="try-form" @submit.prevent="send">
    <label v-for="parameter in operation.params" :key="parameter.location + ':' + parameter.name" class="field">
      <span>{{ parameter.name === "id" ? text("id") : title(parameter.name) }} <small>{{ text(parameter.location === "path" ? "path_parameter" : "query_parameter") }}</small></span>
      <input v-model="values[parameter.location + ':' + parameter.name]" :required="parameter.required" :inputmode="parameter.kind === 'integer' ? 'numeric' : 'text'" />
    </label>
    <label v-if="operation.body !== undefined" class="field"><span>{{ text("try_json") }}</span><textarea v-model="example" required rows="4" /></label>
    <button type="submit" class="rapid-button secondary" :disabled="busy">{{ text(busy ? "loading" : "try_send") }}</button>
    <pre v-if="response" role="status">{{ response }}</pre>
  </form>
</template>
