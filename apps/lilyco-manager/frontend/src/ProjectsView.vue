<script setup lang="ts">
import {computed,onMounted,ref} from 'vue'
import {FolderKanban,Plus,RefreshCw,Target,Trash2} from 'lucide-vue-next'
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
import Textarea from './components/ui/Textarea.vue'

type ProjectStatus='active'|'paused'|'done'
type PlanStatus='draft'|'doing'|'done'
type Plan={id:number;project_id:number;title:string;detail:string;status:PlanStatus;created_at:string;updated_at:string}
type Project={id:number;name:string;description:string;status:ProjectStatus;created_at:string;updated_at:string;plans:Plan[]}
type Draft={title:string;detail:string;status:PlanStatus}

const props=defineProps<{token:string;readOnly?:boolean}>()

const projectStatusLabel:Record<ProjectStatus,string>={active:'进行中',paused:'已暂停',done:'已完成'}
const planStatusLabel:Record<PlanStatus,string>={draft:'草稿',doing:'进行中',done:'已完成'}
const projectStatusVariant:Record<ProjectStatus,'success'|'warning'|'default'|'brand'>={active:'success',paused:'warning',done:'brand'}
const planStatusVariant:Record<PlanStatus,'default'|'warning'|'success'|'brand'>={draft:'default',doing:'warning',done:'success'}

const projects=ref<Project[]>([])
const loading=ref(false),busy=ref(''),error=ref(''),notice=ref('')
const draftName=ref(''),draftDescription=ref(''),draftStatus=ref<ProjectStatus>('active')
const drafts=ref<Record<number,Draft>>({})
const editingProject=ref<number|null>(null)
const projectEdit=ref({name:'',description:''})
const editingPlan=ref<number|null>(null)
const planEdit=ref<Draft>({title:'',detail:'',status:'draft'})
const confirmingProject=ref<number|null>(null)
const confirmingPlan=ref<number|null>(null)

const planCount=computed(()=>projects.value.reduce((total,project)=>total+project.plans.length,0))

async function api(path:string,method='GET',body?:unknown){
 const response=await fetch(path,{
  method,
  headers:{'x-lilyco-token':props.token,'content-type':'application/json'},
  body:body===undefined?undefined:JSON.stringify(body),
 })
 if(!response.ok)throw new Error('HTTP '+response.status)
 return response.json()
}

function clearConfirmations(){
 confirmingProject.value=null
 confirmingPlan.value=null
}

async function load(){
 loading.value=true;error.value=''
 try{
  const data=await api('/api/projects')
  projects.value=(data.projects??[]) as Project[]
  for(const project of projects.value){
   if(!drafts.value[project.id])drafts.value[project.id]={title:'',detail:'',status:'draft'}
  }
 }catch(e){error.value='项目加载失败：'+String(e)}
 finally{loading.value=false}
}

async function createProject(){
 const name=draftName.value.trim()
 if(!name)return
 busy.value='create';error.value='';notice.value='';clearConfirmations()
 try{
  const data=await api('/api/projects','POST',{name,description:draftDescription.value.trim(),status:draftStatus.value})
  draftName.value='';draftDescription.value='';draftStatus.value='active'
  notice.value=`已创建项目「${data.project.name}」`
  await load()
 }catch(e){error.value='创建项目失败：'+String(e)}
 finally{busy.value=''}
}

function startProjectEdit(project:Project){
 editingProject.value=project.id
 projectEdit.value={name:project.name,description:project.description}
 clearConfirmations()
}

async function saveProject(project:Project){
 const name=projectEdit.value.name.trim()
 if(!name)return
 busy.value='project-'+project.id;error.value='';notice.value=''
 try{
  const data=await api('/api/projects/'+project.id,'PATCH',{name,description:projectEdit.value.description.trim()})
  editingProject.value=null
  notice.value=`已保存项目「${data.project.name}」`
  await load()
 }catch(e){error.value='保存项目失败：'+String(e)}
 finally{busy.value=''}
}

async function setProjectStatus(project:Project,status:ProjectStatus){
 if(status===project.status)return
 busy.value='project-'+project.id;error.value='';notice.value=''
 try{
  await api('/api/projects/'+project.id,'PATCH',{status})
  notice.value=`项目「${project.name}」状态已改为${projectStatusLabel[status]}`
  await load()
 }catch(e){error.value='更新项目状态失败：'+String(e)}
 finally{busy.value=''}
}

