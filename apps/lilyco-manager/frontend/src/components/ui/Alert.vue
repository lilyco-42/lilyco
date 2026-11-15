<script setup lang="ts">
import {computed} from 'vue'
import {cn} from '../../lib/utils'
type Variant='default'|'success'|'warning'|'danger'
const props=withDefaults(defineProps<{variant?:Variant;title?:string}>(),{variant:'default'})
const variants:Record<Variant,string>={
 default:'border-border bg-muted text-muted-foreground',
 success:'border-success/35 bg-success-soft text-success',
 warning:'border-warning/35 bg-warning-soft text-warning',
 danger:'border-danger/35 bg-danger-soft text-danger',
}
const classes=computed(()=>cn('rounded-md border px-3.5 py-3 text-[13px]',variants[props.variant]))
</script>
<template>
 <div :class="classes" role="status">
  <p v-if="title" class="font-semibold">{{title}}</p>
  <div :class="title ? 'mt-1' : ''"><slot/></div>
 </div>
</template>
