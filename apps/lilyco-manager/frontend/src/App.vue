<script setup lang="ts">
import {ref,computed,onMounted} from 'vue'
import CommerceStudio from './CommerceStudio.vue'
import AppsView from './AppsView.vue'
import McpView from './McpView.vue'
import catalog from '../../catalog.json'
import {Blocks,Package,LayoutDashboard,History,Plug,Settings,Search,Play,RefreshCw,Terminal,Database,ShieldCheck,Activity,Sun,Moon} from 'lucide-vue-next'
type Tool={name:string;description:string;read_only:boolean}
type Run={id:number;tool:string;output:string;created_at:string}
type Adapter={tool:string;project:string;installed:boolean;read_only:boolean}
type Project={name:string;description:string|null;url:string;category:string;integration:string;visibility:string}
type Theme='light'|'dark'
const token=new URLSearchParams(location.hash.slice(1)).get('token')??''
history.replaceState(null,'',location.pathname)
const hasManagerSession=!!token
const fullMenu=[{id:'overview',name:'概览',icon:LayoutDashboard},{id:'apps',name:'应用',icon:Package},{id:'studio',name:'商品图工坊',icon:LayoutDashboard},{id:'tools',name:'工具中心',icon:Blocks},{id:'ecosystem',name:'生态项目',icon:Blocks},{id:'runs',name:'运行历史',icon:History},{id:'mcp',name:'MCP 服务',icon:Plug},{id:'settings',name:'设置',icon:Settings}]
const menu=hasManagerSession?fullMenu:fullMenu.filter(x=>x.id==='studio'||x.id==='ecosystem')
const projects=ref<Project[]>([])
const adapters=ref<Adapter[]>([])
const tab=ref(hasManagerSession?'apps':'studio'), tools=ref<Tool[]>([]), runs=ref<Run[]>([]), query=ref(''), loading=ref(false), error=ref(''), output=ref(''), selected=ref('')
const theme=ref<Theme>('light')
const installed=computed(()=>new Map(adapters.value.map(x=>[x.tool,x.installed])))
const community=catalog.entries.filter(p=>p.origin==='community')
const ownedFeatured=catalog.entries.filter(p=>p.origin==='first_party')
const filtered=computed(()=>tools.value.filter(x=>(x.name+x.description).toLowerCase().includes(query.value.toLowerCase())))
function applyTheme(next:Theme){
 theme.value=next
 document.documentElement.dataset.theme=next
 try{localStorage.setItem('lilyco-theme',next)}catch{}
}
function toggleTheme(){applyTheme(theme.value==='dark'?'light':'dark')}
async function api(path:string,method='GET'){
 const response=await fetch(path,{method,headers:{'x-lilyco-token':token}})
 if(!response.ok)throw new Error('HTTP '+response.status)
 return response.json()
}
async function refresh(){
 loading.value=true;error.value=''
 try{const [t,h,e,a]=await Promise.all([api('/api/tools'),api('/api/history'),api('/api/ecosystem'),api('/api/adapters')]);tools.value=t.tools??[];runs.value=h.runs??[];projects.value=e.projects??[];adapters.value=a.adapters??[]}
 catch(e){error.value=String(e)}finally{loading.value=false}
}
async function invoke(tool:Tool){
 loading.value=true;selected.value=tool.name;error.value=''
 try{output.value=JSON.stringify(await api('/api/tools/'+encodeURIComponent(tool.name)+'/run','POST'),null,2);await refresh()}
 catch(e){error.value=String(e)}finally{loading.value=false}
}
onMounted(()=>{
 let saved:Theme|null=null
 try{saved=localStorage.getItem('lilyco-theme') as Theme|null}catch{}
 applyTheme(saved??(window.matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light'))
 if(hasManagerSession)refresh()
})
</script>
<template>
<div class="min-h-screen lg:flex bg-canvas text-ink">
 <aside class="lg:w-64 lg:min-h-screen bg-canvas-soft border-b lg:border-b-0 lg:border-r border-line p-5 flex flex-col gap-7">
  <div class="flex items-center gap-3">
   <div class="rounded-xl bg-brand-soft text-brand p-3"><Blocks :size="22"/></div>
   <div><div class="text-lg font-bold">Lilyco</div><div class="text-xs text-ink-mute tracking-wider">MANAGER CONSOLE</div></div>
  </div>
  <nav aria-label="主导航" class="flex lg:flex-col gap-1.5 overflow-x-auto">
   <button v-for="item in menu" :key="item.id" @click="tab=item.id" :aria-current="tab===item.id?'page':undefined" :class="['shrink-0 rounded-lg px-3 py-2.5 flex gap-3 items-center text-sm text-left transition-colors',tab===item.id?'bg-brand-soft text-brand font-semibold':'text-ink-soft hover:bg-surface-hover']"><component :is="item.icon" :size="17"/>{{item.name}}</button>
  </nav>
  <div class="hidden lg:flex mt-auto flex-col gap-3 border-t border-line pt-5">
   <button @click="toggleTheme" class="btn btn-secondary btn-sm justify-start" :aria-label="theme==='dark'?'切换到浅色主题':'切换到深色主题'">
    <Sun v-if="theme==='dark'" :size="15"/><Moon v-else :size="15"/>{{theme==='dark'?'浅色主题':'深色主题'}}
   </button>
   <p class="text-xs text-ink-mute">● 本机连接 · 仅回环地址</p>
  </div>
 </aside>
 <main class="flex-1 min-w-0 max-w-[1500px] mx-auto p-5 sm:p-8 lg:p-10">
  <header class="flex flex-wrap justify-between gap-4 items-center mb-8">
   <div><p class="text-brand text-xs tracking-widest mb-2">WORKSPACE / LILYCO</p><h1 class="text-3xl font-semibold">{{menu.find(x=>x.id===tab)?.name}}</h1><p class="text-sm text-ink-soft mt-2">本地工具 · 执行状态 · Agent 集成</p></div>
   <div class="flex items-center gap-2">
    <button @click="toggleTheme" class="btn btn-secondary btn-sm lg:hidden" :aria-label="theme==='dark'?'切换到浅色主题':'切换到深色主题'"><Sun v-if="theme==='dark'" :size="15"/><Moon v-else :size="15"/></button>
    <button v-if="hasManagerSession && !['studio','apps','mcp'].includes(tab)" @click="refresh" :disabled="loading" class="btn btn-secondary"><RefreshCw :size="16" :class="loading?'animate-spin':''"/>刷新</button>
   </div>
  </header>
  <p v-if="error && tab!=='studio'" role="alert" class="alert alert-danger mb-5">{{error}}</p>
  <div v-if="hasManagerSession && !['studio','apps','mcp'].includes(tab)" class="grid sm:grid-cols-3 gap-4 mb-7">
   <div v-for="stat in [{label:'已注册工具',value:tools.length,icon:Blocks},{label:'最近执行记录',value:runs.length,icon:Activity},{label:'存储引擎',value:'SQLite',icon:Database}]" :key="stat.label" class="card p-5"><div class="flex justify-between text-sm text-ink-soft"><span>{{stat.label}}</span><component :is="stat.icon" :size="18"/></div><div class="text-3xl font-semibold mt-5">{{stat.value}}</div></div>
  </div>
  <section v-if="tab==='overview'||tab==='tools'" class="card overflow-hidden">
   <div class="p-5 border-b border-line flex flex-wrap gap-4 justify-between"><div><h2 class="text-lg font-semibold">工具注册表</h2><p class="text-ink-soft text-xs mt-1">只读工具 · 安全调用</p></div><label class="flex items-center gap-2 border border-line bg-canvas rounded-lg px-3"><Search :size="16" class="text-ink-mute"/><input v-model="query" aria-label="搜索工具" placeholder="搜索工具" class="bg-transparent outline-none py-2 text-sm w-40 text-ink"/></label></div>
   <p v-if="filtered.length===0" class="p-10 text-center text-ink-soft">暂无匹配工具</p>
   <div v-for="tool in filtered" :key="tool.name" class="p-5 border-b last:border-0 border-line/60 flex flex-wrap justify-between items-center gap-4">
    <div class="flex gap-3 items-center"><div class="p-3 rounded-lg bg-brand-soft text-brand"><Terminal :size="19"/></div><div><p class="font-mono text-sm">{{tool.name}}</p><p class="text-ink-soft text-xs mt-1">{{tool.description}}</p></div></div>
    <div class="flex gap-3 items-center"><span class="badge badge-success"><ShieldCheck :size="13"/>只读</span><span v-if="installed.has(tool.name)" class="badge" :class="installed.get(tool.name)?'badge-success':'badge-warning'">{{installed.get(tool.name)?'已安装':'未安装'}}</span><button @click="invoke(tool)" :disabled="loading || (installed.has(tool.name) && !installed.get(tool.name))" class="btn btn-brand btn-sm"><Play :size="13"/>运行</button></div>
   </div>
  </section>
  <section v-if="tab==='overview'||tab==='tools'" class="card p-5 mt-5"><div class="flex justify-between gap-3"><h2 class="font-semibold">执行结果</h2><span class="text-xs font-mono text-ink-mute">{{selected||'NO TASK'}}</span></div><pre class="log mt-4 p-4 max-h-80 overflow-auto">{{output||'选择工具后显示 JSON 输出'}}</pre></section>
  <CommerceStudio v-if="tab==='studio'"/>
  <section v-if="tab==='ecosystem'" class="card overflow-hidden">
   <div class="p-5 border-b border-line"><h2 class="text-lg font-semibold">Lilyco 生态仓库</h2><p class="text-xs text-ink-soft mt-1">来自 lilyco-42 GitHub 仓库目录 · 仅发现，未获准执行</p></div>
   <div class="p-5 border-b border-line">
    <h3 class="font-semibold text-sm text-brand mb-3">重点本家工具</h3>
    <div class="grid md:grid-cols-2 gap-3">
     <a v-for="repo in ownedFeatured" :key="repo.name" :href="repo.repository" target="_blank" rel="noopener noreferrer" class="p-4 rounded-lg bg-brand-soft/60 border border-brand/20 hover:bg-brand-soft">
      <div class="font-mono text-sm">{{repo.name}} ↗</div><p class="text-xs text-ink-soft mt-2">{{repo.description}}</p>
     </a>
    </div>
   </div>
   <div class="p-5 border-b border-line">
    <div class="flex flex-wrap justify-between items-center gap-3 mb-3"><h3 class="font-semibold text-sm text-brand">社区项目 · 外部来源</h3><a class="text-xs text-brand underline" href="https://github.com/lilyco-42/lilyco/issues/new?template=eco-project.yml" rel="noopener noreferrer" target="_blank">投稿发布项目 ↗</a></div>
    <p class="text-xs text-ink-soft mb-3">仅展示已审核的项目链接；不自动执行第三方代码</p>
    <div class="grid md:grid-cols-2 gap-3">
     <a v-for="repo in community" :key="repo.name" :href="repo.repository" target="_blank" rel="noopener noreferrer" class="p-4 rounded-lg bg-canvas-soft border border-line hover:border-brand/30">
      <div class="font-mono text-sm">{{repo.name}} ↗</div><p class="text-xs text-ink-soft mt-2">{{repo.description}}</p>
     </a>
    </div>
   </div>
   <div class="p-3 sm:p-5 grid md:grid-cols-2 xl:grid-cols-3 gap-3">
    <article v-for="project in projects" :key="project.name" class="card p-4 flex flex-col gap-3">
     <div class="flex justify-between gap-2"><h3 class="font-mono text-sm font-semibold">{{project.name}}</h3><span class="text-xs text-ink-mute">{{project.category}}</span></div>
     <p class="text-xs text-ink-soft line-clamp-3 flex-1">{{project.description||'暂无仓库描述'}}</p>
     <div class="flex items-center justify-between"><span class="badge badge-warning">已发现 · 未接入</span><a :href="project.url" target="_blank" rel="noopener noreferrer" class="text-sm text-brand underline">GitHub ↗</a></div>
    </article>
   </div>
  </section>
  <section v-if="tab==='runs'" class="card overflow-hidden"><h2 class="p-5 border-b border-line font-semibold">运行历史</h2><div v-for="run in runs" :key="run.id" class="p-5 border-b border-line/60 flex justify-between gap-2 flex-wrap text-sm"><span class="font-mono">{{run.tool}}</span><span class="text-ink-soft">{{run.created_at}}</span></div><p v-if="!runs.length" class="p-10 text-center text-ink-soft">暂无记录</p></section>
  <AppsView v-if="tab==='apps'" :token="token"/>
  <McpView v-if="tab==='mcp'" :token="token"/>
  <section v-if="tab==='settings'" class="card p-8"><h2 class="font-semibold text-lg">系统设置</h2><p class="text-ink-soft text-sm mt-3">主题偏好保存在本机浏览器；应用与任务配置来自各仓库的 lilyco.app.json。</p></section>
 </main>
</div>
</template>
