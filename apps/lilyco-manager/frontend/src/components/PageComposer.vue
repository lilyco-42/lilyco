<script setup lang="ts">
import {computed,ref,watch} from 'vue'
import {ArrowDown,ArrowUp,Copy,Download,Layers,Plus,RefreshCw,Trash2,Wand2} from 'lucide-vue-next'
import {blocks,createPage,definitionOf,MAX_PAGE_BLOCKS,validatePage,type BlockKind,type BlockSpec,type PageSpec} from '../blocks/catalog'
import BlockPreview from './BlockPreview.vue'
import Alert from './ui/Alert.vue'
import Badge from './ui/Badge.vue'
import Button from './ui/Button.vue'
import Card from './ui/Card.vue'
import CardContent from './ui/CardContent.vue'
import CardDescription from './ui/CardDescription.vue'
import CardHeader from './ui/CardHeader.vue'
import CardTitle from './ui/CardTitle.vue'
import Input from './ui/Input.vue'
import Label from './ui/Label.vue'
import Select from './ui/Select.vue'
import Separator from './ui/Separator.vue'
import Switch from './ui/Switch.vue'
import Textarea from './ui/Textarea.vue'

const page=ref<PageSpec>(createPage())
const selected=ref(0)
const pageJson=ref(JSON.stringify(page.value,null,2))
const error=ref('')
const notice=ref('')

watch(page,value=>{pageJson.value=JSON.stringify(value,null,2)},{deep:true})

const current=computed<BlockSpec|undefined>(()=>page.value.blocks[selected.value])
const definition=computed(()=>current.value?definitionOf(current.value.kind):undefined)
const canAdd=computed(()=>page.value.blocks.length<MAX_PAGE_BLOCKS)

function add(kind:BlockKind){
 if(!canAdd.value){error.value=`一页最多 ${MAX_PAGE_BLOCKS} 块积木`;return}
 page.value.blocks.push({kind,props:{...definitionOf(kind).defaults}})
 selected.value=page.value.blocks.length-1
 error.value=''
 notice.value=`已追加「${definitionOf(kind).name}」`
}
function remove(index:number){
 if(page.value.blocks.length<=1){error.value='原型页至少保留 1 块积木';return}
 page.value.blocks.splice(index,1)
 selected.value=Math.min(selected.value,page.value.blocks.length-1)
 error.value=''
 notice.value='已删除该积木'
}
function move(index:number,delta:number){
 const target=index+delta
 if(target<0||target>=page.value.blocks.length)return
 const [item]=page.value.blocks.splice(index,1)
 page.value.blocks.splice(target,0,item)
 selected.value=target
 error.value=''
}
function field(key:string){return current.value?.props[key]}
function setField(key:string,value:string|boolean){
 if(!current.value)return
 current.value.props[key]=value
 error.value=''
}
function resetBlock(){
 if(!current.value)return
 current.value.props={...definitionOf(current.value.kind).defaults}
 error.value=''
 notice.value='已重置当前积木为默认值'
}
function resetPage(){page.value=createPage();selected.value=0;error.value='';notice.value='已重置为示例原型页'}
function apply(){
 error.value='';notice.value=''
 let parsed:unknown
 try{parsed=JSON.parse(pageJson.value)}catch(e){error.value='JSON 解析失败：'+String(e);return}
 const result=validatePage(parsed)
 if(result.error||!result.page){error.value=result.error??'校验失败';return}
 page.value=result.page
 selected.value=0
 notice.value=`已应用「${result.page.title}」，共 ${result.page.blocks.length} 块`
}
async function copy(){
 try{
  await navigator.clipboard.writeText(pageJson.value)
  notice.value='页面 JSON 已复制到剪贴板'
 }catch{
  notice.value='浏览器拒绝剪贴板访问，请手动复制下方 JSON'
 }
}
function download(){
 const blob=new Blob([pageJson.value],{type:'application/json'})
 const url=URL.createObjectURL(blob)
 const link=document.createElement('a')
 link.href=url;link.download=`lilyco-page-${Date.now()}.json`;link.click()
 URL.revokeObjectURL(url)
 notice.value='页面 JSON 已导出'
}
</script>

