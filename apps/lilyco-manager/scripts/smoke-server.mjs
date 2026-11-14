import { execFileSync, spawn } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const targetRoot = process.env.CARGO_TARGET_DIR
  ? resolve(process.env.CARGO_TARGET_DIR)
  : resolve(root, 'target')
const binary = [
  join(targetRoot, 'release', 'lilyco-manager'),
  join(targetRoot, 'release', 'lilyco-manager.exe'),
].find((path) => existsSync(path))

if (!binary) {
  console.error('smoke-server: build the release binary first: cargo build --release')
  process.exit(1)
}

const STARTUP_BUDGET_MS = 5000
const P95_BUDGET_MS = 500
const RSS_BUDGET_KB = 256 * 1024

const token = '0123456789abcdef'.repeat(4)
const temp = mkdtempSync(join(tmpdir(), 'lilyco-manager-smoke-'))
const appsDir = join(temp, 'apps')
mkdirSync(appsDir)
const dbPath = join(temp, 'runs.sqlite')

const started = Date.now()
const child = spawn(binary, [], {
  env: {
    ...process.env,
    LILYCO_MANAGER_DB: dbPath,
    LILYCO_MANAGER_APPS: appsDir,
    LILYCO_MANAGER_NO_BROWSER: '1',
    LILYCO_MANAGER_TOKEN: token,
  },
  stdio: ['ignore', 'pipe', 'pipe'],
})

let output = ''
let baseUrl = ''
let settled = false

function finish(code, message) {
  if (settled) return
  settled = true
  if (message) console[code === 0 ? 'log' : 'error'](message)
  try {
    child.kill('SIGTERM')
  } catch {}
  setTimeout(() => {
    try {
      child.kill('SIGKILL')
    } catch {}
    rmSync(temp, { recursive: true, force: true })
    process.exit(code)
  }, 300).unref()
}

const listening = new Promise((resolveListening) => {
  const inspect = () => {
    const match = output.match(/listening on (http:\/\/[^\s]+)/)
    if (match && !baseUrl) {
      baseUrl = match[1].replace(/\/$/, '')
      resolveListening()
    }
  }
  child.stdout.on('data', (chunk) => {
    output += chunk.toString()
    inspect()
  })
  child.stderr.on('data', (chunk) => {
    output += chunk.toString()
    inspect()
  })
  child.on('exit', (code) => {
    if (!baseUrl) {
      finish(1, `smoke-server: exited before listening (code ${code})\n${output}`)
    }
  })
  setTimeout(() => finish(1, `smoke-server: startup timeout\n${output}`), STARTUP_BUDGET_MS + 5000).unref()
})

function api(path, init = {}) {
  return fetch(baseUrl + path, {
    ...init,
    headers: { 'x-lilyco-token': token, ...(init.headers ?? {}) },
  })
}

function rssKb(pid) {
  try {
    if (process.platform === 'linux') {
      const status = readFileSync(`/proc/${pid}/status`, 'utf8')
      return Number(status.match(/^VmRSS:\s+(\d+)/m)?.[1] ?? 0)
    }
    if (process.platform === 'darwin') {
      return Number(execFileSync('ps', ['-o', 'rss=', '-p', String(pid)], { encoding: 'utf8' }).trim())
    }
    if (process.platform === 'win32') {
      const bytes = Number(
        execFileSync(
          'powershell',
          ['-NoProfile', '-Command', `(Get-Process -Id ${pid}).WorkingSet64`],
          { encoding: 'utf8', timeout: 5000 },
        ).trim(),
      )
      return Math.round(bytes / 1024)
    }
  } catch {}
  return 0
}

async function main() {
  await listening
  const startupMs = Date.now() - started

  const unauthorized = await fetch(baseUrl + '/api/tools')
  if (unauthorized.status !== 401) throw new Error(`expected 401 without token, got ${unauthorized.status}`)

  const tools = await (await api('/api/tools')).json()
  if (tools.tools?.length !== 2) throw new Error(`expected 2 tools, got ${tools.tools?.length}`)

  const apps = await (await api('/api/apps')).json()
  if (!Array.isArray(apps.apps) || apps.apps.length !== 0) throw new Error('expected an empty isolated app registry')

  const executed = await (await api('/api/tools/manager.health/run', { method: 'POST' })).json()
  if (executed.output?.status !== 'ok') throw new Error('manager.health did not return ok')

  const history = await (await api('/api/history')).json()
  if (history.runs?.[0]?.tool !== 'manager.health') throw new Error('history did not record the run')

  const crossOrigin = await fetch(baseUrl + '/api/apps/missing/start', {
    method: 'POST',
    headers: { 'x-lilyco-token': token, Origin: 'http://evil.example' },
  })
  if (crossOrigin.status !== 401) throw new Error(`expected 401 for cross-origin mutation, got ${crossOrigin.status}`)

  const samples = []
  for (let i = 0; i < 40; i += 1) {
    const begin = Date.now()
    const response = await api('/api/tools')
    if (!response.ok) throw new Error(`latency request failed: ${response.status}`)
    await response.arrayBuffer()
    samples.push(Date.now() - begin)
  }
  samples.sort((a, b) => a - b)
  const p95 = samples[Math.ceil(samples.length * 0.95) - 1]
  const rss = rssKb(child.pid)

  const problems = []
  if (startupMs > STARTUP_BUDGET_MS) problems.push(`startup ${startupMs} ms > ${STARTUP_BUDGET_MS} ms`)
  if (p95 > P95_BUDGET_MS) problems.push(`p95 ${p95} ms > ${P95_BUDGET_MS} ms`)
  if (rss && rss > RSS_BUDGET_KB) problems.push(`RSS ${rss} KiB > ${RSS_BUDGET_KB} KiB`)
  if (problems.length) throw new Error(problems.join('; '))

  console.log(
    `smoke-server: 401/200/run/history/origin checks passed; startup ${startupMs} ms, p95 ${p95} ms, RSS ${rss || 'n/a'} KiB`,
  )
  finish(0)
}

main().catch((error) => finish(1, `smoke-server: ${error.message}`))
