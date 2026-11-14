import { readFileSync, statSync, existsSync } from 'node:fs'
import { dirname, isAbsolute, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { gzipSync } from 'node:zlib'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const RAW_BUDGET = 1024 * 1024
const GZIP_BUDGET = 128 * 1024
const BINARY_BUDGET = 32 * 1024 * 1024

const bundlePath = resolve(root, 'frontend/dist/index.html')
if (!existsSync(bundlePath)) {
  throw Error('Missing frontend/dist/index.html. Run `bun run build` in frontend first.')
}

const bundle = readFileSync(bundlePath)
const gzip = gzipSync(bundle)
const problems = []

if (bundle.byteLength > RAW_BUDGET) {
  problems.push(`raw bundle ${bundle.byteLength} B exceeds ${RAW_BUDGET} B`)
}
if (gzip.byteLength > GZIP_BUDGET) {
  problems.push(`gzip bundle ${gzip.byteLength} B exceeds ${GZIP_BUDGET} B`)
}

const html = bundle.toString('utf8')
if (/<script[^>]+src=/i.test(html)) {
  problems.push('bundle references an external script; the console must stay single-file and offline')
}
if (/<link[^>]+rel=["']?stylesheet/i.test(html)) {
  problems.push('bundle references an external stylesheet; the console must stay single-file and offline')
}
for (const pattern of [/https?:\/\/cdn\./i, /unpkg\.com/i, /jsdelivr\.net/i]) {
  if (pattern.test(html)) problems.push(`bundle contains a CDN reference (${pattern})`)
}

const targetRoot = process.env.CARGO_TARGET_DIR
  ? resolve(isAbsolute(process.env.CARGO_TARGET_DIR) ? process.env.CARGO_TARGET_DIR : resolve(root, process.env.CARGO_TARGET_DIR))
  : resolve(root, 'target')
const binaryCandidates = [
  resolve(targetRoot, 'release/lilyco-manager'),
  resolve(targetRoot, 'release/lilyco-manager.exe'),
]
const binary = binaryCandidates.find((path) => existsSync(path))
let binarySize = null
if (binary) {
  binarySize = statSync(binary).size
  if (binarySize > BINARY_BUDGET) {
    problems.push(`release binary ${binarySize} B exceeds ${BINARY_BUDGET} B`)
  }
}

for (const problem of problems) console.error('perf-check:', problem)
if (problems.length) process.exit(1)

console.log(`perf-check: bundle ${bundle.byteLength} B raw / ${gzip.byteLength} B gzip (budgets ${RAW_BUDGET} / ${GZIP_BUDGET})`)
console.log(binary ? `perf-check: release binary ${binarySize} B (budget ${BINARY_BUDGET})` : 'perf-check: release binary not built; size gate skipped')
