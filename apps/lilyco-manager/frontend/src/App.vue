<script setup lang="ts">
import {ref,computed,onMounted} from 'vue'
import {Blocks,LayoutDashboard,History,Plug,Settings,Search,Play,RefreshCw,Terminal,Database,ShieldCheck,Activity} from 'lucide-vue-next'
type Tool={name:string;description:string;read_only:boolean}
type Run={id:number;tool:string;output:string;created_at:string}
type Adapter={tool:string;project:string;installed:boolean;read_only:boolean}
type Project={name:string;description:string|null;url:string;category:string;integration:string;visibility:string}
const token=new URLSearchParams(location.hash.slice(1)).get('token')??''
history.replaceState(null,'',location.pathname)
const menu=[{id:'overview',name:'概览',icon:LayoutDashboard},{id:'tools',name:'工具中心',icon:Blocks},{id:'ecosystem',name:'生态项目',icon:Blocks},{id:'runs',name:'运行历史',icon:History},{id:'mcp',name:'MCP 服务',icon:Plug},{id:'settings',name:'设置',icon:Settings}]
const projects=ref<Project[]>([])
const adapters=ref<Adapter[]>([])
const tab=ref('tools'), tools=ref<Tool[]>([]), runs=ref<Run[]>([]), query=ref(''), loading=ref(false), error=ref(''), output=ref(''), selected=ref('')
const installed=computed(()=>new Map(adapters.value.map(x=>[x.tool,x.installed])))
const filtered=computed(()=>tools.value.filter(x=>(x.name+x.description).toLowerCase().includes(query.value.toLowerCase())))
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
onMounted(refresh)
</script>
<template>
<div class="min-h-screen lg:flex bg-[#0b1220] text-[#e9eefb]">
 <aside class="lg:w-64 lg:min-h-screen bg-[#111b2b] border-b lg:border-b-0 lg:border-r border-white/10 p-5 flex flex-col gap-9">
  <div class="flex items-center gap-3"><div class="rounded-xl bg-indigo-400/15 text-indigo-300 p-3"><Blocks :size="22"/></div><div><div class="text-lg font-bold">Lilyco</div><div class="text-xs text-slate-500 tracking-wider">MANAGER CONSOLE</div></div></div>
  <nav aria-label="主导航" class="flex lg:flex-col gap-2 overflow-x-auto">
   <button v-for="item in menu" :key="item.id" @click="tab=item.id" :aria-current="tab===item.id?'page':undefined" :class="['shrink-0 rounded-lg px-3 py-3 flex gap-3 items-center text-sm text-left',tab===item.id?'bg-indigo-400/15 text-indigo-200':'text-slate-400 hover:bg-white/5']"><component :is="item.icon" :size="17"/>{{item.name}}</button>
  </nav>
  <p class="hidden lg:block mt-auto text-xs text-slate-500 border-t border-white/10 pt-5">● 本机连接 · Lilyco Manager</p>
 </aside>
 <main class="flex-1 min-w-0 max-w-[1500px] mx-auto p-5 sm:p-8 lg:p-10">
  <header class="flex flex-wrap justify-between gap-4 items-center mb-8"><div><p class="text-indigo-300 text-xs tracking-widest mb-2">WORKSPACE / LILYCO</p><h1 class="text-3xl font-semibold">{{menu.find(x=>x.id===tab)?.name}}</h1><p class="text-sm text-slate-400 mt-2">本地工具 · 执行状态 · Agent 集成</p></div><button @click="refresh" :disabled="loading" class="border border-white/15 rounded-lg px-4 py-2 flex items-center gap-2 text-sm hover:bg-white/5"><RefreshCw :size="16" :class="loading?'animate-spin':''"/>刷新</button></header>
  <p v-if="error" role="alert" class="p-4 mb-5 border border-red-400/30 rounded-xl bg-red-400/10 text-red-200">{{error}}</p>
  <div class="grid sm:grid-cols-3 gap-4 mb-7">
   <div v-for="stat in [{label:'已注册工具',value:tools.length,icon:Blocks},{label:'最近执行记录',value:runs.length,icon:Activity},{label:'存储引擎',value:'SQLite',icon:Database}]" :key="stat.label" class="rounded-xl bg-[#141d2d] border border-white/10 p-5"><div class="flex justify-between text-sm text-slate-400"><span>{{stat.label}}</span><component :is="stat.icon" :size="18"/></div><div class="text-3xl font-semibold mt-5">{{stat.value}}</div></div>
  </div>
  <section v-if="tab==='overview'||tab==='tools'" class="bg-[#141d2d] border border-white/10 rounded-xl overflow-hidden">
   <div class="p-5 border-b border-white/10 flex flex-wrap gap-4 justify-between"><div><h2 class="text-lg font-semibold">工具注册表</h2><p class="text-slate-400 text-xs mt-1">只读工具 · 安全调用</p></div><label class="flex items-center gap-2 border border-white/10 bg-[#0b1220] rounded-lg px-3"><Search :size="16" class="text-slate-400"/><input v-model="query" aria-label="搜索工具" placeholder="搜索工具" class="bg-transparent outline-none py-2 text-sm w-40"/></label></div>
   <p v-if="filtered.length===0" class="p-10 text-center text-slate-400">暂无匹配工具</p>
   <div v-for="tool in filtered" :key="tool.name" class="p-5 border-b last:border-0 border-white/5 flex flex-wrap justify-between items-center gap-4">
    <div class="flex gap-3 items-center"><div class="p-3 rounded-lg bg-indigo-400/10 text-indigo-300"><Terminal :size="19"/></div><div><p class="font-mono text-sm">{{tool.name}}</p><p class="text-slate-400 text-xs mt-1">{{tool.description}}</p></div></div>
    <div class="flex gap-3 items-center"><span class="flex gap-1 text-xs text-emerald-300"><ShieldCheck :size="14"/>只读</span><span v-if="installed.has(tool.name)" class="text-xs" :class="installed.get(tool.name)?'text-emerald-300':'text-amber-300'">{{installed.get(tool.name)?'已安装':'未安装'}}</span><button @click="invoke(tool)" :disabled="loading || (installed.has(tool.name) && !installed.get(tool.name))" class="rounded-lg bg-indigo-500 hover:bg-indigo-400 disabled:opacity-50 px-3 py-2 flex gap-2 items-center text-xs"><Play :size="13"/>运行</button></div>
   </div>
  </section>
  <section v-if="tab==='overview'||tab==='tools'" class="mt-5 bg-[#141d2d] border border-white/10 rounded-xl p-5"><div class="flex justify-between gap-3"><h2 class="font-semibold">执行结果</h2><span class="text-xs font-mono text-slate-400">{{selected||'NO TASK'}}</span></div><pre class="mt-4 rounded-lg bg-[#090f19] p-4 text-sm text-emerald-200 max-h-80 overflow-auto">{{output||'选择工具后显示 JSON 输出'}}</pre></section>
  <section v-if="tab==='ecosystem'" class="bg-[#141d2d] border border-white/10 rounded-xl overflow-hidden">
   <div class="p-5 border-b border-white/10"><h2 class="text-lg font-semibold">Lilyco 生态仓库</h2><p class="text-xs text-slate-400 mt-1">来自 lilyco-42 GitHub 仓库目录 · 仅发现，未获准执行</p></div>
   <div class="p-3 sm:p-5 grid md:grid-cols-2 xl:grid-cols-3 gap-3">
    <article v-for="project in projects" :key="project.name" class="rounded-xl bg-[#0b1220] border border-white/10 p-4 flex flex-col gap-3">
     <div class="flex justify-between gap-2"><h3 class="font-mono text-sm font-semibold">{{project.name}}</h3><span class="text-xs text-slate-400">{{project.category}}</span></div>
     <p class="text-xs text-slate-400 line-clamp-3 flex-1">{{project.description||'暂无仓库描述'}}</p>
     <div class="flex items-center justify-between"><span class="text-xs text-amber-300">已发现 · 未接入</span><a :href="project.url" target="_blank" rel="noopener noreferrer" class="text-sm text-indigo-300 underline">GitHub ↗</a></div>
    </article>
   </div>
  </section>
  <section v-if="tab==='runs'" class="bg-[#141d2d] border border-white/10 rounded-xl overflow-hidden"><h2 class="p-5 border-b border-white/10 font-semibold">运行历史</h2><div v-for="run in runs" :key="run.id" class="p-5 border-b border-white/5 flex justify-between gap-2 flex-wrap text-sm"><span class="font-mono">{{run.tool}}</span><span class="text-slate-400">{{run.created_at}}</span></div><p v-if="!runs.length" class="p-10 text-center text-slate-400">暂无记录</p></section>
  <section v-if="tab==='mcp'||tab==='settings'" class="bg-[#141d2d] border border-white/10 rounded-xl p-8"><h2 class="font-semibold text-lg">{{tab==='mcp'?'MCP 网关':'系统设置'}}</h2><p class="text-slate-400 text-sm mt-3">功能规划中，后端接口尚未开放。</p></section>
 </main>
</div>
</template>


