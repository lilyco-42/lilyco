import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
const file = process.argv[2]
if (!file) throw Error('Usage: bun scripts/embed-wasm.mjs <path-to-tested-wasm>')
const bytes = readFileSync(resolve(file))
if (bytes.length < 1000 || bytes.length > 3_000_000) throw Error('Unexpected WASM binary size')
if (bytes.subarray(0, 4).toString('hex') !== '0061736d') throw Error('WASM magic bytes missing')
const output = resolve('frontend/src/generated/kernels.ts')
mkdirSync(dirname(output), { recursive: true })
writeFileSync(output, '// Generated in GitHub Actions from pinned wasm-pixel-kernels. Do not hand-edit.\n' +
  'export const kernelWasmBase64 = ' + JSON.stringify(bytes.toString('base64')) + '\n')
console.log('Embedded ' + bytes.length + ' bytes of verified WebAssembly')
