<script setup lang="ts">
withDefaults(defineProps<{modelValue?:string;id?:string;placeholder?:string;rows?:number;disabled?:boolean}>(),{rows:4})
const emit=defineEmits<{(e:'update:modelValue',value:string):void}>()
function onInput(event:Event){emit('update:modelValue',(event.target as HTMLTextAreaElement).value)}
</script>
<template>
 <textarea :id="id" :value="modelValue" :rows="rows" :placeholder="placeholder" :disabled="disabled"
  class="flex w-full rounded-md border border-input bg-card px-3 py-2 text-[13px] text-foreground transition-colors placeholder:text-muted-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:bg-disabled-surface disabled:text-disabled-ink"
  @input="onInput"></textarea>
</template>
