<script setup lang="ts">
import {computed} from 'vue'
import type {BlockSpec, Tone} from '../blocks/catalog'
import Badge from './ui/Badge.vue'
import Button from './ui/Button.vue'
import Card from './ui/Card.vue'
import Input from './ui/Input.vue'
import Switch from './ui/Switch.vue'

const props = defineProps<{ spec: BlockSpec }>()
const p = computed(() => props.spec.props)
const text = (key: string) => String(p.value[key] ?? '')
const toneOf = computed(() => String(p.value.tone ?? 'neutral') as Tone)
const badgeVariant = computed(() => (toneOf.value === 'neutral' ? 'default' : toneOf.value))
const frame = computed(() => ({
  neutral: 'border-border bg-card',
  brand: 'border-primary/30 bg-accent/50',
  warning: 'border-warning/40 bg-warning-soft/50',
  danger: 'border-danger/40 bg-danger-soft/50',
}[toneOf.value]))
</script>

<template>
 <div class="w-full" data-testid="block-preview">
  <div v-if="spec.kind === 'hero'" :class="['rounded-lg border p-6', frame]">
   <Badge :variant="badgeVariant">{{ text('badge') || 'PREVIEW' }}</Badge>
   <h3 class="mt-4 text-2xl font-semibold">{{ text('title') }}</h3>
   <p class="mt-2 max-w-xl text-sm text-muted-foreground">{{ text('description') }}</p>
  </div>

  <Card v-else-if="spec.kind === 'stats'" class="p-5">
   <div class="flex items-center justify-between text-xs text-muted-foreground">
    <span>{{ text('label') }}</span><Badge :variant="badgeVariant">实时</Badge>
   </div>
   <p class="mt-4 text-3xl font-semibold">{{ text('value') }}</p>
   <p class="mt-1 text-xs text-muted-foreground">{{ text('hint') }}</p>
  </Card>

  <Card v-else-if="spec.kind === 'app-card'" :class="['p-5', frame]">
   <div class="flex flex-wrap items-start justify-between gap-3">
    <div>
     <div class="flex items-center gap-2">
      <h3 class="text-lg font-semibold">{{ text('name') }}</h3>
      <Badge variant="secondary">{{ text('version') }}</Badge>
     </div>
     <p class="mt-2 text-xs text-muted-foreground">{{ text('description') }}</p>
    </div>
    <Badge :variant="p.running ? 'success' : 'warning'">{{ p.running ? '运行中' : '已停止' }}</Badge>
   </div>
   <div class="mt-4 flex items-center gap-2">
    <Button size="sm">{{ p.running ? '停止' : '启动' }}</Button>
    <Button size="sm" variant="secondary">日志</Button>
   </div>
  </Card>

  <Card v-else-if="spec.kind === 'logs'" class="p-5">
   <div class="flex items-center justify-between gap-2">
    <h3 class="text-sm font-semibold">{{ text('title') }}</h3>
    <Switch :model-value="true" label="自动滚动" disabled/>
   </div>
   <pre class="mt-3 max-h-40 overflow-auto rounded-md border border-border bg-muted p-3 font-mono text-[12px] text-muted-foreground">{{ text('lines') }}</pre>
  </Card>

  <Card v-else-if="spec.kind === 'form'" :class="['p-5', frame]">
   <h3 class="text-lg font-semibold">{{ text('title') }}</h3>
   <div class="mt-4 flex flex-col gap-1.5">
    <label class="text-xs font-semibold">{{ text('label') }}</label>
    <Input :placeholder="text('placeholder')"/>
   </div>
   <Button class="mt-3" size="sm">{{ text('button') }}</Button>
  </Card>

  <div v-else :class="['rounded-lg border border-dashed p-8 text-center', frame]">
   <h3 class="text-lg font-semibold">{{ text('title') }}</h3>
   <p class="mx-auto mt-2 max-w-sm text-sm text-muted-foreground">{{ text('description') }}</p>
   <Button class="mt-4" size="sm" variant="secondary">{{ text('action') }}</Button>
  </div>
 </div>
</template>
