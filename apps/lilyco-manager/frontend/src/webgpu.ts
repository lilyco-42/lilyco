export async function webGpuAvailable(): Promise<boolean> {
  const gpu = (navigator as Navigator & { gpu?: GPU }).gpu
  if (!gpu) return false
  try { return !!(await gpu.requestAdapter()) } catch { return false }
}

/** GPU-executed RGB exposure; alpha stays untouched. Fallback is handled by caller. */
export async function exposeOnGpu(data: ImageData, gain: number): Promise<ImageData> {
  const gpu = (navigator as Navigator & { gpu?: GPU }).gpu
  if (!gpu) throw Error('WebGPU not available')
  const adapter = await gpu.requestAdapter()
  if (!adapter) throw Error('No WebGPU adapter')
  const device = await adapter.requestDevice()
  const { width, height } = data
  const pixels = width * height
  const bytes = pixels * 4
  const input = new Uint32Array(data.data.buffer.slice(0))
  const source = device.createBuffer({ size: bytes, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST })
  const result = device.createBuffer({ size: bytes, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC })
  const readback = device.createBuffer({ size: bytes, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ })
  const settings = device.createBuffer({ size: 16, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST })
  try {
    device.queue.writeBuffer(source, 0, input)
    device.queue.writeBuffer(settings, 0, new Float32Array([gain, pixels, 0, 0]))
    const shader = device.createShaderModule({ code: `
struct Params { gain: f32, count: f32, reserved1: f32, reserved2: f32 }
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
 let i = id.x;
 if (i >= u32(params.count)) { return; }
 let pixel = src[i];
 let r = u32(min(255.0, f32(pixel & 255u) * params.gain));
 let g = u32(min(255.0, f32((pixel >> 8u) & 255u) * params.gain));
 let b = u32(min(255.0, f32((pixel >> 16u) & 255u) * params.gain));
 dst[i] = (pixel & 0xff000000u) | (b << 16u) | (g << 8u) | r;
}` })
    const pipeline = device.createComputePipeline({ layout: 'auto', compute: { module: shader, entryPoint: 'main' } })
    const group = device.createBindGroup({ layout: pipeline.getBindGroupLayout(0), entries: [
      { binding: 0, resource: { buffer: source } },
      { binding: 1, resource: { buffer: result } },
      { binding: 2, resource: { buffer: settings } },
    ] })
    const command = device.createCommandEncoder()
    const pass = command.beginComputePass()
    pass.setPipeline(pipeline)
    pass.setBindGroup(0, group)
    pass.dispatchWorkgroups(Math.ceil(pixels / 256))
    pass.end()
    command.copyBufferToBuffer(result, 0, readback, 0, bytes)
    device.queue.submit([command.finish()])
    await readback.mapAsync(GPUMapMode.READ)
    const dataCopy = new Uint8ClampedArray(readback.getMappedRange().slice(0))
    readback.unmap()
    return new ImageData(dataCopy, width, height)
  } finally {
    source.destroy(); result.destroy(); readback.destroy(); settings.destroy()
  }
}
