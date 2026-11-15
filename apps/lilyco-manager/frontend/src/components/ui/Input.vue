<script setup lang="ts">
withDefaults(defineProps<{modelValue?:string|number;id?:string;placeholder?:string;type?:string;disabled?:boolean;autocomplete?:string}>(),{type:'text'})
const emit=defineEmits<{(e:'update:modelValue',value:string):void}>()
function onInput(event:Event){emit('update:modelValue',(event.target as HTMLInputElement).value)}
</script>
<template>
 <input :id="id" :value="modelValue" :type="type" :placeholder="placeholder" :disabled="disabled" :autocomplete="autocomplete"
  class="flex h-9 w-full rounded-md border border-input bg-card px-3 py-1 text-[13px] text-foreground shadow-none transition-colors placeholder:text-muted-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:bg-disabled-surface disabled:text-disabled-ink"
  @input="onInput">
</template>
