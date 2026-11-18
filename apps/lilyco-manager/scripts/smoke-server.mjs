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

function apiJson(path, method, body) {
  return api(path, {
    method,
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
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

  const spec = await (await api('/api/openapi.json')).json()
  if (spec.openapi !== '3.1.0') throw new Error(`expected OpenAPI 3.1.0, got ${spec.openapi}`)
  if (Object.keys(spec.paths ?? {}).length < 19) throw new Error('OpenAPI document is missing routes')
  if (spec.paths?.['/api/projects']?.post?.responses?.['403'] === undefined) {
    throw new Error('OpenAPI document is missing the read-only 403 contract')
  }

  const settings = await (await api('/api/settings')).json()
  if (settings.read_only !== false) {
    throw new Error(`expected writable smoke server, got read_only=${settings.read_only}`)
  }

  // 项目/方案真实生命周期：鉴权先于 JSON 解析、校验 400、未知 id 404、
  // 状态校验与删除级联都要在真实二进制上跑一遍。
  const anonymousProject = await fetch(baseUrl + '/api/projects', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: '{}',
  })
  if (anonymousProject.status !== 401) {
    throw new Error(`expected 401 for project create without token, got ${anonymousProject.status}`)
  }

  const invalidProject = await apiJson('/api/projects', 'POST', { name: '   ' })
  if (invalidProject.status !== 400) {
    throw new Error(`expected 400 for blank project name, got ${invalidProject.status}`)
  }

  const createdProject = await apiJson('/api/projects', 'POST', {
    name: ' 冒烟项目 ',
    description: ' 运行时项目/方案契约 ',
    status: 'active',
  })
  if (createdProject.status !== 200) {
    throw new Error(`project create returned ${createdProject.status}`)
  }
  const project = (await createdProject.json()).project
  if (project.name !== '冒烟项目' || project.status !== 'active') {
    throw new Error(`project create did not normalise the payload: ${JSON.stringify(project)}`)
  }

  const createdPlan = await apiJson(`/api/projects/${project.id}/plans`, 'POST', { title: '接入 SSE' })
  if (createdPlan.status !== 200) throw new Error(`plan create returned ${createdPlan.status}`)
  const plan = (await createdPlan.json()).plan
  if (plan.status !== 'draft' || plan.project_id !== project.id) {
    throw new Error(`plan create returned an unexpected payload: ${JSON.stringify(plan)}`)
  }

  const projects = await (await api('/api/projects')).json()
  if (projects.count !== 1 || projects.projects?.[0]?.plans?.[0]?.id !== plan.id) {
    throw new Error('project list did not embed the created plan')
  }

  const pausedProject = await apiJson(`/api/projects/${project.id}`, 'PATCH', { status: 'paused' })
  if (pausedProject.status !== 200) throw new Error(`project patch returned ${pausedProject.status}`)
  const paused = (await pausedProject.json()).project
  if (paused.status !== 'paused' || paused.name !== '冒烟项目') {
    throw new Error('project patch did not preserve untouched fields')
  }

  const doingPlan = await apiJson(`/api/plans/${plan.id}`, 'PATCH', { title: '接入 SSE 日志流', status: 'doing' })
  if (doingPlan.status !== 200) throw new Error(`plan patch returned ${doingPlan.status}`)
  const doing = (await doingPlan.json()).plan
  if (doing.status !== 'doing' || doing.title !== '接入 SSE 日志流') {
    throw new Error('plan patch did not apply both fields')
  }

  const unknownProject = await api('/api/projects/999')
  if (unknownProject.status !== 404) {
    throw new Error(`expected 404 for unknown project, got ${unknownProject.status}`)
  }

  const crossOriginPatch = await fetch(baseUrl + `/api/projects/${project.id}`, {
    method: 'PATCH',
    headers: { Origin: 'http://evil.example', 'content-type': 'application/json', 'x-lilyco-token': token },
    body: JSON.stringify({ status: 'done' }),
  })
  if (crossOriginPatch.status !== 401) {
    throw new Error(`expected 401 for cross-origin project patch, got ${crossOriginPatch.status}`)
  }

  const deletedPlan = await api(`/api/plans/${plan.id}`, { method: 'DELETE' })
  if (deletedPlan.status !== 200) throw new Error(`plan delete returned ${deletedPlan.status}`)

  const secondPlan = await apiJson(`/api/projects/${project.id}/plans`, 'POST', { title: '第二条' })
  if (secondPlan.status !== 200) throw new Error(`second plan create returned ${secondPlan.status}`)

  const deletedProject = await api(`/api/projects/${project.id}`, { method: 'DELETE' })
  if (deletedProject.status !== 200) throw new Error(`project delete returned ${deletedProject.status}`)
  const deleted = await deletedProject.json()
  if (deleted.deleted_plans !== 1) {
    throw new Error(`project delete should cascade one plan, got ${deleted.deleted_plans}`)
  }

  const remaining = await (await api('/api/projects')).json()
  if (remaining.count !== 0) throw new Error(`expected no projects after delete, got ${remaining.count}`)

  const audit = await (await api('/api/history')).json()
  const auditTools = (audit.runs ?? []).map((run) => run.tool)
  for (const tool of ['project.create', 'project.update', 'project.delete', 'plan.create', 'plan.update', 'plan.delete']) {
    if (!auditTools.includes(tool)) throw new Error(`missing audit entry ${tool}`)
  }

  const auditOnly = await (await api('/api/audit')).json()
  if (auditOnly.count !== auditOnly.runs?.length || auditOnly.count < 6) {
    throw new Error(`expected at least 6 audit-only entries, got ${auditOnly.count}`)
  }
  if (auditOnly.runs.some((run) => !/^(project|plan)\./.test(run.tool))) {
    throw new Error('audit endpoint leaked non-project/plan entries')
  }


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
    `smoke-server: 401/200/run/history/openapi/origin/projects checks passed; startup ${startupMs} ms, p95 ${p95} ms, RSS ${rss || 'n/a'} KiB`,
  )
  finish(0)
}

main().catch((error) => finish(1, `smoke-server: ${error.message}`))
