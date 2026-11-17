<script setup lang="ts">
import {computed,onMounted,ref} from 'vue'
import {Copy,FileJson,Play,RefreshCw,ShieldCheck} from 'lucide-vue-next'
import Alert from './components/ui/Alert.vue'
import Badge from './components/ui/Badge.vue'
import Button from './components/ui/Button.vue'
import Card from './components/ui/Card.vue'
import CardContent from './components/ui/CardContent.vue'
import CardDescription from './components/ui/CardDescription.vue'
import CardHeader from './components/ui/CardHeader.vue'
import CardTitle from './components/ui/CardTitle.vue'
import Input from './components/ui/Input.vue'
import Label from './components/ui/Label.vue'

type Operation = { summary?: string; tags?: string[] }
type Document = {
  openapi: string
  info: { title: string; version: string; description?: string }
  paths: Record<string, Record<string, Operation>>
}
type Result = { status: number; ms: number; body: string }

const props = defineProps<{ token: string }>()

const doc = ref<Document | null>(null)
const loading = ref(false)
const error = ref('')
const notice = ref('')

const tagNames: Record<string, string> = {
  tools: '工具',
  meta: '元数据',
  apps: '应用',
  projects: '项目',
  release: '发布',
  dev: '开发任务',
  mcp: 'MCP',
}
const tagOrder = ['tools', 'apps', 'projects', 'release', 'dev', 'mcp', 'meta']

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

// 只有 GET 可以在文档页「试调用」：写操作留在各自面板里执行，
// 避免文档页成为绕过确认的一键改机入口。
const activeKey = ref('')
const params = ref<Record<string, string>>({})
const result = ref<Result | null>(null)
const sending = ref(false)
const tryError = ref('')

const defaultParams: Record<string, string> = { id: 'demo', job: 'none' }

function pathParams(path: string) {
  return [...path.matchAll(/\{(\w+)\}/g)].map(match => match[1])
}

function keyOf(method: string, path: string) {
  return `${method} ${path}`
}

function openTry(method: string, path: string) {
  const key = keyOf(method, path)
  if (activeKey.value === key) {
    activeKey.value = ''
    return
  }
  activeKey.value = key
  params.value = Object.fromEntries(pathParams(path).map(name => [name, defaultParams[name] ?? '']))
  result.value = null
  tryError.value = ''
}

function resolved(path: string) {
  return pathParams(path).reduce(
    (url, name) => url.replace(`{${name}}`, encodeURIComponent(params.value[name] ?? '')),
    path,
  )
}

async function send(path: string) {
  const url = resolved(path)
  if (pathParams(path).some(name => !(params.value[name] ?? '').trim())) {
    tryError.value = '请先填写所有路径参数'
    return
  }
  sending.value = true
  tryError.value = ''
  result.value = null
  const started = performance.now()
  try {
    const response = await fetch(url, { headers: { 'x-lilyco-token': props.token } })
    const text = await response.text()
    let body = text
    try {
      body = JSON.stringify(JSON.parse(text), null, 2)
    } catch {
      // 非 JSON 就原样展示（例如纯文本错误页）。
    }
    result.value = {
      status: response.status,
      ms: Math.round(performance.now() - started),
      body: body.length > 4000 ? `${body.slice(0, 4000)}\n…（已截断，共 ${body.length} 字符）` : body,
    }
  } catch (cause) {
    tryError.value = '请求失败：' + String(cause)
  } finally {
    sending.value = false
  }
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    const response = await fetch('/api/openapi.json', { headers: { 'x-lilyco-token': props.token } })
    if (!response.ok) throw new Error('HTTP ' + response.status)
    doc.value = (await response.json()) as Document
    activeKey.value = ''
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
          <code>x-lilyco-token</code>，状态变更接口额外校验同源；GET 可直接试调用，写操作留在各自面板执行。
        </p>
      </div>
      <div class="flex items-center gap-2">
        <Badge variant="brand"><FileJson :size="12"/>{{ doc?.openapi ?? 'OpenAPI' }}</Badge>
        <Badge v-if="doc">{{ operationCount }} 个操作</Badge>
        <Badge variant="success"><ShieldCheck :size="12"/>需 token</Badge>
      </div>
    </div>

    <Card data-testid="api-doc-card">
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
              class="border-b border-border/60 last:border-0"
            >
              <div
                class="flex flex-wrap items-center gap-3 px-3 py-2.5 text-[13px]"
                data-testid="api-operation"
              >
                <span :class="['inline-flex w-14 justify-center rounded-pill px-2 py-0.5 text-[11px] font-semibold', methodClass(operation.method)]">
                  {{ operation.method }}
                </span>
                <code class="font-mono text-xs">{{ operation.path }}</code>
                <span class="ml-auto text-xs text-ink-soft">{{ operation.summary }}</span>
                <Button
                  v-if="operation.method === 'GET'"
                  size="sm"
                  variant="ghost"
                  :aria-label="`试调用 ${operation.path}`"
                  @click="openTry(operation.method, operation.path)"
                >
                  <Play :size="12"/>试调用
                </Button>
              </div>

              <div
                v-if="activeKey === keyOf(operation.method, operation.path)"
                class="border-t border-border/60 bg-muted/40 px-3 py-3"
                data-testid="api-try"
              >
                <div v-if="pathParams(operation.path).length" class="mb-3 flex flex-wrap gap-3">
                  <div
                    v-for="name in pathParams(operation.path)"
                    :key="name"
                    class="flex min-w-[140px] flex-1 flex-col gap-1"
                  >
                    <Label :for="`try-${name}`">{{ name }}</Label>
                    <Input :id="`try-${name}`" v-model="params[name]" :placeholder="defaultParams[name] ?? ''"/>
                  </div>
                </div>
                <div class="flex flex-wrap items-center gap-2">
                  <Button size="sm" :disabled="sending" @click="send(operation.path)">
                    <Play :size="13"/>{{ sending ? '请求中…' : '发送 GET' }}
                  </Button>
                  <code class="text-[11px] text-ink-mute">{{ resolved(operation.path) }}</code>
                </div>

                <Alert v-if="tryError" variant="danger" title="请求失败" class="mt-3">{{ tryError }}</Alert>
                <div v-else-if="result" class="mt-3" data-testid="api-result">
                  <div class="mb-2 flex flex-wrap items-center gap-2 text-xs">
                    <Badge :variant="result.status < 400 ? 'success' : 'danger'">HTTP {{ result.status }}</Badge>
                    <span class="text-ink-mute">{{ result.ms }} ms</span>
                  </div>
                  <pre class="max-h-72 overflow-auto rounded-md bg-canvas-soft p-3 font-mono text-[11px] leading-relaxed">{{ result.body }}</pre>
                </div>
              </div>
            </div>
          </div>
        </div>

        <p v-if="!error && !doc && loading" class="text-xs text-muted-foreground">正在读取接口清单…</p>
      </CardContent>
    </Card>
  </div>
</template>