function askDeleteProject(project:Project){
 if(confirmingProject.value===project.id)void removeProject(project)
 else{confirmingProject.value=project.id;confirmingPlan.value=null;notice.value=''}
}

async function removeProject(project:Project){
 busy.value='project-'+project.id;error.value='';notice.value='';clearConfirmations()
 try{
  await api('/api/projects/'+project.id,'DELETE')
  editingProject.value=null
  notice.value=`已删除项目「${project.name}」及其方案`
  await load()
 }catch(e){error.value='删除项目失败：'+String(e)}
 finally{busy.value=''}
}

async function addPlan(project:Project){
 const draft=drafts.value[project.id]
 if(!draft||!draft.title.trim())return
 busy.value='plan-'+project.id;error.value='';notice.value='';clearConfirmations()
 try{
  const data=await api('/api/projects/'+project.id+'/plans','POST',{
   title:draft.title.trim(),
   detail:draft.detail.trim(),
   status:draft.status,
  })
  drafts.value[project.id]={title:'',detail:'',status:'draft'}
  notice.value=`已添加方案「${data.plan.title}」`
  await load()
 }catch(e){error.value='添加方案失败：'+String(e)}
 finally{busy.value=''}
}

function startPlanEdit(plan:Plan){
 editingPlan.value=plan.id
 planEdit.value={title:plan.title,detail:plan.detail,status:plan.status}
 clearConfirmations()
}

async function savePlan(plan:Plan){
 const title=planEdit.value.title.trim()
 if(!title)return
 busy.value='plan-'+plan.id;error.value='';notice.value=''
 try{
  const data=await api('/api/plans/'+plan.id,'PATCH',{
   title,
   detail:planEdit.value.detail.trim(),
   status:planEdit.value.status,
  })
  editingPlan.value=null
  notice.value=`已保存方案「${data.plan.title}」`
  await load()
 }catch(e){error.value='保存方案失败：'+String(e)}
 finally{busy.value=''}
}

async function setPlanStatus(plan:Plan,status:PlanStatus){
 if(status===plan.status)return
 busy.value='plan-'+plan.id;error.value='';notice.value=''
 try{
  await api('/api/plans/'+plan.id,'PATCH',{status})
  notice.value=`方案「${plan.title}」状态已改为${planStatusLabel[status]}`
  await load()
 }catch(e){error.value='更新方案状态失败：'+String(e)}
 finally{busy.value=''}
}

function askDeletePlan(plan:Plan){
 if(confirmingPlan.value===plan.id)void removePlan(plan)
 else{confirmingPlan.value=plan.id;confirmingProject.value=null;notice.value=''}
}

async function removePlan(plan:Plan){
 busy.value='plan-'+plan.id;error.value='';notice.value='';clearConfirmations()
 try{
  await api('/api/plans/'+plan.id,'DELETE')
  if(editingPlan.value===plan.id)editingPlan.value=null
  notice.value=`已删除方案「${plan.title}」`
  await load()
 }catch(e){error.value='删除方案失败：'+String(e)}
 finally{busy.value=''}
}

onMounted(load)
</script>