<template>
 <div>
  <div class="mb-5 flex flex-wrap items-start justify-between gap-3">
   <div>
    <h2 class="text-lg font-semibold">页面组合</h2>
    <p class="mt-1 text-xs text-ink-soft">把积木按顺序拼成原型页：左侧追加、中央排序与选中、右侧改选中块属性、下方编辑整页 JSON。纯本机渲染，不写后端。</p>
   </div>
   <div class="flex items-center gap-2">
    <Badge variant="brand"><Wand2 :size="12"/>原型页</Badge>
    <Badge>{{page.blocks.length}} / {{MAX_PAGE_BLOCKS}} 块</Badge>
   </div>
  </div>

  <div class="grid gap-5 xl:grid-cols-[210px_minmax(0,1fr)_280px]">
   <Card>
    <CardHeader><CardTitle class="flex items-center gap-2"><Layers :size="16"/>追加积木</CardTitle><CardDescription>点击追加到页面末尾</CardDescription></CardHeader>
    <CardContent class="flex flex-col gap-1.5 pt-0">
     <button v-for="block in blocks" :key="block.kind" type="button" :aria-label="`添加积木：${block.name}`" :disabled="!canAdd"
      class="rounded-md border border-transparent px-3 py-2 text-left transition-colors hover:bg-muted disabled:opacity-50"
      @click="add(block.kind)">
      <span class="flex items-center justify-between gap-2 text-[13px] font-semibold">{{block.name}}<Plus :size="13" class="text-ink-mute"/></span>
      <span class="mt-0.5 block text-[11px] text-muted-foreground">{{block.description}}</span>
     </button>
    </CardContent>
   </Card>

   <Card class="min-w-0">
    <CardHeader class="flex-row flex-wrap items-center justify-between gap-3">
     <div><CardTitle>原型页预览</CardTitle><CardDescription>{{page.title}} · 点击积木选中后调属性</CardDescription></div>
     <div class="flex gap-2">
      <Button size="sm" variant="secondary" @click="resetPage"><RefreshCw :size="13"/>重置页面</Button>
      <Button size="sm" variant="secondary" @click="copy"><Copy :size="13"/>复制 JSON</Button>
      <Button size="sm" variant="outline" @click="download"><Download :size="13"/>导出</Button>
     </div>
    </CardHeader>
    <CardContent>
     <div class="min-h-[360px] rounded-lg bg-canvas-soft p-5 sm:p-8" data-testid="page-canvas">
      <h3 class="mb-4 text-xl font-semibold">{{page.title}}</h3>
      <div class="flex flex-col gap-4">
       <div v-for="(spec,index) in page.blocks" :key="`${index}-${spec.kind}`" class="relative">
        <div :class="['rounded-lg p-1 transition-shadow',selected===index?'ring-2 ring-focus-ring':'']" @click="selected=index">
         <BlockPreview :spec="spec"/>
        </div>
        <div class="mt-1 flex items-center justify-end gap-1 text-[11px] text-ink-mute">
         <span class="mr-auto">第 {{index+1}} 块 · {{definitionOf(spec.kind).name}}</span>
         <Button size="sm" variant="ghost" :aria-label="`上移第 ${index+1} 块`" :disabled="index===0" @click.stop="move(index,-1)"><ArrowUp :size="12"/></Button>
         <Button size="sm" variant="ghost" :aria-label="`下移第 ${index+1} 块`" :disabled="index===page.blocks.length-1" @click.stop="move(index,1)"><ArrowDown :size="12"/></Button>
         <Button size="sm" variant="ghost" :aria-label="`删除第 ${index+1} 块`" :disabled="page.blocks.length<=1" @click.stop="remove(index)"><Trash2 :size="12"/></Button>
        </div>
       </div>
      </div>
     </div>
     <p class="mt-3 text-[11px] text-ink-mute">排序、删除只改本地数组；导出得到可直接复用的页面 JSON。</p>
    </CardContent>
   </Card>

   <Card>
    <CardHeader><CardTitle>选中块属性</CardTitle><CardDescription>{{definition?`第 ${selected+1} 块 · ${definition.name}`:'没有选中块'}}</CardDescription></CardHeader>
    <CardContent class="flex flex-col gap-3 pt-0">
     <template v-if="definition">
      <div v-for="item in definition.fields" :key="item.key" class="flex flex-col gap-1.5">
       <Label :for="`page-${item.key}`">{{item.label}}</Label>
       <Textarea v-if="item.type==='textarea'" :id="`page-${item.key}`" :model-value="String(field(item.key) ?? '')" :rows="3" @update:model-value="value=>setField(item.key,value)"/>
       <Select v-else-if="item.type==='select'" :id="`page-${item.key}`" :model-value="String(field(item.key) ?? 'neutral')" @update:model-value="value=>setField(item.key,value)">
        <option v-for="option in item.options" :key="option" :value="option">{{option}}</option>
       </Select>
       <div v-else-if="item.type==='toggle'" class="flex items-center gap-2">
        <Switch :id="`page-${item.key}`" :model-value="Boolean(field(item.key))" :label="item.label" @update:model-value="value=>setField(item.key,value)"/>
        <span class="text-xs text-muted-foreground">{{Boolean(field(item.key))?'开':'关'}}</span>
       </div>
       <Input v-else :id="`page-${item.key}`" :model-value="String(field(item.key) ?? '')" @update:model-value="value=>setField(item.key,value)"/>
      </div>
      <Separator/>
      <Button size="sm" variant="secondary" @click="resetBlock"><RefreshCw :size="13"/>重置该块</Button>
     </template>
     <p v-else class="text-xs text-muted-foreground">先在上方点击一块积木。</p>
    </CardContent>
   </Card>
  </div>

  <Card class="mt-5">
   <CardHeader class="flex-row flex-wrap items-center justify-between gap-3">
    <div><CardTitle>页面 JSON</CardTitle><CardDescription>整页规格：title + blocks[]；校验通过才会替换预览</CardDescription></div>
    <div class="flex gap-2">
     <Button size="sm" @click="apply">应用 JSON</Button>
     <Button size="sm" variant="secondary" @click="pageJson=JSON.stringify(page,null,2)">还原</Button>
    </div>
   </CardHeader>
   <CardContent>
    <Textarea v-model="pageJson" aria-label="页面 JSON" :rows="14" class="font-mono text-[12px]"/>
    <Alert v-if="error" variant="danger" title="校验失败" class="mt-3">{{error}}</Alert>
    <p v-else-if="notice" class="mt-3 text-xs text-muted-foreground" aria-live="polite">{{notice}}</p>
   </CardContent>
  </Card>
 </div>
</template>
