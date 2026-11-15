<script setup lang="ts">
import {computed,ref,watch} from 'vue'
import {Blocks,Copy,Download,RefreshCw,Wand2} from 'lucide-vue-next'
import {blocks,createSpec,definitionOf,validateSpec,type BlockKind,type BlockSpec} from './blocks/catalog'
import BlockPreview from './components/BlockPreview.vue'
import PageComposer from './components/PageComposer.vue'
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
import Select from './components/ui/Select.vue'
import Separator from './components/ui/Separator.vue'
import Switch from './components/ui/Switch.vue'
import Textarea from './components/ui/Textarea.vue'

const definitionsCount=blocks.length
const mode=ref<'block'|'page'>('block')
const spec=ref<BlockSpec>(createSpec('hero'))
const definition=computed(()=>definitionOf(spec.value.kind))
const jsonDraft=ref(JSON.stringify(spec.value,null,2))
const error=ref('')
const notice=ref('')

watch(spec,value=>{jsonDraft.value=JSON.stringify(value,null,2)},{deep:true})

function select(kind:BlockKind){
 spec.value=createSpec(kind)
 error.value=''
 notice.value=`已加载「${definitionOf(kind).name}」`
}
function field(key:string){return spec.value.props[key]}
function setField(key:string,value:string|boolean){spec.value.props[key]=value;error.value=''}
function apply(){
 error.value='';notice.value=''
 let parsed:unknown
 try{parsed=JSON.parse(jsonDraft.value)}catch(e){error.value='JSON 解析失败：'+String(e);return}
 const result=validateSpec(parsed)
 if(result.error||!result.spec){error.value=result.error??'校验失败';return}
 spec.value=result.spec
 notice.value=`已应用「${definitionOf(result.spec.kind).name}」`
}
function reset(){spec.value=createSpec(spec.value.kind);error.value='';notice.value='已重置为默认值'}
async function copy(){
 try{
  await navigator.clipboard.writeText(jsonDraft.value)
  notice.value='JSON 已复制到剪贴板'
 }catch{
  notice.value='浏览器拒绝剪贴板访问，请手动复制下方 JSON'
 }
}
function download(){
 const blob=new Blob([jsonDraft.value],{type:'application/json'})
 const url=URL.createObjectURL(blob)
 const link=document.createElement('a')
 link.href=url;link.download=`lilyco-block-${spec.value.kind}.json`;link.click()
 URL.revokeObjectURL(url)
 notice.value='JSON 已导出'
}
</script>

