import { test, expect } from '@playwright/test'
import { createServer, type IncomingMessage, type Server } from 'node:http'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { AddressInfo } from 'node:net'

test.use({ channel: 'chrome' })

const pageHtml = readFileSync(resolve('dist/index.html'))
let server: Server
let baseUrl = ''
let lastWriteToken = ''
let readOnlyMode = false
type Run = { id: number; tool: string; output: string; created_at: string }
let runs: Run[] = [{ id: 0, tool: 'manager.health', output: '{"status":"ok"}', created_at: '2026-11-17 00:00:00' }]
let nextRun = 1

function audit(tool: string, output: Record<string, unknown>) {
  runs.unshift({ id: nextRun++, tool, output: JSON.stringify(output), created_at: '2026-11-17 00:00:00' })
}

type Plan = {
  id: number
  project_id: number
  title: string
  detail: string
  status: 'draft' | 'doing' | 'done'
  created_at: string
  updated_at: string
}
type Project = {
  id: number
  name: string
  description: string
  status: 'active' | 'paused' | 'done'
  created_at: string
  updated_at: string
  plans: Plan[]
}

let projects: Project[] = []
let nextProject = 1
let nextPlan = 1

function json(response: import('node:http').ServerResponse, body: unknown, status = 200) {
  response.writeHead(status, { 'content-type': 'application/json; charset=utf-8' })
  response.end(JSON.stringify(body))
}

async function readBody(request: IncomingMessage): Promise<Record<string, unknown>> {
  let raw = ''
  for await (const chunk of request) raw += chunk.toString()
  return raw ? JSON.parse(raw) : {}
}

function projectOf(id: number) {
  return projects.find((project) => project.id === id)
}

function planOf(id: number) {
  return projects.flatMap((project) => project.plans).find((plan) => plan.id === id)
}

test.beforeAll(async () => {
  server = createServer(async (request, response) => {
    const url = new URL(request.url ?? '/', 'http://127.0.0.1')
    const path = url.pathname
    if (!path.startsWith('/api/')) {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
      response.end(pageHtml)
      return
    }
    if (path === '/api/apps') {
      json(response, { apps: [], warnings: [] })
    } else if (path === '/api/settings') {
      json(response, { read_only: readOnlyMode, version: 'test' })
    } else if (path === '/api/tools') {
      json(response, { tools: [{ name: 'manager.health', description: '健康检查', read_only: true }] })
    } else if (path === '/api/history') {
      json(response, { runs })
    } else if (path === '/api/audit') {
      const audited = runs.filter((run) => run.tool.startsWith('project.') || run.tool.startsWith('plan.'))
      json(response, { runs: audited, count: audited.length })
    } else if (path === '/api/ecosystem') {
      json(response, { projects: [] })
    } else if (path === '/api/adapters') {
      json(response, { adapters: [] })
    } else if (path === '/api/projects' && request.method === 'GET') {
      json(response, { projects, count: projects.length })
    } else if (path === '/api/projects' && request.method === 'POST') {
      lastWriteToken = String(request.headers['x-lilyco-token'] ?? '')
      const body = await readBody(request)
      const project: Project = {
        id: nextProject++,
        name: String(body.name ?? '').trim(),
        description: String(body.description ?? '').trim(),
        status: (body.status as Project['status']) ?? 'active',
        created_at: '1',
        updated_at: '1',
        plans: [],
      }
      projects.unshift(project)
      audit('project.create', { id: project.id, name: project.name, status: project.status })
      json(response, { project })
    } else if (/^\/api\/projects\/\d+$/.test(path)) {
      const project = projectOf(Number(path.split('/').pop()))
      if (!project) {
        json(response, { error: 'missing' }, 404)
        return
      }
      if (request.method === 'GET') {
        json(response, { project })
      } else if (request.method === 'PATCH') {
        lastWriteToken = String(request.headers['x-lilyco-token'] ?? '')
        const body = await readBody(request)
        if (body.name !== undefined) project.name = String(body.name).trim()
        if (body.description !== undefined) project.description = String(body.description).trim()
        if (body.status !== undefined) project.status = body.status as Project['status']
        audit('project.update', { id: project.id, name: project.name, status: project.status })
        json(response, { project })
      } else if (request.method === 'DELETE') {
        projects = projects.filter((item) => item.id !== project.id)
        audit('project.delete', { id: project.id, deleted_plans: project.plans.length })
        json(response, { ok: true, deleted_plans: project.plans.length })
      } else {
        json(response, { error: 'method' }, 405)
      }
    } else if (/^\/api\/projects\/\d+\/plans$/.test(path) && request.method === 'POST') {
      const project = projectOf(Number(path.split('/')[3]))
      if (!project) {
        json(response, { error: 'missing' }, 404)
        return
      }
      lastWriteToken = String(request.headers['x-lilyco-token'] ?? '')
      const body = await readBody(request)
      const plan: Plan = {
        id: nextPlan++,
        project_id: project.id,
        title: String(body.title ?? '').trim(),
        detail: String(body.detail ?? '').trim(),
        status: (body.status as Plan['status']) ?? 'draft',
        created_at: '1',
        updated_at: '1',
      }
      project.plans.push(plan)
      audit('plan.create', { id: plan.id, project_id: plan.project_id, title: plan.title, status: plan.status })
      json(response, { plan })
    } else if (/^\/api\/plans\/\d+$/.test(path)) {
      const plan = planOf(Number(path.split('/').pop()))
      if (!plan) {
        json(response, { error: 'missing' }, 404)
        return
      }
      if (request.method === 'PATCH') {
        lastWriteToken = String(request.headers['x-lilyco-token'] ?? '')
        const body = await readBody(request)
        if (body.title !== undefined) plan.title = String(body.title).trim()
        if (body.detail !== undefined) plan.detail = String(body.detail).trim()
        if (body.status !== undefined) plan.status = body.status as Plan['status']
        audit('plan.update', { id: plan.id, title: plan.title, status: plan.status })
        json(response, { plan })
      } else if (request.method === 'DELETE') {
        for (const project of projects) {
          project.plans = project.plans.filter((item) => item.id !== plan.id)
        }
        audit('plan.delete', { id: plan.id })
        json(response, { ok: true })
      } else {
        json(response, { error: 'method' }, 405)
      }
    } else {
      json(response, { error: 'not found' }, 404)
    }
  })
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done))
  baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
})

