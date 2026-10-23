<script setup lang="ts">
import { ref, computed, watch, onBeforeUnmount, onMounted } from 'vue'
import { zipSync, strToU8 } from 'fflate'
import { webGpuAvailable, exposeOnGpu } from './webgpu'
import { exposureOnWasm, wasmEmbedded } from './wasm-kernels'
import { ImagePlus, Download, X, Cpu, Zap, CheckCircle2, AlertCircle, Layers, ExternalLink } from 'lucide-vue-next'

type Entry = { id:number; name:string; source:File; preview:string }
const entries=ref<Entry[]>([])
const size=ref(1200)
const background=ref<'white'|'gray'|'transparent'>('white')
const format=ref<'png'|'jpeg'|'webp'>('png')
const margin=ref(8)
const skuPrefix=ref('SKU')
const exposure=ref(1)
const gpuSupport=ref(false)
const useGpu=ref(true)
const useWasm=ref(true)
const executionBackend=ref('')
const isBusy=ref(false)
const progress=ref('')
const status=ref('')
const nextId=ref(0)
watch(background,b=>{if(b==='transparent'&&format.value==='jpeg')format.value='png'})
const sample= computed(()=>entries.value[0]?.preview)
onMounted(async()=>{gpuSupport.value=await webGpuAvailable()})
onBeforeUnmount(()=>entries.value.forEach(e=>URL.revokeObjectURL(e.preview)))