<template>
 <div>
  <div class="mb-5 flex flex-wrap items-start justify-between gap-3">
   <div>
    <h2 class="text-lg font-semibold">设计积木</h2>
    <p class="mt-1 text-xs text-ink-soft">shadcn 风格组件 + 同一份 JSON 规格，右侧改属性、下方改 JSON，预览即时更新。纯本机渲染，不加载第三方脚本。</p>
   </div>
   <div class="flex items-center gap-2">
    <Badge variant="brand"><Wand2 :size="12"/>原型模式</Badge>
    <Badge>{{definitionsCount}} 块积木</Badge>
   </div>
  </div>

  <div class="mb-5 flex flex-wrap items-center gap-2">
   <div class="inline-flex rounded-lg border border-line bg-surface p-1" role="tablist" aria-label="原型模式">
    <button type="button" role="tab" :aria-selected="mode==='block'" @click="mode='block'"
     :class="['rounded-md px-3 py-1.5 text-[13px] font-semibold transition-colors',mode==='block'?'bg-primary text-on-brand':'text-ink-soft hover:bg-surface-hover']">单块调试</button>
    <button type="button" role="tab" :aria-selected="mode==='page'" @click="mode='page'"
     :class="['rounded-md px-3 py-1.5 text-[13px] font-semibold transition-colors',mode==='page'?'bg-primary text-on-brand':'text-ink-soft hover:bg-surface-hover']">页面组合</button>
   </div>
   <p class="text-[11px] text-ink-mute">单块调试适合抠组件；页面组合把积木拼成可导出的原型页。</p>
  </div>

  <PageComposer v-if="mode==='page'"/>

  <template v-else>
  <div class="grid gap-5 xl:grid-cols-[210px_minmax(0,1fr)_280px]">
   <Card>
    <CardHeader><CardTitle class="flex items-center gap-2"><Blocks :size="16"/>积木目录</CardTitle><CardDescription>选择后替换预览</CardDescription></CardHeader>
    <CardContent class="flex flex-col gap-1.5 pt-0">
     <button v-for="block in blocks" :key="block.kind" type="button" :aria-label="`切换到积木：${block.name}`"
      :aria-pressed="spec.kind===block.kind"
      :class="['rounded-md border px-3 py-2 text-left transition-colors',spec.kind===block.kind?'border-primary/40 bg-accent text-primary':'border-transparent hover:bg-muted']"
      @click="select(block.kind)">
      <span class="flex items-center justify-between gap-2 text-[13px] font-semibold">{{block.name}}<span class="text-[10px] font-normal text-muted-foreground">{{block.category}}</span></span>
      <span class="mt-0.5 block text-[11px] text-muted-foreground">{{block.description}}</span>
     </button>
    </CardContent>
   </Card>

   <Card class="min-w-0">
    <CardHeader class="flex-row items-center justify-between gap-3">
     <div><CardTitle>实时预览</CardTitle><CardDescription>{{definition.name}} · {{Object.keys(spec.props).length}} 个属性</CardDescription></div>
     <div class="flex gap-2">
      <Button size="sm" variant="secondary" @click="reset"><RefreshCw :size="13"/>重置</Button>
      <Button size="sm" variant="secondary" @click="copy"><Copy :size="13"/>复制 JSON</Button>
      <Button size="sm" variant="outline" @click="download"><Download :size="13"/>导出</Button>
     </div>
    </CardHeader>
    <CardContent>
     <div class="min-h-[300px] rounded-lg bg-canvas-soft p-5 sm:p-8" data-testid="preview-canvas">
      <BlockPreview :spec="spec"/>
     </div>
     <p class="mt-3 text-[11px] text-ink-mute">映射：{{spec.kind}} → components/BlockPreview.vue → components/ui/*；主题跟随全局浅色/深色令牌。</p>
    </CardContent>
   </Card>

   <Card>
    <CardHeader><CardTitle>属性调试</CardTitle><CardDescription>改动直接写入规格</CardDescription></CardHeader>
    <CardContent class="flex flex-col gap-3 pt-0">
     <div v-for="item in definition.fields" :key="item.key" class="flex flex-col gap-1.5">
      <Label :for="`block-${item.key}`">{{item.label}}</Label>
      <Textarea v-if="item.type==='textarea'" :id="`block-${item.key}`" :model-value="String(field(item.key) ?? '')" :rows="3" @update:model-value="value=>setField(item.key,value)"/>
      <Select v-else-if="item.type==='select'" :id="`block-${item.key}`" :model-value="String(field(item.key) ?? 'neutral')" @update:model-value="value=>setField(item.key,value)">
       <option v-for="option in item.options" :key="option" :value="option">{{option}}</option>
      </Select>
      <div v-else-if="item.type==='toggle'" class="flex items-center gap-2">
       <Switch :id="`block-${item.key}`" :model-value="Boolean(field(item.key))" :label="item.label" @update:model-value="value=>setField(item.key,value)"/>
       <span class="text-xs text-muted-foreground">{{Boolean(field(item.key))?'开':'关'}}</span>
      </div>
      <Input v-else :id="`block-${item.key}`" :model-value="String(field(item.key) ?? '')" @update:model-value="value=>setField(item.key,value)"/>
     </div>
     <Separator/>
     <p class="text-[11px] text-ink-mute">属性只影响预览，不会写入后端或磁盘。</p>
    </CardContent>
   </Card>
  </div>

  <Card class="mt-5">
   <CardHeader class="flex-row flex-wrap items-center justify-between gap-3">
    <div><CardTitle>JSON 调试</CardTitle><CardDescription>粘贴规格、校验并渲染；未知类型或类型错误会就地报错</CardDescription></div>
    <div class="flex gap-2">
     <Button size="sm" @click="apply">应用 JSON</Button>
     <Button size="sm" variant="secondary" @click="jsonDraft=JSON.stringify(spec,null,2)">还原</Button>
    </div>
   </CardHeader>
   <CardContent>
    <Textarea v-model="jsonDraft" aria-label="积木 JSON" :rows="12" class="font-mono text-[12px]"/>
    <Alert v-if="error" variant="danger" title="校验失败" class="mt-3">{{error}}</Alert>
    <p v-else-if="notice" class="mt-3 text-xs text-muted-foreground" aria-live="polite">{{notice}}</p>
   </CardContent>
  </Card>
  </template>
 </div>
</template>