test.afterAll(async () => {
  await new Promise<void>((done) => server.close(() => done()))
})

test('项目/方案：创建、方案推进、编辑、删除与窄屏深色', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text())
  })
  page.on('pageerror', (error) => errors.push(error.message))

  await page.goto(`${baseUrl}/#token=test-token-1234567890`)
  await page.getByRole('button', { name: '项目/方案' }).click()
  await expect(page.getByRole('heading', { name: '项目/方案', level: 1 })).toBeVisible()
  await expect(page.getByTestId('projects-empty')).toBeVisible()

  // 创建项目：名称去空格、状态与描述写入后端。
  await page.getByLabel('项目名称').fill('管理器 v0.3')
  await page.getByLabel('初始状态').selectOption('active')
  await page.getByLabel('描述').fill('把项目/方案纳入生命周期')
  await page.getByRole('button', { name: '创建项目' }).click()
  let card = page.getByTestId('project-card').filter({ hasText: '管理器 v0.3' })
  await expect(card).toBeVisible()
  await expect(card.getByTestId('project-status-badge')).toHaveText('进行中')
  await expect(card).toContainText('把项目/方案纳入生命周期')
  await expect(page.getByTestId('project-count')).toContainText('1 个项目')
  expect(lastWriteToken).toBe('test-token-1234567890')

  // 新增方案，默认草稿。
  await page.getByLabel('新方案标题：管理器 v0.3').fill('接入 SSE 日志')
  await page.getByLabel('新方案状态：管理器 v0.3').selectOption('draft')
  await page.getByLabel('新方案说明：管理器 v0.3').fill('实时查看开发任务输出')
  await card.getByRole('button', { name: '添加方案' }).click()
  let planRow = card.getByTestId('plan-row').filter({ hasText: '接入 SSE 日志' })
  await expect(planRow).toBeVisible()
  await expect(planRow.getByTestId('plan-status-badge')).toHaveText('草稿')
  await expect(page.getByTestId('plan-count')).toContainText('1 个方案')

  // 行内改方案状态。
  await planRow.getByLabel('方案状态：接入 SSE 日志').selectOption('doing')
  await expect(planRow.getByTestId('plan-status-badge')).toHaveText('进行中')

  // 编辑方案标题与说明。
  await planRow.getByTestId('plan-edit').click()
  await card.getByLabel('方案标题', { exact: true }).fill('SSE 实时日志')
  await card.getByLabel('说明', { exact: true }).fill('已通过契约测试')
  await page.getByRole('button', { name: '保存方案' }).click()
  planRow = card.getByTestId('plan-row').filter({ hasText: 'SSE 实时日志' })
  await expect(planRow).toBeVisible()
  await expect(planRow).toContainText('已通过契约测试')

  // 改项目状态，再编辑项目名称。
  await card.getByLabel('项目状态：管理器 v0.3').selectOption('paused')
  await expect(card.getByTestId('project-status-badge')).toHaveText('已暂停')
  await card.getByTestId('project-edit').click()
  await card.getByLabel('项目名称').fill('管理器 v0.4')
  await card.getByRole('button', { name: '保存' }).click()
  card = page.getByTestId('project-card').filter({ hasText: '管理器 v0.4' })
  await expect(card).toBeVisible()
  await expect(card.getByTestId('project-status-badge')).toHaveText('已暂停')

  // 两步确认删除方案。
  planRow = card.getByTestId('plan-row').filter({ hasText: 'SSE 实时日志' })
  await planRow.getByTestId('plan-delete').click()
  await planRow.getByTestId('plan-delete').click()
  await expect(card.getByTestId('plan-row')).toHaveCount(0)
  await expect(page.getByTestId('plan-count')).toContainText('0 个方案')

  // 再建一条方案，然后两步确认删除项目（级联方案）。
  await card.getByLabel('新方案标题：管理器 v0.4').fill('收尾')
  await card.getByRole('button', { name: '添加方案' }).click()
  await expect(card.getByTestId('plan-row')).toHaveCount(1)
  await card.getByTestId('project-delete').click()
  await expect(card.getByTestId('project-delete')).toHaveText('确认删除')
  await card.getByTestId('project-delete').click()
  await expect(page.getByTestId('projects-empty')).toBeVisible()
  await expect(page.getByTestId('project-count')).toContainText('0 个项目')

  // 审计：六类变更出现在运行历史，展开行显示真实 JSON 输出。
  await page.getByRole('button', { name: '运行历史' }).click()
  await page.getByRole('button', { name: '刷新' }).click()
  await expect(page.getByTestId('run-row').first()).toContainText('project.delete')
  const createRow = page.getByTestId('run-row').filter({ hasText: 'project.create' })
  await createRow.click()
  await expect(page.getByTestId('run-row').filter({ hasText: 'project.create' }).locator('..')).toContainText('"name": "管理器 v0.3"')
  await expect(page.getByTestId('run-row').filter({ hasText: 'plan.update' }).first()).toBeVisible()

  // 「仅审计」隐藏工具执行，只保留项目/方案变更。
  await expect(page.getByTestId('run-row').filter({ hasText: 'manager.health' })).toBeVisible()
  await page.getByRole('button', { name: '仅审计' }).click()
  await expect(page.getByTestId('run-row').filter({ hasText: 'manager.health' })).toHaveCount(0)
  await expect(page.getByTestId('run-row').first()).toContainText('project.delete')
  await page.getByRole('button', { name: '全部' }).click()
  await expect(page.getByTestId('run-row').filter({ hasText: 'manager.health' })).toBeVisible()
  await page.getByRole('button', { name: '项目/方案' }).click()

  // 375px + 深色：新视图沿用 shadcn 令牌，不横向溢出。
  await page.setViewportSize({ width: 375, height: 800 })
  await page.getByLabel('项目名称').fill('窄屏项目')
  await page.getByRole('button', { name: '创建项目' }).click()
  await expect(page.getByTestId('project-card')).toBeVisible()
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
    ),
  ).toBe(false)
  await page.getByRole('button', { name: '切换到深色主题' }).last().click()
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  const cardBackground = await page
    .getByTestId('project-card')
    .evaluate((element) => getComputedStyle(element).backgroundColor)
  expect(cardBackground).not.toBe('rgba(0, 0, 0, 0)')

  expect(errors).toEqual([])
})

test('只读模式：提示可见，项目与工具的写控件禁用', async ({ page }) => {
  readOnlyMode = true
  try {
    await page.goto(`${baseUrl}/#token=test-token-1234567890`)
    await expect(page.getByTestId('read-only-banner')).toContainText('只读模式')

    await page.getByRole('button', { name: '项目/方案' }).click()
    await expect(page.getByRole('button', { name: '创建项目' })).toBeDisabled()
    await expect(page.getByLabel('项目名称')).toBeDisabled()
    const card = page.getByTestId('project-card').first()
    await expect(card.getByTestId('project-edit')).toBeDisabled()
    await expect(card.getByTestId('project-delete')).toBeDisabled()
    await expect(card.getByLabel(/项目状态：/)).toBeDisabled()
    await expect(card.getByRole('button', { name: '添加方案' })).toBeDisabled()

    await page.getByRole('button', { name: '工具中心' }).click()
    await expect(page.getByRole('button', { name: '运行', exact: true }).first()).toBeDisabled()
  } finally {
    readOnlyMode = false
  }
})
