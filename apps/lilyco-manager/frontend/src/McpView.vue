<script setup lang="ts">
import {computed,onMounted,ref} from 'vue'
import {Plug,RefreshCw,Search,Copy,Save,ShieldCheck,AlertTriangle} from 'lucide-vue-next'
type Tool={name:string;app:string;original_name:string;description:string;input_schema:unknown}
type Provider={id:string;ok:boolean;tools?:number;error?:string}
type Config={servers:Record<string,unknown>;config_path:string|null;toolhub:string|null;client_config:unknown;hint:string;written?:boolean}
const props=defineProps<{token:string}>()
const tools=ref<Tool[]>([]),providers=ref<Provider[]>([]),config=ref<Config|null>(null)
const query=ref(''),loading=ref(false),busy=ref(''),error=ref(''),notice=ref('')
async function api(path:string,method='GET'){
 const response=await fetch(path,{method,headers:{'x-lilyco-token':props.token}})
 if(!response.ok)throw new Error('HTTP '+response.status)
 return response.json()
}
async function refresh(){
 loading.value=true;error.value=''
 try{
  const [t,c]=await Promise.all([api('/api/mcp/tools'),api('/api/mcp/config')])
  tools.value=t.tools??[];providers.value=t.providers??[];config.value=c
 }catch(e){error.value=String(e)}finally{loading.value=false}
}
const filtered=computed(()=>tools.value.filter(tool=>(tool.name+tool.app+tool.description).toLowerCase().includes(query.value.toLowerCase())))
const grouped=computed(()=>{
 const groups=new Map<string,Tool[]>()
 for(const tool of filtered.value){const list=groups.get(tool.app)??[];list.push(tool);groups.set(tool.app,list)}
 return [...groups.entries()].sort((a,b)=>a[0].localeCompare(b[0]))
})
const configText=computed(()=>JSON.stringify(config.value?.client_config??config.value?.servers??{},null,2))
async function copyConfig(){
 try{await navigator.clipboard.writeText(configText.value);notice.value='已复制到剪贴板'}catch{notice.value='复制失败，请手动选择文本'}
}
async function writeConfig(){
 busy.value='write';error.value='';notice.value=''
 try{const data=await api('/api/mcp/config','POST');config.value=data;notice.value='已写入 '+(data.config_path??'')}
 catch(e){error.value='写入失败：'+String(e)}finally{busy.value=''}
}
onMounted(refresh)
</script>
<template>
 <section class="mb-5">
  <div class="flex flex-wrap justify-between gap-3 items-center mb-4">
   <div><h2 class="text-lg font-semibold">MCP 服务</h2><p class="text-xs text-ink-soft mt-1">从每个应用的 --mcp 动态发现工具 · 统一命名空间 app__tool</p></div>
   <button @click="refresh" :disabled="loading" class="btn btn-secondary"><RefreshCw :size="16" :class="loading?'animate-spin':''"/>刷新</button>
  </div>
  <p v-if="error" role="alert" class="alert alert-danger mb-4">{{error}}</p>
  <p v-if="notice" class="alert alert-success mb-4">{{notice}}</p>
  <div v-if="providers.length" class="grid sm:grid-cols-2 xl:grid-cols-3 gap-3 mb-5">
   <div v-for="provider in providers" :key="provider.id" class="card p-4">
    <div class="flex justify-between gap-2 items-center"><span class="font-mono text-sm">{{provider.id}}</span><span class="badge" :class="provider.ok?'badge-success':'badge-danger'"><ShieldCheck v-if="provider.ok" :size="13"/><AlertTriangle v-else :size="13"/>{{provider.ok?'可用':'失败'}}</span></div>
    <p v-if="provider.ok" class="text-xs text-ink-soft mt-2">{{provider.tools}} 个工具</p>
    <p v-else class="text-xs text-danger mt-2 break-words">{{provider.error}}</p>
   </div>
  </div>
  <div class="card overflow-hidden mb-5">
   <div class="p-5 border-b border-line flex flex-wrap gap-4 justify-between items-center">
    <div><h3 class="font-semibold">聚合工具</h3><p class="text-xs text-ink-soft mt-1">{{tools.length}} 个工具 · 点击复制完整配置后接入 Claude / Cursor / Codex</p></div>
    <label class="flex items-center gap-2 border border-line bg-canvas rounded-lg px-3"><Search :size="16" class="text-ink-mute"/><input v-model="query" aria-label="搜索 MCP 工具" placeholder="搜索工具" class="bg-transparent outline-none py-2 text-sm w-44 text-ink"/></label>
   </div>
   <p v-if="!filtered.length" class="p-10 text-center text-ink-soft">{{loading?'正在发现 MCP 工具…':'暂无 MCP 工具'}}</p>
   <div v-for="[app,list] in grouped" :key="app" class="p-5 border-b last:border-0 border-line/60">
    <div class="flex items-center gap-2 mb-3"><Plug :size="15" class="text-brand"/><h4 class="font-mono text-sm text-brand">{{app}}</h4><span class="badge">{{list.length}}</span></div>
    <div class="grid md:grid-cols-2 gap-2">
     <div v-for="tool in list" :key="tool.name" class="rounded-lg bg-canvas-soft border border-line/70 p-3">
      <p class="font-mono text-xs text-brand break-all">{{tool.name}}</p>
      <p class="text-xs text-ink-soft mt-1 line-clamp-2">{{tool.description||'无描述'}}</p>
     </div>
    </div>
   </div>
  </div>
  <div class="card p-5">
   <div class="flex flex-wrap justify-between gap-3 items-center mb-3">
    <div><h3 class="font-semibold">客户端配置</h3><p class="text-xs text-ink-soft mt-1">{{config?.hint}}</p></div>
    <div class="flex gap-2">
     <button @click="copyConfig" class="btn btn-secondary btn-sm"><Copy :size="13"/>复制</button>
     <button @click="writeConfig" :disabled="busy==='write'" class="btn btn-brand btn-sm"><Save :size="13"/>写入 tools.json</button>
    </div>
   </div>
   <pre class="log p-4 max-h-72 overflow-auto">{{configText}}</pre>
   <p v-if="config?.config_path" class="text-xs text-ink-mute mt-2 font-mono break-all">{{config.config_path}}</p>
  </div>
 </section>
</template>
