<script setup lang="ts">
import {computed,onMounted,ref} from 'vue'
import {Copy,FileJson,RefreshCw,ShieldCheck} from 'lucide-vue-next'
import Alert from './components/ui/Alert.vue'
import Badge from './components/ui/Badge.vue'
import Button from './components/ui/Button.vue'
import Card from './components/ui/Card.vue'
import CardContent from './components/ui/CardContent.vue'
import CardDescription from './components/ui/CardDescription.vue'
import CardHeader from './components/ui/CardHeader.vue'
import CardTitle from './components/ui/CardTitle.vue'

type Operation = { summary?: string; tags?: string[] }
type Document = {
  openapi: string
  info: { title: string; version: string; description?: string }
  paths: Record<string, Record<string, Operation>>
}

const props = defineProps<{ token: string }>()

const doc = ref<Document | null>(null)
const loading = ref(false)
const error = ref('')
const notice = ref('')

const tagNames: Record<string, string> = {
  tools: '工具',
  meta: '元数据',
  apps: '应用',
  release: '发布',
  dev: '开发任务',
  mcp: 'MCP',
}
const tagOrder = ['tools', 'apps', 'release', 'dev', 'mcp', 'meta']

const groups = computed(() => {
  const buckets = new Map<string, { method: string; path: string; summary: string }[]>()
  for (const [path, item] of Object.entries(doc.value?.paths ?? {})) {
    for (const [method, operation] of Object.entries(item)) {
      const tag = operation.tags?.[0] ?? 'meta'
      const list = buckets.get(tag) ?? []
      list.push({ method: method.toUpperCase(), path, summary: operation.summary ?? '' })
      buckets.set(tag, list)
    }
  }
  return [...buckets.entries()]
    .sort((a, b) => {
      const left = tagOrder.indexOf(a[0])
      const right = tagOrder.indexOf(b[0])
      return (left === -1 ? 99 : left) - (right === -1 ? 99 : right)
    })
    .map(([tag, operations]) => ({
      tag,
      name: tagNames[tag] ?? tag,
      operations: operations.sort((a, b) => a.path.localeCompare(b.path)),
    }))
})

const operationCount = computed(() =>
  groups.value.reduce((total, group) => total + group.operations.length, 0),
)

function methodClass(method: string) {
  if (method === 'GET') return 'bg-info-soft text-info'
  if (method === 'POST') return 'bg-success-soft text-success'
  return 'bg-muted text-muted-foreground'
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    const response = await fetch('/api/openapi.json', { headers: { 'x-lilyco-token': props.token } })
    if (!response.ok) throw new Error('HTTP ' + response.status)
    doc.value = (await response.json()) as Document
  } catch (cause) {
    error.value = '读取接口清单失败：' + String(cause)
  } finally {
    loading.value = false
  }
}

async function copyJson() {
  if (!doc.value) return
  try {
    await navigator.clipboard.writeText(JSON.stringify(doc.value, null, 2))
    notice.value = 'OpenAPI JSON 已复制到剪贴板'
  } catch {
    notice.value = '浏览器拒绝剪贴板访问，请从接口返回处获取 JSON'
  }
}

onMounted(load)
</script>

<template>
  <div>
    <div class="mb-5 flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-lg font-semibold">接口文档</h2>
        <p class="mt-1 text-xs text-ink-soft">
          来自 <code>GET /api/openapi.json</code> 的同一份路由清单，和契约测试共用一份来源。所有接口都要求
          <code>x-lilyco-token</code>，状态变更接口额外校验同源。
        </p>
      </div>
      <div class="flex items-center gap-2">
        <Badge variant="brand"><FileJson :size="12"/>{{ doc?.openapi ?? 'OpenAPI' }}</Badge>
        <Badge v-if="doc">{{ operationCount }} 个操作</Badge>
        <Badge variant="success"><ShieldCheck :size="12"/>需 token</Badge>
      </div>
    </div>

    <Card>
      <CardHeader class="flex-row flex-wrap items-center justify-between gap-3">
        <div>
          <CardTitle>{{ doc?.info.title ?? 'Lilyco Manager API' }}</CardTitle>
          <CardDescription>
            <template v-if="doc">v{{ doc.info.version }} · {{ doc.info.description }}</template>
            <template v-else>尚未加载</template>
          </CardDescription>
        </div>
        <div class="flex gap-2">
          <Button size="sm" variant="secondary" @click="load" :disabled="loading">
            <RefreshCw :size="13" :class="loading ? 'animate-spin' : ''"/>刷新
          </Button>
          <Button size="sm" variant="outline" @click="copyJson" :disabled="!doc">
            <Copy :size="13"/>复制 JSON
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <Alert v-if="error" variant="danger" title="读取失败">{{ error }}</Alert>
        <p v-else-if="notice" class="mb-3 text-xs text-muted-foreground" aria-live="polite">{{ notice }}</p>

        <div v-for="group in groups" :key="group.tag" class="mb-6 last:mb-0">
          <h3 class="mb-2 flex items-center gap-2 text-sm font-semibold">
            {{ group.name }}
            <span class="text-[11px] font-normal text-ink-mute">{{ group.operations.length }} 个操作</span>
          </h3>
          <div class="overflow-hidden rounded-lg border border-border">
            <div
              v-for="operation in group.operations"
              :key="`${operation.method}-${operation.path}`"
              class="flex flex-wrap items-center gap-3 border-b border-border/60 px-3 py-2.5 text-[13px] last:border-0"
              data-testid="api-operation"
            >
              <span :class="['inline-flex w-14 justify-center rounded-pill px-2 py-0.5 text-[11px] font-semibold', methodClass(operation.method)]">
                {{ operation.method }}
              </span>
              <code class="font-mono text-xs">{{ operation.path }}</code>
              <span class="ml-auto text-xs text-ink-soft">{{ operation.summary }}</span>
            </div>
          </div>
        </div>

        <p v-if="!error && !doc && loading" class="text-xs text-muted-foreground">正在读取接口清单…</p>
      </CardContent>
    </Card>
  </div>
</template>