<template>
 <div>
  <div class="mb-5 flex flex-wrap items-start justify-between gap-3">
   <div>
    <h2 class="text-lg font-semibold">项目 / 方案</h2>
    <p class="mt-1 text-xs text-ink-soft">项目承载目标与状态，方案是可以逐条推进的执行单元。数据写入本机 SQLite，面板只与本机回环接口通信。</p>
    <p v-if="readOnly" class="mt-1 text-xs text-ink-mute">只读模式：创建、编辑、状态推进与删除已禁用。</p>
   </div>
   <div class="flex flex-wrap items-center gap-2">
    <Badge variant="brand" data-testid="project-count"><FolderKanban :size="12"/>{{projects.length}} 个项目</Badge>
    <Badge data-testid="plan-count"><Target :size="12"/>{{planCount}} 个方案</Badge>
    <Button size="sm" variant="secondary" :disabled="loading" @click="load"><RefreshCw :size="13" :class="loading?'animate-spin':''"/>刷新</Button>
   </div>
  </div>

  <Alert v-if="error" variant="danger" title="操作失败" class="mb-4">{{error}}</Alert>
  <p v-if="notice" aria-live="polite" class="mb-4 text-xs text-ink-soft">{{notice}}</p>

  <Card class="mb-5">
   <CardHeader>
    <CardTitle>新建项目</CardTitle>
    <CardDescription>名称必填；描述与初始状态可留空，默认「进行中」。</CardDescription>
   </CardHeader>
   <CardContent>
    <form class="grid gap-3 sm:grid-cols-2" @submit.prevent="createProject">
     <div class="flex flex-col gap-1.5">
      <Label for="project-name">项目名称</Label>
      <Input id="project-name" v-model="draftName" :disabled="readOnly" placeholder="例如：管理器 v0.3" autocomplete="off"/>
     </div>
     <div class="flex flex-col gap-1.5">
      <Label for="project-status">初始状态</Label>
      <Select id="project-status" v-model="draftStatus" :disabled="readOnly">
       <option v-for="(label,value) in projectStatusLabel" :key="value" :value="value">{{label}}</option>
      </Select>
     </div>
     <div class="flex flex-col gap-1.5 sm:col-span-2">
      <Label for="project-description">描述</Label>
      <Textarea id="project-description" v-model="draftDescription" :disabled="readOnly" :rows="2" placeholder="目标、范围或验收标准（可留空）"/>
     </div>
     <div class="sm:col-span-2">
      <Button type="submit" :disabled="readOnly||busy==='create'||!draftName.trim()"><Plus :size="14"/>创建项目</Button>
     </div>
    </form>
   </CardContent>
  </Card>

  <p v-if="loading&&projects.length===0" class="py-8 text-center text-sm text-ink-soft">正在读取项目…</p>
  <p v-else-if="projects.length===0" data-testid="projects-empty" class="py-8 text-center text-sm text-ink-soft">暂无项目。先创建一个项目，再为它添加方案。</p>

  <div v-else class="flex flex-col gap-4">
   <Card v-for="project in projects" :key="project.id" data-testid="project-card">
    <CardHeader>
     <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="min-w-0">
       <CardTitle class="break-words">{{project.name}}</CardTitle>
       <CardDescription class="mt-1 whitespace-pre-line break-words">{{project.description||'暂无描述'}}</CardDescription>
      </div>
      <div class="flex flex-wrap items-center gap-2">
       <Badge :variant="projectStatusVariant[project.status]" data-testid="project-status-badge">{{projectStatusLabel[project.status]}}</Badge>
       <Select class="w-[124px]" :aria-label="`项目状态：${project.name}`" :disabled="readOnly" :model-value="project.status"
        @update:model-value="value=>setProjectStatus(project,value as ProjectStatus)">
        <option v-for="(label,value) in projectStatusLabel" :key="value" :value="value">{{label}}</option>
       </Select>
       <Button size="sm" variant="secondary" data-testid="project-edit" :disabled="readOnly" @click="startProjectEdit(project)">编辑</Button>
       <Button size="sm" variant="destructive" data-testid="project-delete" :disabled="readOnly||busy==='project-'+project.id" @click="askDeleteProject(project)">
        <Trash2 :size="13"/>{{confirmingProject===project.id?'确认删除':'删除'}}
       </Button>
      </div>
     </div>
    </CardHeader>
    <CardContent>
     <form v-if="editingProject===project.id" class="grid gap-3 rounded-md border border-border p-3 sm:grid-cols-2" @submit.prevent="saveProject(project)">
      <div class="flex flex-col gap-1.5 sm:col-span-2">
       <Label :for="`project-edit-name-${project.id}`">项目名称</Label>
       <Input :id="`project-edit-name-${project.id}`" v-model="projectEdit.name" autocomplete="off"/>
      </div>
      <div class="flex flex-col gap-1.5 sm:col-span-2">
       <Label :for="`project-edit-description-${project.id}`">描述</Label>
       <Textarea :id="`project-edit-description-${project.id}`" v-model="projectEdit.description" :rows="2"/>
      </div>
      <div class="flex gap-2 sm:col-span-2">
       <Button type="submit" size="sm" :disabled="readOnly||busy==='project-'+project.id||!projectEdit.name.trim()">保存</Button>
       <Button type="button" size="sm" variant="ghost" @click="editingProject=null">取消</Button>
      </div>
     </form>

     <Separator class="my-4"/>
     <div class="mb-2 flex items-center justify-between gap-2">
      <h3 class="text-sm font-semibold">方案</h3>
      <Badge>{{project.plans.length}} 个</Badge>
     </div>
     <p v-if="project.plans.length===0" class="py-2 text-xs text-muted-foreground">还没有方案。</p>

     <div v-for="plan in project.plans" :key="plan.id" class="mt-2 rounded-md border border-border p-3" data-testid="plan-row">
      <form v-if="editingPlan===plan.id" class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_150px]" @submit.prevent="savePlan(plan)">
       <div class="flex flex-col gap-1.5">
        <Label :for="`plan-edit-title-${plan.id}`">方案标题</Label>
        <Input :id="`plan-edit-title-${plan.id}`" v-model="planEdit.title" autocomplete="off"/>
       </div>
       <div class="flex flex-col gap-1.5">
        <Label :for="`plan-edit-status-${plan.id}`">状态</Label>
        <Select :id="`plan-edit-status-${plan.id}`" v-model="planEdit.status">
         <option v-for="(label,value) in planStatusLabel" :key="value" :value="value">{{label}}</option>
        </Select>
       </div>
       <div class="flex flex-col gap-1.5 sm:col-span-2">
        <Label :for="`plan-edit-detail-${plan.id}`">说明</Label>
        <Textarea :id="`plan-edit-detail-${plan.id}`" v-model="planEdit.detail" :rows="2"/>
       </div>
       <div class="flex gap-2 sm:col-span-2">
        <Button type="submit" size="sm" :disabled="readOnly||busy==='plan-'+plan.id||!planEdit.title.trim()">保存方案</Button>
        <Button type="button" size="sm" variant="ghost" @click="editingPlan=null">取消</Button>
       </div>
      </form>
      <div v-else class="flex flex-wrap items-start justify-between gap-2">
       <div class="min-w-0">
        <p class="break-words text-[13px] font-semibold">{{plan.title}}</p>
        <p v-if="plan.detail" class="mt-1 whitespace-pre-line break-words text-[11px] text-muted-foreground">{{plan.detail}}</p>
       </div>
       <div class="flex flex-wrap items-center gap-2">
        <Badge :variant="planStatusVariant[plan.status]" data-testid="plan-status-badge">{{planStatusLabel[plan.status]}}</Badge>
        <Select class="w-[116px]" :aria-label="`方案状态：${plan.title}`" :disabled="readOnly" :model-value="plan.status"
         @update:model-value="value=>setPlanStatus(plan,value as PlanStatus)">
         <option v-for="(label,value) in planStatusLabel" :key="value" :value="value">{{label}}</option>
        </Select>
        <Button size="sm" variant="secondary" data-testid="plan-edit" :disabled="readOnly" @click="startPlanEdit(plan)">编辑</Button>
        <Button size="sm" variant="destructive" data-testid="plan-delete" :disabled="readOnly||busy==='plan-'+plan.id" @click="askDeletePlan(plan)">
         <Trash2 :size="13"/>{{confirmingPlan===plan.id?'确认删除':'删除'}}
        </Button>
       </div>
      </div>
     </div>

     <form class="mt-3 grid gap-2 sm:grid-cols-[minmax(0,1fr)_150px_auto]" @submit.prevent="addPlan(project)">
      <Input v-model="drafts[project.id].title" :disabled="readOnly" :aria-label="`新方案标题：${project.name}`" placeholder="新方案标题"/>
      <Select v-model="drafts[project.id].status" :disabled="readOnly" :aria-label="`新方案状态：${project.name}`">
       <option v-for="(label,value) in planStatusLabel" :key="value" :value="value">{{label}}</option>
      </Select>
      <Button type="submit" :disabled="readOnly||busy==='plan-'+project.id||!drafts[project.id].title.trim()"><Plus :size="14"/>添加方案</Button>
      <div class="sm:col-span-3">
       <Textarea v-model="drafts[project.id].detail" :disabled="readOnly" :rows="2" :aria-label="`新方案说明：${project.name}`" placeholder="说明（可留空）"/>
      </div>
     </form>
    </CardContent>
   </Card>
  </div>
 </div>
</template>
