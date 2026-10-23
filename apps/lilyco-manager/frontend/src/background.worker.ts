import type { RawImage as TransformerRawImage } from '@huggingface/transformers'

/** Download browser inference runtime ONLY after a user explicitly starts AI background removal.
 * Pinned public CDN version; nothing is fetched from or uploaded to Lilyco servers.
 * The 21 MB ONNX WASM binary and model weights are loaded by the vendor runtime on demand.
 */
const runtimeUrl = 'https://cdn.jsdelivr.net/npm/@huggingface/transformers@3.8.1/dist/transformers.web.min.js'
let library: Promise<typeof import('@huggingface/transformers')> | undefined
function runtime() {
  if (!library) {
    library = import(/* @vite-ignore */ runtimeUrl) as Promise<typeof import('@huggingface/transformers')>
  }
  return library
}

type Mode = 'portrait' | 'product'
type Request = { kind: 'remove'; id: number; bytes: ArrayBuffer; mime: string; mode: Mode; webgpu: boolean }
type Progress = { status?: string; progress?: number; file?: string }
type Segmenter = (input: TransformerRawImage) => Promise<TransformerRawImage | TransformerRawImage[]>
const MODEL: Record<Mode, string> = {
  portrait: 'Xenova/modnet',
  product: 'onnx-community/ormbg-ONNX',
}
const engines = new Map<string, Segmenter>()

async function load(mode: Mode, webgpu: boolean): Promise<{ pipe: Segmenter; backend: string }> {
  // Product model has a known WebGPU MaxPool limitation; use WASM until verified.
  const backend = mode === 'portrait' && webgpu ? 'webgpu' : 'wasm'
  const key = mode + ':' + backend
  if (engines.has(key)) return { pipe: engines.get(key)!, backend }
  const model = MODEL[mode]
  postMessage({ kind: 'progress', text: '正在初始化本机推理：' + model })
  postMessage({ kind: 'progress', text: '首次使用，正在按需加载浏览器推理运行库…' })
  const { pipeline, env } = await runtime()
  env.allowLocalModels = false
  env.allowRemoteModels = true
  env.useBrowserCache = true
  const instance = await pipeline('background-removal', model, {
    device: backend,
    dtype: backend === 'webgpu' ? 'fp16' : 'q8',
    progress_callback: (item: Progress) => {
      if (item.status === 'progress') {
        postMessage({ kind: 'progress', text: '正在下载模型缓存：' + Math.round(item.progress ?? 0) + '%' })
      }
    },
  })
  engines.set(key, instance as unknown as Segmenter)
  return { pipe: instance as unknown as Segmenter, backend }
}

self.onmessage = async (event: MessageEvent<Request>) => {
  const request = event.data
  if (request?.kind !== 'remove') return
  try {
    let selected
    try {
      selected = await load(request.mode, request.webgpu)
    } catch (err) {
      if (!request.webgpu || request.mode !== 'portrait') throw err
      postMessage({ kind: 'progress', text: 'GPU 模型不可用，回退本机 WASM CPU…' })
      selected = await load(request.mode, false)
    }
    postMessage({ kind: 'progress', text: '模型已加载，正在本地处理像素…' })
    const { RawImage } = await runtime()
    const source = await RawImage.fromBlob(new Blob([request.bytes], { type: request.mime }))
    const result = await selected.pipe(source)
    const image = Array.isArray(result) ? result[0] : result
    if (!image) throw Error('模型没有返回图片')
    const blob = await image.toBlob()
    if (blob.type && !blob.type.includes('png')) {
      // Require transparent PNG round-trip. Image model results are expected to be RGBA.
      postMessage({ kind: 'progress', text: '正在转换透明图…' })
    }
    postMessage({ kind: 'result', id: request.id, blob, backend: selected.backend, model: MODEL[request.mode] })
  } catch (error) {
    postMessage({ kind: 'error', id: request.id, message: error instanceof Error ? error.message : String(error) })
  }
}



