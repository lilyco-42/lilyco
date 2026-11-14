<script setup lang="ts">
import {onMounted,onUnmounted,ref} from 'vue'
import {Package,Play,Square,ScrollText,ExternalLink,RefreshCw,ShieldCheck,Terminal,AlertTriangle,Download,GitBranch,Wrench,ChevronDown,ChevronUp,Trash2} from 'lucide-vue-next'
type Status={running:boolean;pid:number|null;started_at:number|null;exit_code:number|null;log_lines:number}
type AppInfo={id:string;name:string;description:string;repository:string|null;binary:string;installed:boolean;capabilities:{run:boolean;mcp:boolean;schema:boolean};url:string|null;release:boolean;dev_tasks:string[];status:Status|null}
type Release={configured:boolean;repo:string|null;platform:string;current_tag:string|null;latest_tag:string|null;asset:string|null;update_available:boolean;error:string|null}
type DevTask={name:string;argv:string[]}
type DevJob={id:string;app:string;task:string;running:boolean;exit_code:number|null;started_at:number;finished_at:number|null;lines:string[]}
const props=defineProps<{token:string}>()
const apps=ref<AppInfo[]>([]),warnings=ref<string[]>([]),loading=ref(false),busy=ref(''),error=ref(''),notice=ref('')
const logs=ref<Record<string,string[]>>({}),schemaCounts=ref<Record<string,number>>({})
const releases=ref<Record<string,Release>>({})
const dev=ref<Record<string,{tasks:DevTask[];jobs:DevJob[]}>>({})
const jobs=ref<Record<string,DevJob>>({})
const timers:Record<string,number>={}
async function api(path:string,method='GET'){
 const response=await fetch(path,{method,headers:{'x-lilyco-token':props.token}})
 if(!response.ok){let message='HTTP '+response.status;try{const body=await response.json();if(body.error)message=body.error}catch{}throw new Error(message)}
 return response.json()
}
async function refresh(){
 loading.value=true;error.value=''
 try{const data=await api('/api/apps');apps.value=data.apps??[];warnings.value=data.warnings??[]}
 catch(e){error.value=String(e)}finally{loading.value=false}
}
async function start(app:AppInfo){
 busy.value='start:'+app.id;error.value=''
 try{await api('/api/apps/'+encodeURIComponent(app.id)+'/start','POST');await refresh()}
 catch(e){error.value=app.name+' 启动失败：'+String(e)}finally{busy.value=''}
}
async function stop(app:AppInfo){
 busy.value='stop:'+app.id;error.value=''
 try{await api('/api/apps/'+encodeURIComponent(app.id)+'/stop','POST');await refresh()}
 catch(e){error.value=app.name+' 停止失败：'+String(e)}finally{busy.value=''}
}
async function toggleLogs(app:AppInfo){
 if(logs.value[app.id]){delete logs.value[app.id];return}
 error.value=''
 try{const data=await api('/api/apps/'+encodeURIComponent(app.id)+'/logs?limit=200');logs.value[app.id]=data.lines??[]}
 catch(e){error.value='日志读取失败：'+String(e)}
}
async function loadSchema(app:AppInfo){
 busy.value='schema:'+app.id;error.value=''
 try{const data=await api('/api/apps/'+encodeURIComponent(app.id)+'/schema');schemaCounts.value[app.id]=data.commands??0}
 catch(e){error.value=app.name+' 命令清单读取失败：'+String(e)}finally{busy.value=''}
}
async function checkRelease(app:AppInfo){
 busy.value='release:'+app.id;error.value=''
 try{const data=await api('/api/apps/'+encodeURIComponent(app.id)+'/release');releases.value[app.id]=data.release}
 catch(e){error.value=app.name+' 更新检查失败：'+String(e)}finally{busy.value=''}
}
async function install(app:AppInfo){
 busy.value='install:'+app.id;error.value='';notice.value=''
 try{
  const data=await api('/api/apps/'+encodeURIComponent(app.id)+'/install','POST')
  await Promise.all([refresh(),checkRelease(app)])
  notice.value=app.name+' 已安装 '+data.tag+(data.updated?'（新版本）':'（已是最新）')
 }catch(e){error.value=app.name+' 安装失败：'+String(e)}finally{busy.value=''}
}
async function uninstall(app:AppInfo){
 if(!window.confirm('确定卸载 '+app.name+' 的托管版本？源代码和 PATH 中的版本不会被删除。'))return
 busy.value='uninstall:'+app.id;error.value='';notice.value=''
 try{
  const data=await api('/api/apps/'+encodeURIComponent(app.id)+'/uninstall','POST')
  await Promise.all([refresh(),checkRelease(app)])
  notice.value=app.name+(data.removed?' 已卸载托管版本':' 没有托管版本')
 }catch(e){error.value=app.name+' 卸载失败：'+String(e)}finally{busy.value=''}
}
async function toggleDev(app:AppInfo){
 if(dev.value[app.id]){delete dev.value[app.id];return}
 error.value=''
 try{const data=await api('/api/apps/'+encodeURIComponent(app.id)+'/dev');dev.value[app.id]={tasks:data.tasks??[],jobs:data.jobs??[]}}
 catch(e){error.value=app.name+' 开发任务读取失败：'+String(e)}
}
async function runTask(app:AppInfo,task:DevTask){
 busy.value='dev:'+app.id+':'+task.name;error.value='';notice.value=''
 try{
  const data=await api('/api/apps/'+encodeURIComponent(app.id)+'/dev/'+encodeURIComponent(task.name),'POST')
  const now=Date.now()
  jobs.value[app.id]={id:data.job,app:app.id,task:task.name,running:true,exit_code:null,started_at:Math.floor(now/1000),finished_at:null,lines:[]}
  pollJob(app,data.job)
 }catch(e){error.value=app.name+' / '+task.name+' 启动失败：'+String(e)}finally{busy.value=''}
}
function pollJob(app:AppInfo,jobId:string){
 if(timers[app.id])clearTimeout(timers[app.id])
 const tick=async()=>{
  try{
   const data=await api('/api/dev/jobs/'+encodeURIComponent(jobId)+'/logs?limit=300')
   jobs.value[app.id]=data.job
   if(data.job.running)timers[app.id]=window.setTimeout(tick,1200)
  }catch(e){error.value=app.name+' 任务日志读取失败：'+String(e)}
 }
 tick()
}
function openApp(app:AppInfo){
 if(app.url)window.open(app.url,'_blank','noopener,noreferrer')
}
function command(task:DevTask){return task.argv.join(' ')}
onMounted(refresh)
onUnmounted(()=>{for(const timer of Object.values(timers))clearTimeout(timer)})
</script>
<template>
 <section class="mb-5">
  <div class="flex flex-wrap justify-between gap-3 items-center mb-4">
   <div><h2 class="text-lg font-semibold">应用</h2><p class="text-xs text-ink-soft mt-1">来自各仓库的 lilyco.app.json 清单 · 只按清单固定参数启动</p></div>
   <button @click="refresh" :disabled="loading" class="btn btn-secondary"><RefreshCw :size="16" :class="loading?'animate-spin':''"/>刷新</button>
  </div>
  <p v-if="error" role="alert" class="alert alert-danger mb-4">{{error}}</p>
  <p v-if="notice" class="alert alert-success mb-4">{{notice}}</p>
  <div v-if="warnings.length" class="alert alert-warning mb-4">
   <div class="flex gap-2 items-center font-medium"><AlertTriangle :size="15"/>清单告警</div>
   <p v-for="warning in warnings" :key="warning" class="mt-1 text-xs">{{warning}}</p>
  </div>
  <p v-if="loading && !apps.length" class="p-10 text-center text-ink-soft">正在读取应用清单…</p>
  <p v-else-if="!apps.length" class="p-10 text-center text-ink-soft">未发现应用清单。把 lilyco.app.json 放进清单目录，或设置 LILYCO_MANAGER_APPS。</p>
  <div v-else class="grid xl:grid-cols-2 gap-4 items-start">
   <article v-for="app in apps" :key="app.id" class="card p-5 flex flex-col gap-4">
    <div class="flex justify-between gap-3 items-start">
     <div class="flex gap-3 items-center min-w-0">
      <div class="p-3 rounded-lg bg-brand-soft text-brand shrink-0"><Package :size="19"/></div>
      <div class="min-w-0"><h3 class="font-semibold truncate">{{app.name}}</h3><p class="font-mono text-xs text-ink-mute truncate">{{app.id}} · {{app.binary}}</p></div>
     </div>
     <span class="badge shrink-0" :class="app.installed?'badge-success':'badge-warning'">{{app.installed?'已安装':'未安装'}}</span>
    </div>
    <p class="text-xs text-ink-soft min-h-8">{{app.description||'暂无描述'}}</p>
    <div class="flex flex-wrap gap-2">
     <span v-if="app.capabilities.run" class="badge">可启动</span>
     <span v-if="app.capabilities.schema" class="badge">--schema</span>
     <span v-if="app.capabilities.mcp" class="badge">MCP</span>
     <span v-if="app.release" class="badge badge-brand">可安装/更新</span>
     <span v-if="app.dev_tasks.length" class="badge badge-brand">开发任务</span>
     <span v-if="app.status?.running" class="badge badge-success">运行中 · PID {{app.status.pid}}</span>
     <span v-else-if="app.status?.exit_code!==null && app.status?.exit_code!==undefined" class="badge">已退出 · {{app.status.exit_code}}</span>
     <span v-else class="badge">未运行</span>
    </div>
    <div class="flex flex-wrap gap-2 mt-auto">
     <button v-if="app.capabilities.run && !app.status?.running" @click="start(app)" :disabled="!app.installed||busy==='start:'+app.id" class="btn btn-brand btn-sm"><Play :size="13"/>启动</button>
     <button v-if="app.status?.running" @click="stop(app)" :disabled="busy==='stop:'+app.id" class="btn btn-danger btn-sm"><Square :size="13"/>停止</button>
     <button v-if="app.status" @click="toggleLogs(app)" class="btn btn-secondary btn-sm"><ScrollText :size="13"/>日志</button>
     <button v-if="app.capabilities.schema" @click="loadSchema(app)" :disabled="!app.installed||busy==='schema:'+app.id" class="btn btn-secondary btn-sm"><Terminal :size="13"/>命令清单</button>
     <button v-if="app.url" @click="openApp(app)" class="btn btn-secondary btn-sm"><ExternalLink :size="13"/>打开</button>
     <button v-if="app.release" @click="checkRelease(app)" :disabled="busy==='release:'+app.id" class="btn btn-secondary btn-sm"><GitBranch :size="13" :class="busy==='release:'+app.id?'animate-pulse':''"/>检查更新</button>
     <button v-if="app.dev_tasks.length" @click="toggleDev(app)" class="btn btn-secondary btn-sm"><Wrench :size="13"/>开发任务<ChevronUp v-if="dev[app.id]" :size="13"/><ChevronDown v-else :size="13"/></button>
     <span v-if="schemaCounts[app.id]!==undefined" class="badge badge-success self-center"><ShieldCheck :size="13"/>{{schemaCounts[app.id]}} 个命令</span>
    </div>

    <div v-if="releases[app.id]" class="rounded-lg border border-line bg-canvas-soft p-3 text-xs flex flex-col gap-2">
     <div class="flex flex-wrap justify-between gap-2 items-center">
      <span class="font-medium text-ink">版本状态</span>
      <span class="badge" :class="releases[app.id].error?'badge-danger':releases[app.id].update_available?'badge-warning':'badge-success'">
       {{releases[app.id].error ? '检查失败' : releases[app.id].update_available ? '有可用更新' : '已是最新'}}
      </span>
     </div>
     <div class="grid sm:grid-cols-2 gap-x-4 gap-y-1 text-ink-soft">
      <span>当前：<span class="font-mono text-ink">{{releases[app.id].current_tag??'未托管安装'}}</span></span>
      <span>最新：<span class="font-mono text-ink">{{releases[app.id].latest_tag??'未知'}}</span></span>
      <span>平台：<span class="font-mono text-ink">{{releases[app.id].platform}}</span></span>
      <span>资产：<span class="font-mono text-ink break-all">{{releases[app.id].asset??'无匹配'}}</span></span>
     </div>
     <p v-if="releases[app.id].error" class="text-danger break-words">{{releases[app.id].error}}</p>
     <div class="flex justify-end gap-2">
      <button v-if="releases[app.id].current_tag" @click="uninstall(app)" :disabled="busy==='uninstall:'+app.id||busy==='install:'+app.id" class="btn btn-danger btn-sm"><Trash2 :size="13"/>{{busy==='uninstall:'+app.id?'卸载中…':'卸载'}}</button>
      <button @click="install(app)" :disabled="busy==='install:'+app.id||busy==='uninstall:'+app.id||!!releases[app.id].error" class="btn btn-brand btn-sm"><Download :size="13"/>{{busy==='install:'+app.id?'安装中…':(releases[app.id].current_tag?'安装更新':'安装')}}</button>
     </div>
    </div>

    <div v-if="dev[app.id]" class="rounded-lg border border-line bg-canvas-soft p-3">
     <div class="flex flex-wrap gap-2">
      <button v-for="task in dev[app.id].tasks" :key="task.name" @click="runTask(app,task)" :disabled="busy==='dev:'+app.id+':'+task.name||jobs[app.id]?.running" class="btn btn-secondary btn-sm" :title="command(task)"><Play :size="12"/>{{task.name}}</button>
      <span v-if="!dev[app.id].tasks.length" class="text-xs text-ink-soft">该应用未配置开发任务</span>
     </div>
     <p v-if="jobs[app.id]" class="text-xs mt-2 flex flex-wrap gap-2 items-center">
      <span class="font-mono text-ink">{{jobs[app.id].task}}</span>
      <span class="badge" :class="jobs[app.id].running?'badge-warning':(jobs[app.id].exit_code===0?'badge-success':'badge-danger')">{{jobs[app.id].running?'运行中':(jobs[app.id].exit_code===0?'完成':'退出 '+jobs[app.id].exit_code)}}</span>
      <span class="text-ink-mute">每次任务在仓库目录执行，参数来自清单</span>
     </p>
     <pre v-if="jobs[app.id]" class="log mt-2 p-3 max-h-64 overflow-auto">{{jobs[app.id].lines.join('\n')||'等待输出…'}}</pre>
    </div>

    <pre v-if="logs[app.id]" class="log p-3 max-h-56 overflow-auto">{{logs[app.id].join('\n')||'暂无日志输出'}}</pre>
   </article>
  </div>
 </section>
</template>
