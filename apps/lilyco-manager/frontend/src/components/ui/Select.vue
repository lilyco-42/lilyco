<script setup lang="ts">
defineProps<{modelValue?:string;id?:string;disabled?:boolean}>()
const emit=defineEmits<{(e:'update:modelValue',value:string):void}>()
function onChange(event:Event){emit('update:modelValue',(event.target as HTMLSelectElement).value)}
</script>
<template>
 <select :id="id" :value="modelValue" :disabled="disabled"
  class="flex h-9 w-full rounded-md border border-input bg-card px-3 py-1 text-[13px] text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:bg-disabled-surface disabled:text-disabled-ink"
  @change="onChange"><slot/></select>
</template>
