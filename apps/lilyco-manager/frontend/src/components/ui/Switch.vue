<script setup lang="ts">
const props=withDefaults(defineProps<{modelValue?:boolean;id?:string;label?:string;disabled?:boolean}>(),{modelValue:false})
const emit=defineEmits<{(e:'update:modelValue',value:boolean):void}>()
function toggle(){if(!props.disabled)emit('update:modelValue',!props.modelValue)}
</script>
<template>
 <button :id="id" type="button" role="switch" :aria-checked="modelValue" :aria-label="label" :disabled="disabled"
  :class="['relative inline-flex h-5 w-9 shrink-0 items-center rounded-pill border border-transparent transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring', modelValue ? 'bg-primary' : 'bg-input', disabled ? 'opacity-50' : '']"
  @click="toggle">
  <span :class="['pointer-events-none block h-4 w-4 rounded-pill bg-card shadow-sm transition-transform', modelValue ? 'translate-x-[18px]' : 'translate-x-0.5']"/>
 </button>
</template>
