<script setup lang="ts">
import {computed} from 'vue'
import {cn} from '../../lib/utils'
type Variant='default'|'secondary'|'outline'|'brand'|'success'|'warning'|'danger'
const props=withDefaults(defineProps<{variant?:Variant}>(),{variant:'default'})
const variants:Record<Variant,string>={
 default:'bg-muted text-muted-foreground',
 secondary:'border border-border bg-card text-card-foreground',
 outline:'border border-input text-foreground',
 brand:'bg-accent text-primary',
 success:'bg-success-soft text-success',
 warning:'bg-warning-soft text-warning',
 danger:'bg-danger-soft text-danger',
}
const classes=computed(()=>cn('inline-flex items-center gap-1 rounded-pill px-2 py-0.5 text-[11px] font-semibold',variants[props.variant]))
</script>
<template>
 <span :class="classes"><slot/></span>
</template>
