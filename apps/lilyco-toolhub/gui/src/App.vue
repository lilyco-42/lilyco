<script setup>
import {computed,onBeforeUnmount,onMounted,ref,watch} from 'vue'
import {
  NAlert,NButton,NCard,NCode,NCollapse,NCollapseItem,NConfigProvider,NDataTable,
  NDivider,NEmpty,NFlex,NForm,NFormItem,NGi,NGrid,NInput,NInputGroup,NInputNumber,
  NLayout,NLayoutContent,NLayoutHeader,NLayoutSider,NMenu,NModal,NSelect,
  NSkeleton,NSpace,NStatistic,NSwitch,NTabPane,NTabs,NTag,NText,
  darkTheme,zhCN,dateZhCN,createDiscreteApi
} from 'naive-ui'
import {api,defaults,loadPreview} from './api'

const {message}=createDiscreteApi(['message'],{configProviderProps:{theme:darkTheme}})
const palette={
  common:{primaryColor:'#8299ee',primaryColorHover:'#9db1ff',primaryColorPressed:'#6277d8',
    infoColor:'#8299ee',successColor:'#59c7a9',borderRadius:'10px',
    bodyColor:'#10151f',cardColor:'#17202e',popoverColor:'#1d2838',modalColor:'#17202e'}
}
const tools=ref([])
const status=ref({providers:{},tools:0,local:true})
const loading=ref(true)
const page=ref('tools')
const query=ref('')
const category=ref('all')
const selected=ref(null)
const args=ref({})
const advanced=ref(false)
const jsonArgs=ref('{}')
const running=ref(false)
const confirm=ref(false)
const last=ref(null)
const historyRuns=ref([])
const preview=ref(null)
const previewURL=ref('')
const displayError=ref('')
const theme=ref(darkTheme)
const providers=computed(()=>Object.entries(status.value.providers || {}))
const categories=computed(()=>[...new Set(tools.value.map(t=>t.name.split('__')[0]))].sort())
const visibleTools=computed(()=>tools.value.filter(tool=>{
  const group=tool.name.split('__')[0]
  const needle=query.value.trim().toLowerCase()
  return (category.value==='all'||category.value===group)&&
    (!needle||[tool.name,tool.description||''].join(' ').toLowerCase().includes(needle))
}))
const realToolCount=computed(()=>tools.value.filter(t=>t.name!=='hub_status').length)
const menuOptions=[
  {label:'全部工具',key:'tools'}, {label:'最近运行',key:'history'}, {label:'服务状态',key:'providers'}
]
const alertTag=computed(()=>loading.value?'正在加载':displayError.value?'连接异常':'本地已连接')
const toolGroups=[
  {key:'vision',title:'图像处理',desc:'本地抠图、图片调整与分析'},
  {key:'spine',title:'Spine 素材',desc:'骨骼动画资源元数据检查'},
  {key:'media',title:'创作与视频',desc:'Manim / HTML 动画渲染'},
  {key:'ffmpeg',title:'视频处理',desc:'格式转换与裁剪'},
  {key:'files',title:'文件管理',desc:'本地查找与批处理'},
  {key:'grep',title:'内容搜索',desc:'快速搜索文件内容'},
  {key:'mpkg',title:'记忆工具',desc:'记忆包访问与校验'},
]
function titleFor(name){
  const special={
    'vision__remove-background':'AI 智能抠图',
    'vision__ImageInfo':'图片信息',
    'spine__inspect-skeleton':'Spine 骨骼检查',
    'spine__inspect-atlas':'Spine 图集检查',
    'media__manim_render':'Manim 数学动画',
    'media__html_to_video':'HTML 转视频',
    'hub_status':'工具服务状态',
  }
  return special[name] || name.split('__').slice(1).join('__').replaceAll('-',' ').replaceAll('_',' ')
}
function groupName(name){return toolGroups.find(g=>g.key===name.split('__')[0])?.title||'其他工具'}
function resetEditor(tool) {
  selected.value=tool
  args.value=defaults(tool)
  jsonArgs.value=JSON.stringify(args.value,null,2)
  advanced.value=false
  last.value=null
  displayError.value=''
  if(tool.name==='hub_status') {category.value='all'}
}
function choose(tool){resetEditor(tool);page.value='tools'}
function selectCategory(key){category.value=key;page.value='tools'}
function schemaFields(tool){return Object.entries(tool?.inputSchema?.properties||{}).map(([name,schema])=>({name,schema,required:(tool?.inputSchema?.required||[]).includes(name)}))}
const fields=computed(()=>schemaFields(selected.value))
const risky=computed(()=>selected.value&&/(rename|delete|remove|write|move|replace|reset|exec|shell|install|manim_render|html_to_video)/i.test(selected.value.name) && selected.value.name!=='vision__remove-background')
function argumentsFor(tool){
  const raw=advanced.value?JSON.parse(jsonArgs.value):args.value
  if(!raw||Array.isArray(raw)||typeof raw!=='object')throw Error('参数必须是 JSON 对象')
  const clean={}
  for(const [name,value] of Object.entries(raw)){
    if(value===null||value===undefined||value==='')continue
    clean[name]=value
  }
  for(const name of tool.inputSchema?.required||[]){
    if(clean[name]===undefined)throw Error('请填写必填参数：'+name)
  }
  return clean
}
function formatOutput(result){
  const content=(result?.content||[]).filter(c=>c.type==='text').map(c=>c.text||'').join('\n')
  try{return JSON.stringify(JSON.parse(content),null,2)}catch{return content||'工具已执行，但没有文本结果'}
}
function canPick(field){return /(image|file|path|input|output|out|root|folder|source|dest|target)/i.test(field)}
async function pick(field){
  let mode = /(root|folder|directory)/i.test(field) ? 'folder' :
    /(output|out|dest|target)/i.test(field) ? 'save' : 'file'
  try{
    const response=await api('pick',{mode})
    if(response.path)args.value[field]=response.path
  }catch(e){message.error(e.message||String(e))}
}function startRun(){
  if(!selected.value||running.value)return
  try{
    argumentsFor(selected.value)
    if(risky.value){confirm.value=true;return}
    run(false)
  }catch(error){message.error(error.message||String(error))}
}
async function run(confirmed){
  if(!selected.value||running.value)return
  confirm.value=false
  let values
  try{values=argumentsFor(selected.value)}catch(error){message.error(error.message);return}
  running.value=true
  displayError.value=''
  preview.value=null
  if(previewURL.value)URL.revokeObjectURL(previewURL.value)
  previewURL.value=''
  try{
    const response=await api('run',{name:selected.value.name,arguments:values,confirmed})
    last.value=response.result
    if(response.result.isError){
      message.error('工具执行被拒绝或运行失败')
    } else{
      message.success('已在本机完成执行')
      if(response.run?.preview){
        preview.value=response.run.preview
        try{previewURL.value=await loadPreview(response.run.preview)}catch{ /* file may have been moved */ }
      }
    }
    await refreshHistory()
  }catch(error){
    displayError.value=error.message||String(error)
    message.error(displayError.value)
  }finally{running.value=false}
}
async function refreshHistory(){
  const res=await api('history')
  historyRuns.value=res.runs||[]
}
async function reload(){
  loading.value=true;displayError.value=''
  try{
    const [catalog,system]=await Promise.all([api('tools'),api('status')])
    tools.value=catalog.tools||[]
    status.value=system
    if(!selected.value||!tools.value.some(t=>t.name===selected.value.name)){
      const preferred=tools.value.find(t=>t.name==='vision__remove-background')||tools.value[1]||tools.value[0]
      if(preferred)resetEditor(preferred)
    }
    await refreshHistory()
  }catch(error){displayError.value=error.message||String(error)}
  finally{loading.value=false}
}
function openHistory(){page.value='history';refreshHistory().catch(e=>message.error(String(e)))}
function copyResult(){if(last.value)navigator.clipboard.writeText(formatOutput(last.value)).then(()=>message.success('结果已复制')).catch(()=>message.error('复制失败'))}
const historyColumns=[
  {title:'时间',key:'at',render(row){return new Date(row.at).toLocaleString('zh-CN')}},
  {title:'工具',key:'name',ellipsis:true},
  {title:'状态',key:'ok',render(row){return row.ok?'成功':'失败'}},
]
onMounted(reload)
onBeforeUnmount(()=>{if(previewURL.value)URL.revokeObjectURL(previewURL.value)})
watch(advanced,v=>{if(v)jsonArgs.value=JSON.stringify(args.value,null,2);else{try{args.value=JSON.parse(jsonArgs.value)}catch{}}})
</script>

