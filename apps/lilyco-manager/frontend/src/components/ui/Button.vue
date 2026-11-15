<script setup lang="ts">
import {computed} from 'vue'
import {cn} from '../../lib/utils'
type Variant='default'|'secondary'|'outline'|'ghost'|'destructive'
type Size='default'|'sm'|'icon'
const props=withDefaults(defineProps<{variant?:Variant;size?:Size;type?:'button'|'submit'|'reset';disabled?:boolean}>(),{variant:'default',size:'default',type:'button'})
const base='inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-[13px] font-semibold transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:pointer-events-none disabled:bg-disabled-surface disabled:text-disabled-ink'
const variants:Record<Variant,string>={
 default:'bg-primary text-primary-foreground hover:opacity-90',
 secondary:'border border-border bg-card text-card-foreground hover:bg-muted',
 outline:'border border-input bg-transparent text-foreground hover:bg-muted',
 ghost:'text-foreground hover:bg-muted',
 destructive:'bg-destructive text-destructive-foreground hover:opacity-90',
}
const sizes:Record<Size,string>={default:'h-9 px-3',sm:'h-[30px] px-2.5 text-xs',icon:'h-9 w-9 p-0'}
const classes=computed(()=>cn(base,variants[props.variant],sizes[props.size]))
</script>
<template>
 <button :type="type" :disabled="disabled" :class="classes"><slot/></button>
</template>
