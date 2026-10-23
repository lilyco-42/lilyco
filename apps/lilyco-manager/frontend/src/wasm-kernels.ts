import { kernelWasmBase64 } from './generated/kernels'

type WasmAbi = {
  memory: WebAssembly.Memory
  alloc(bytes: number): number
  dealloc(ptr: number, bytes: number): void
  kernel_names(ptr: number, cap: number): number
  process(id: number, rgba: number, len: number, w: number, h: number, params: number, count: number): number
}

type Runtime = { abi: WasmAbi; names: string[] }
let cached: Promise<Runtime> | undefined

export const wasmEmbedded = kernelWasmBase64.length > 0

async function runtime(): Promise<Runtime> {
  if (!wasmEmbedded) throw Error('WASM kernel was not embedded in this frontend build')
  if (!cached) {
    cached = (async () => {
      const binary = Uint8Array.from(atob(kernelWasmBase64), c => c.charCodeAt(0))
      const instantiated = await WebAssembly.instantiate(binary, {})
      const abi = instantiated.instance.exports as unknown as WasmAbi
      const cap = 16384
      const namesPtr = abi.alloc(cap)
      try {
        const length = abi.kernel_names(namesPtr, cap)
        if (length <= 0 || length > cap) throw Error('Unexpected WASM kernel registry length')
        const names = new TextDecoder().decode(new Uint8Array(abi.memory.buffer, namesPtr, length)).split('\0').filter(Boolean)
        if (!names.includes('exposure')) throw Error('WASM module lacks exposure kernel')
        return { abi, names }
      } finally {
        abi.dealloc(namesPtr, cap)
      }
    })()
  }
  return cached
}

/** Execute a verified local pixel kernel; no fetch, no remote GPU, no shell. */
export async function exposureOnWasm(image: ImageData, gain: number): Promise<ImageData> {
  if (!(gain > 0 && gain < 8) || !Number.isFinite(gain)) throw Error('Invalid exposure gain')
  const { abi, names } = await runtime()
  const bytes = image.data.length
  const rgbaPtr = abi.alloc(bytes)
  if (!rgbaPtr) throw Error('WASM allocation failed')
  const paramsLength = 4
  const paramsPtr = abi.alloc(paramsLength)
  if (!paramsPtr) {
    abi.dealloc(rgbaPtr, bytes)
    throw Error('WASM parameter allocation failed')
  }
  try {
    new Uint8Array(abi.memory.buffer, rgbaPtr, bytes).set(image.data)
    new Float32Array(abi.memory.buffer, paramsPtr, 1)[0] = Math.log2(gain)
    const code = abi.process(names.indexOf('exposure'), rgbaPtr, bytes, image.width, image.height, paramsPtr, 1)
    if (code !== 0) throw Error('WASM exposure returned ' + code)
    const output = new Uint8ClampedArray(abi.memory.buffer.slice(rgbaPtr, rgbaPtr + bytes))
    return new ImageData(output, image.width, image.height)
  } finally {
    abi.dealloc(paramsPtr, paramsLength)
    abi.dealloc(rgbaPtr, bytes)
  }
}