<template>
<n-config-provider :theme="theme" :theme-overrides="palette" :locale="zhCN" :date-locale="dateZhCN">
<n-layout class="app-layout" has-sider>
  <n-layout-sider class="app-sider" bordered :width="252" collapse-mode="width" :collapsed-width="0" show-trigger="bar">
    <div class="brand">
      <div class="brand-overline">LYCO · LOCAL WORKSPACE</div>
      <div class="brand-title">Lilyco Tool Hub</div>
      <div class="brand-subtitle">一个入口，连接所有本地 AI 工具。<br/>Human + Agent Workspace</div>
    </div>
    <div class="sider-actions">
      <n-tag :type="displayError?'error':'success'" size="small" round>{{alertTag}}</n-tag>
      <n-text depth="3" style="display:block;margin-top:10px;font-size:12px">客户端执行 · 无云端推理</n-text>
    </div>
    <n-menu :value="page" :options="menuOptions" @update:value="v=>v==='history'?openHistory():page=v"/>
    <n-divider style="margin:14px 0"/>
    <div class="sider-lower">
      <n-text depth="3" strong style="font-size:11px;letter-spacing:.12em">工具分类</n-text>
      <n-space vertical style="margin-top:12px" :size="4">
        <n-button block text style="justify-content:flex-start" @click="selectCategory('all')">全部能力 · {{realToolCount}}</n-button>
        <n-button v-for="group in toolGroups" :key="group.key" block text style="justify-content:flex-start"
          @click="selectCategory(group.key)">{{group.title}} · {{tools.filter(t=>t.name.startsWith(group.key+'__')).length}}</n-button>
      </n-space>
      <n-divider/>
      <n-text depth="3" style="font-size:12px;line-height:1.7">仅监听本机 127.0.0.1<br/>执行权限继承 Lilyco MCP</n-text>
    </div>
  </n-layout-sider>
  <n-layout>
    <n-layout-header bordered style="padding:13px 28px;background:#151d29">
      <n-flex align="center" justify="space-between" :wrap="true">
        <n-space align="center" :size="12">
          <n-tag type="info" size="small" :bordered="false">LOCAL</n-tag>
          <n-text strong>工具工作台</n-text>
          <n-text depth="3" style="font-size:12px">{{realToolCount}} 个可调用工具 · 实时注册表</n-text>
        </n-space>
        <n-space align="center">
          <n-button size="small" secondary :loading="loading" @click="reload">刷新工具</n-button>
          <n-button size="small" quaternary @click="page='providers'">连接详情</n-button>
        </n-space>
      </n-flex>
      <div class="mobile-nav">
      <n-space :size="6">
        <n-button size="small" :type="page==='tools'?'primary':'default'" @click="page='tools'">全部工具</n-button>
        <n-button size="small" :type="page==='history'?'primary':'default'" @click="openHistory">最近运行</n-button>
        <n-button size="small" :type="page==='providers'?'primary':'default'" @click="page='providers'">状态</n-button>
      </n-space>
      </div>
    </n-layout-header>
    <n-layout-content content-style="background:#10151f;min-height:calc(100vh - 66px)">
      <main class="content">
        <n-alert v-if="displayError" type="error" closable style="margin-bottom:20px" @close="displayError=''">{{displayError}}</n-alert>
        <n-space v-if="page==='tools'&&loading" vertical><n-skeleton text :repeat="4"/><n-skeleton height="160px"/></n-space>
        <template v-else-if="page==='tools'">
          <div class="title-row">
            <div><h1 class="page-title">工具中心</h1><p class="page-description">查找、配置和运行本地工具。与 AI 使用相同的 MCP 能力。</p></div>
            <n-space align="center">
              <n-tag type="info" size="medium" :bordered="false">{{realToolCount}} 个工具</n-tag>
              <n-tag type="success" size="medium" :bordered="false">{{providers.filter(([,state])=>String(state).startsWith('ready')).length}} 个在线提供者</n-tag>
            </n-space>
          </div>
          <n-grid cols="1 l:5" :x-gap="18" :y-gap="20" responsive="screen">
            <n-gi span="1 l:3">
              <n-card :bordered="false" title="发现工具">
                <template #header-extra><n-text depth="3" style="font-size:12px">统一搜索与分类</n-text></template>
                <n-space vertical :size="14">
                  <n-input v-model:value="query" placeholder="搜索工具名称、能力或参数..." clearable size="large"/>
                  <n-space :size="6" :wrap="true">
                    <n-button size="small" :type="category==='all'?'primary':'default'" :secondary="category!=='all'" @click="category='all'">全部</n-button>
                    <n-button v-for="group in toolGroups" :key="group.key" size="small" :type="category===group.key?'primary':'default'" :secondary="category!==group.key" @click="category=group.key">{{group.title}}</n-button>
                  </n-space>
                  <n-grid v-if="visibleTools.length" cols="1 m:2" :x-gap="10" :y-gap="10" responsive="screen">
                    <n-gi v-for="tool in visibleTools" :key="tool.name">
                      <n-card size="small" class="tool-tile" :class="{selected:selected?.name===tool.name}" :bordered="true">
                        <n-flex justify="space-between" align="center" style="margin-bottom:10px">
                          <n-tag size="tiny" :bordered="false">{{groupName(tool.name)}}</n-tag>
                          <n-text depth="3" style="font-size:10px">{{tool.name.split('__')[0]}}</n-text>
                        </n-flex>
                        <div class="tool-tile-title">{{titleFor(tool.name)}}</div>
                        <p class="tool-description">{{tool.description||'本地 MCP 工具'}}</p>
                        <n-button block size="small" :type="selected?.name===tool.name?'primary':'default'"
                          :secondary="selected?.name!==tool.name" @click="choose(tool)">{{selected?.name===tool.name?'当前选中':'打开工具'}}</n-button>
                      </n-card>
                    </n-gi>
                  </n-grid>
                  <n-empty v-else description="没有匹配的工具，尝试更换关键词"/>
                </n-space>
              </n-card>
            </n-gi>
            <n-gi span="1 l:2">
              <n-space vertical :size="16">
                <n-card :bordered="false" :title="selected?titleFor(selected.name):'选择工具'">
                  <template #header-extra><n-tag v-if="selected" size="small" type="info">MCP</n-tag></template>
                  <template v-if="selected">
                    <div class="panel-head"><n-text depth="3" style="font-size:12px">{{selected.name}}</n-text>
                      <p class="tool-description" style="display:block;-webkit-line-clamp:unset">{{selected.description}}</p>
                    </div>
                    <n-alert v-if="risky" type="warning" style="margin-bottom:14px">这项操作可能修改本地文件，需要确认。下游安全策略仍然生效。</n-alert>
                    <n-space vertical :size="12">
                      <n-form label-placement="top" label-width="auto">
                        <template v-if="!advanced">
                          <n-form-item v-for="field in fields" :key="field.name"
                            :label="field.name+(field.required?' *':'')" :feedback="field.schema.description||undefined">
                            <n-select v-if="field.schema.enum" v-model:value="args[field.name]" :options="field.schema.enum.map(v=>({label:v,value:v}))" clearable/>
                            <n-switch v-else-if="field.schema.type==='boolean'" v-model:value="args[field.name]"/>
                            <n-input-number v-else-if="['number','integer'].includes(field.schema.type)" v-model:value="args[field.name]" :min="field.schema.minimum" :max="field.schema.maximum" style="width:100%" clearable/>
                            <n-input-group v-else>
                              <n-input v-model:value="args[field.name]" clearable :placeholder="field.schema.description||'输入 '+field.name"/>
                              <n-button v-if="canPick(field.name)" @click="pick(field.name)">浏览</n-button>
                            </n-input-group>
                          </n-form-item>
                          <n-empty v-if="fields.length===0" size="small" description="该工具不需要参数"/>
                        </template>
                        <n-form-item v-else label="JSON 参数（高级）">
                          <n-input v-model:value="jsonArgs" type="textarea" :rows="10" class="mono" placeholder="{}" />
                        </n-form-item>
                      </n-form>
                      <n-flex justify="space-between" align="center">
                        <n-space align="center" :size="8"><n-switch v-model:value="advanced" size="small"/><n-text depth="3" style="font-size:12px">JSON 模式</n-text></n-space>
                        <n-button type="primary" :loading="running" :disabled="running" @click="startRun">在本机运行</n-button>
                      </n-flex>
                    </n-space>
                  </template>
                  <n-empty v-else description="在左侧选择一个工具"/>
                </n-card>
                <n-card :bordered="false" title="运行结果">
                  <template #header-extra><n-button v-if="last" quaternary size="tiny" @click="copyResult">复制结果</n-button></template>
                  <n-space vertical :size="10" v-if="last">
                    <n-alert :type="last.isError?'error':'success'">{{last.isError?'运行被拒绝或失败':'工具运行完成'}}</n-alert>
                    <div class="result-text">{{formatOutput(last)}}</div>
                    <template v-if="previewURL">
                      <n-divider style="margin:4px 0"/>
                      <video v-if="preview&&historyRuns.find(x=>x.preview===preview)?.name?.includes('video')" class="preview-video" :src="previewURL" controls/>
                      <video v-else-if="selected?.name.startsWith('media__') || selected?.name.startsWith('ffmpeg__')" class="preview-video" :src="previewURL" controls/>
                      <img v-else class="preview-image" :src="previewURL" alt="本地工具输出预览"/>
                    </template>
                  </n-space>
                  <n-empty v-else size="small" description="执行工具后，可在这里查看输出和预览"/>
                </n-card>
              </n-space>
            </n-gi>
          </n-grid>
        </template>
        <template v-else-if="page==='history'">
          <div class="title-row"><div><h1 class="page-title">运行记录</h1><p class="page-description">仅保留本次进程中的最近 100 条操作，不上传云端、不存储工具入参。</p></div><n-button secondary @click="refreshHistory">刷新</n-button></div>
          <n-card :bordered="false"><n-data-table :columns="historyColumns" :data="historyRuns" :pagination="{pageSize:12}" :single-line="false"/><n-empty v-if="historyRuns.length===0" description="当前尚无运行记录"/></n-card>
        </template>
        <template v-else>
          <div class="title-row"><div><h1 class="page-title">服务与设备</h1><p class="page-description">Lilyco MCP 提供者的真实连接状态。所有计算均在用户设备执行。</p></div></div>
          <n-grid cols="1 m:3" :x-gap="14" :y-gap="14" responsive="screen">
            <n-gi><n-card title="已发现工具" :bordered="false"><n-statistic :value="realToolCount"/></n-card></n-gi>
            <n-gi><n-card title="在线工具组" :bordered="false"><n-statistic :value="providers.filter(([,value])=>String(value).startsWith('ready')).length"/></n-card></n-gi>
            <n-gi><n-card title="计算位置" :bordered="false"><n-tag type="success" size="large">用户本机</n-tag></n-card></n-gi>
          </n-grid>
          <n-card title="MCP 提供者" :bordered="false" style="margin-top:18px">
            <n-space vertical :size="12">
              <n-flex v-for="[name,state] in providers" :key="name" justify="space-between" align="center">
                <n-text strong>{{name}}</n-text><n-tag :type="String(state).startsWith('ready')?'success':'error'">{{state}}</n-tag>
              </n-flex>
              <n-empty v-if="providers.length===0" description="未检测到提供者"/>
            </n-space>
          </n-card>
          <n-alert type="info" title="安全说明" style="margin-top:18px">GUI 仅在 127.0.0.1 监听。工具调用需要会话令牌与同源校验；调用安全边界仍由 Lilyco MCP 执行。请勿把本地网页端口公开映射到公网。</n-alert>
        </template>
        <n-flex v-if="page!=='tools'" style="margin-top:24px"><n-button @click="page='tools'">返回工具中心</n-button></n-flex>
      </main>
    </n-layout-content>
  </n-layout>
</n-layout>
<n-modal v-model:show="confirm" preset="dialog" title="确认执行本地操作" positive-text="确认运行" negative-text="取消"
  @positive-click="()=>run(true)" @negative-click="confirm=false">
  工具 {{selected?.name}} 可能修改电脑中的文件。确认你理解工具参数和操作范围，Lilyco 下游安全限制不会被解除。
</n-modal>
</n-config-provider>
</template>