function addFiles(event:Event){
 const input=event.target as HTMLInputElement
 const selected=Array.from(input.files??[])
 for(const file of selected){
   if(!['image/png','image/jpeg','image/webp'].includes(file.type)){status.value='仅支持 PNG / JPEG / WebP 文件';continue}
   if(file.size>25*1024*1024){status.value='单张图片上限 25MB';continue}
   if(entries.value.length>=50){status.value='每批最多 50 张';break}
   entries.value.push({id:nextId.value++,name:file.name,source:file,preview:URL.createObjectURL(file)})
 }
 input.value=''
}
function remove(id:number){
 const i=entries.value.findIndex(e=>e.id===id)
 if(i>=0){URL.revokeObjectURL(entries.value[i]!.preview);entries.value.splice(i,1)}
}
function safeName(name:string){return name.replace(/[^a-zA-Z0-9_-]/g,'_').slice(0,35)||'product'}
function toBlob(canvas:HTMLCanvasElement,type:string):Promise<Blob>{
 return new Promise((resolve,reject)=>canvas.toBlob(b=>b?resolve(b):reject(Error('图片编码失败')),type,0.93))
}
async function loadImage(url:string):Promise<ImageBitmap>{
 return createImageBitmap(await fetch(url).then(r=>r.blob()))
}
function bounds(image:ImageBitmap):[number,number,number,number]{
 const scratch=document.createElement('canvas')
 const ratio=Math.min(1,512/Math.max(image.width,image.height))
 scratch.width=Math.max(1,Math.round(image.width*ratio));scratch.height=Math.max(1,Math.round(image.height*ratio))
 const ctx=scratch.getContext('2d',{willReadFrequently:true})!
 ctx.drawImage(image,0,0,scratch.width,scratch.height)
 const rgba=ctx.getImageData(0,0,scratch.width,scratch.height).data
 let l=scratch.width,t=scratch.height,r=-1,b=-1
 for(let y=0;y<scratch.height;y++)for(let x=0;x<scratch.width;x++){
  if(rgba[(y*scratch.width+x)*4+3]>12){l=Math.min(l,x);t=Math.min(t,y);r=Math.max(r,x);b=Math.max(b,y)}
 }
 return r<0?[0,0,image.width,image.height]:
  [l/ratio,t/ratio,Math.max(1,(r-l+1)/ratio),Math.max(1,(b-t+1)/ratio)]
}
async function renderOne(entry:Entry):Promise<Uint8Array>{
 const bitmap=await loadImage(entry.preview)
 try {
  const canvas=document.createElement('canvas');canvas.width=canvas.height=size.value
  const ctx=canvas.getContext('2d',{willReadFrequently:true})!
  if(background.value!=='transparent'){
   ctx.fillStyle=background.value==='white'?'#ffffff':'#f0f1f3'
   ctx.fillRect(0,0,size.value,size.value)
  }
  const [sx,sy,sw,sh]=bounds(bitmap)
  const available=size.value*(1-2*margin.value/100)
  const scale=Math.min(available/sw,available/sh)
  const dw=sw*scale,dh=sh*scale
  ctx.drawImage(bitmap,sx,sy,sw,sh,(size.value-dw)/2,(size.value-dh)/2,dw,dh)
  if(exposure.value!==1){
   const data=ctx.getImageData(0,0,canvas.width,canvas.height)
   let modified=false
   if(useGpu.value&&gpuSupport.value){
    try{ctx.putImageData(await exposeOnGpu(data,exposure.value),0,0);modified=true;executionBackend.value='WebGPU'}
    catch{status.value='WebGPU 失败，正在尝试 WebAssembly / CPU 回退'}
   }
   if(!modified&&wasmEmbedded&&useWasm.value){
    try{ctx.putImageData(await exposureOnWasm(data,exposure.value),0,0);modified=true;executionBackend.value='WebAssembly'}
    catch{status.value='WebAssembly 内核执行失败，已回退浏览器 CPU'}
   }
   if(!modified){
    executionBackend.value='Canvas2D CPU'
    for(let i=0;i<data.data.length;i+=4)for(let c=0;c<3;c++)data.data[i+c]=Math.min(255,data.data[i+c]!*exposure.value)
    ctx.putImageData(data,0,0)
   }
  }
  const mime=format.value==='jpeg'?'image/jpeg':format.value==='webp'?'image/webp':'image/png'
  const blob=await toBlob(canvas,mime)
  return new Uint8Array(await blob.arrayBuffer())
 }finally{bitmap.close()}
}
function download(bytes:Uint8Array,filename:string,mime='application/octet-stream'){
 const blob=new Blob([bytes as BlobPart],{type:mime})
 const url=URL.createObjectURL(blob)
 const a=document.createElement('a');a.href=url;a.download=filename;a.click()
 setTimeout(()=>URL.revokeObjectURL(url),30000)
}
async function exportImages(){
 if(isBusy.value||!entries.value.length)return
 isBusy.value=true;status.value='';progress.value='准备处理...'
 try{
  const files:Record<string,Uint8Array>={}
  const manifest:{source:string;file:string}[]=[]
  for(const [i,e] of entries.value.entries()){
   progress.value=`正在处理 ${i+1}/${entries.value.length}：${e.name}`
   await new Promise<void>(r=>setTimeout(r,0))
   const filename=`${safeName(skuPrefix.value)}_${String(i+1).padStart(3,'0')}_${safeName(e.name.replace(/\.[^.]+$/,''))}.${format.value==='jpeg'?'jpg':format.value}`
   files[filename]=await renderOne(e)
   manifest.push({source:e.name,file:filename})
  }
  files['delivery-manifest.json']=strToU8(JSON.stringify({format:format.value,pixels:size.value,background:background.value,generated_at:new Date().toISOString(),items:manifest},null,2))
  progress.value='正在打包...'
  download(zipSync(files,{level:0}),`${safeName(skuPrefix.value)}-commerce-assets.zip`,'application/zip')
  progress.value=`已导出 ${entries.value.length} 张图片（ZIP）`
 }catch(e){status.value='处理失败：'+String(e);progress.value=''}
 finally{isBusy.value=false}
}
</script>
<template>
<div class="space-y-5">
 <section class="rounded-2xl border border-indigo-400/20 bg-gradient-to-r from-[#172945] to-[#111b2a] p-6">
  <p class="uppercase text-xs tracking-widest text-indigo-300">LOCAL-FIRST · COMMERCE STUDIO</p>
  <h2 class="text-2xl font-semibold mt-2">跨境电商商品图工作台</h2>
  <p class="text-sm text-slate-300 mt-3 max-w-3xl">图片只在当前浏览器中处理，不上传到 Lilyco 服务器。支持批量统一尺寸、背景、透明图居中、亮度增强与 ZIP 交付。AI 抠图复用 Rembg Studio，创意画布复用 Infinite Canvas。</p>
  <div class="flex flex-wrap gap-2 mt-5">
   <a href="http://127.0.0.1:8042/" target="_blank" rel="noopener noreferrer" class="px-3 py-2 rounded-lg bg-indigo-500/70 hover:bg-indigo-500 text-sm inline-flex items-center gap-2">打开本机 Rembg Studio（需已启动） ↗</a>
   <a href="https://github.com/lilyco-42/rembg-ui" rel="noopener noreferrer" target="_blank" class="px-3 py-2 rounded-lg bg-white/10 hover:bg-white/15 text-sm inline-flex items-center gap-2"><Layers :size="15"/>Rembg Studio · 本地 AI 抠图 <ExternalLink :size="13"/></a>
   <a href="https://github.com/basketikun/infinite-canvas" rel="noopener noreferrer" target="_blank" class="px-3 py-2 rounded-lg bg-white/10 hover:bg-white/15 text-sm inline-flex items-center gap-2"><ImagePlus :size="15"/>Infinite Canvas · 创作画布 <ExternalLink :size="13"/></a>
  </div>
 </section>
 <div class="grid lg:grid-cols-[minmax(0,1fr)_310px] gap-5 items-start">
  <section class="bg-[#141d2d] rounded-xl border border-white/10 p-5 space-y-5">
   <div class="flex items-center justify-between gap-2"><h3 class="font-semibold">批量导入</h3><span class="text-xs text-slate-400">{{entries.length}} / 50</span></div>
   <label class="cursor-pointer block border-2 border-dashed border-indigo-400/30 hover:border-indigo-400/60 rounded-xl text-center py-10 bg-indigo-300/[.03]">
    <ImagePlus :size="30" class="mx-auto text-indigo-300"/>
    <span class="block mt-3 text-sm">点击选择商品图片（可多选）</span>
    <span class="block mt-2 text-xs text-slate-400">PNG、JPEG、WebP 等 · 单张不超过 25 MB</span>
    <input aria-label="导入商品图片" type="file" accept="image/*" multiple class="sr-only" @change="addFiles"/>
   </label>
   <div v-if="entries.length" class="grid grid-cols-2 sm:grid-cols-3 xl:grid-cols-4 gap-3">
    <div v-for="item in entries" :key="item.id" class="rounded-lg bg-[#0d1625] border border-white/10 p-2 relative">
     <img :src="item.preview" :alt="item.name" class="w-full h-32 object-contain rounded bg-[linear-gradient(45deg,#293349_25%,transparent_25%),linear-gradient(-45deg,#293349_25%,transparent_25%),linear-gradient(45deg,transparent_75%,#293349_75%),linear-gradient(-45deg,transparent_75%,#293349_75%)] bg-[length:16px_16px]"/>
     <p class="truncate text-xs text-slate-400 mt-2" :title="item.name">{{item.name}}</p>
     <button type="button" :aria-label="'移除 '+item.name" @click="remove(item.id)" class="absolute right-1 top-1 rounded-md bg-[#101827]/95 p-1"><X :size="14"/></button>
    </div>
   </div>
   <div v-else class="text-xs text-slate-500">本地预览暂为空；导入图片后才会进行处理。</div>
   <p v-if="sample" class="text-xs text-slate-400">已载入本地图片预览；抠图需要先使用 Rembg Studio 生成透明 PNG。</p>
  </section>
  <section class="rounded-xl bg-[#141d2d] border border-white/10 p-5 space-y-5">
   <h3 class="font-semibold">导出参数</h3>
   <label class="block text-sm">画布尺寸
    <select v-model.number="size" class="mt-2 w-full rounded-lg bg-[#0b1220] border border-white/15 p-3"><option v-for="v in [800,1200,1600,2000]" :value="v" :key="v">{{v}} × {{v}} px</option></select>
   </label>
   <label class="block text-sm">背景
    <select v-model="background" class="mt-2 w-full rounded-lg bg-[#0b1220] border border-white/15 p-3"><option value="white">纯白</option><option value="gray">浅灰</option><option value="transparent">透明</option></select>
   </label>
   <label class="block text-sm">导出格式
    <select v-model="format" class="mt-2 w-full rounded-lg bg-[#0b1220] border border-white/15 p-3"><option value="png">PNG</option><option value="jpeg" :disabled="background==='transparent'">JPEG</option><option value="webp">WebP</option></select>
   </label>
   <label class="block text-sm">SKU 前缀<input v-model="skuPrefix" maxlength="32" aria-label="SKU 文件名前缀" class="mt-2 w-full rounded-lg bg-[#0b1220] border border-white/15 p-3" placeholder="SKU"/></label>
   <label class="block text-sm">边距 {{margin}}%<input type="range" min="0" max="25" v-model.number="margin" class="w-full mt-3 accent-indigo-400"/></label>
   <label class="block text-sm">亮度 {{exposure.toFixed(2)}} ×<input type="range" min="0.7" max="1.4" step="0.05" v-model.number="exposure" class="w-full mt-3 accent-indigo-400"/></label>
   <div class="rounded-lg bg-[#0b1220] p-3 text-xs space-y-2">
    <p :data-wasm-ready='wasmEmbedded' class='text-slate-300'>WASM 图像内核：{{wasmEmbedded?'已嵌入（离线）':'未打包 · Canvas2D 回退'}}</p>
    <div class="flex items-center gap-2"><Zap v-if="gpuSupport" :size="16" class="text-emerald-300"/><Cpu v-else :size="16" class="text-amber-300"/><span>{{gpuSupport?'检测到 WebGPU':'WebGPU 不可用 · CPU 兼容模式'}}</span></div>
    <label v-if="gpuSupport" class="flex gap-2 items-center"><input type="checkbox" v-model="useGpu" class="accent-indigo-400"/>用 WebGPU 加速亮度调整</label>
    <label v-if="wasmEmbedded" class="flex gap-2 items-center"><input type="checkbox" v-model="useWasm" class="accent-indigo-400"/>启用 WebAssembly 像素算子</label>
    <p class="text-slate-400">WebGPU 仅用于本地像素增强；当前未内置 GPU 抠图模型。WASM AI 推理需额外模型适配。</p>
   </div>
   <button type="button" :disabled="isBusy||!entries.length||(!entries.length)" @click="exportImages" class="w-full flex items-center justify-center gap-2 rounded-lg bg-indigo-500 hover:bg-indigo-400 disabled:opacity-40 py-3 font-semibold"><Download :size="17"/>{{isBusy?'正在处理':'导出商品图 ZIP'}}</button>
   <p v-if="executionBackend" class="text-xs text-slate-300">上次执行引擎：{{executionBackend}}</p>
   <p v-if="progress" role="status" class="text-xs text-emerald-300 flex gap-2"><CheckCircle2 :size="15"/>{{progress}}</p>
   <p v-if="status" role="alert" class="text-xs text-amber-300 flex gap-2"><AlertCircle :size="15"/>{{status}}</p>
  </section>
 </div>
</div>
</template>





