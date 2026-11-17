import { test, expect } from '@playwright/test'
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

test.use({ channel: 'chrome' })

// 只有提供已构建的二进制时才跑：本地 `cargo build --release` 后设置
//   LILYCO_MANAGER_E2E_BIN=target/release/lilyco-manager
// CI 在 Release build 之后单独跑这条用例。
const binary = process.env.LILYCO_MANAGER_E2E_BIN

const token = 'e2e0123456789abcdef'.repeat(2)
let child: ChildProcessWithoutNullStreams | undefined
let baseUrl = ''
let temp = ''

function waitForUrl(stream: NodeJS.ReadableStream): Promise<string> {
  return new Promise((resolve, reject) => {
    let output = ''
    const timer = setTimeout(() => reject(new Error(`manager 未在 15 秒内监听:\n${output}`)), 15000)
    stream.on('data', (chunk: Buffer) => {
      output += chunk.toString()
      const match = output.match(/listening on (http:\/\/127\.0\.0\.1:\d+)\//)
      if (match) {
        clearTimeout(timer)
        resolve(match[1])
      }
    })
    stream.on('error', (error) => {
      clearTimeout(timer)
      reject(error)
    })
  })
}

test.beforeAll(async () => {
  if (!binary) return
  temp = mkdtempSync(join(tmpdir(), 'lilyco-manager-e2e-'))
  const appsDir = join(temp, 'apps')
  mkdirSync(appsDir)
  // 一个真实可跑的开发任务：先吐一行、等 1 秒、再吐一行，
  // 用来证明日志是在任务结束前实时到达的。
  const devDir = join(appsDir, 'devdemo')
  mkdirSync(devDir)
  writeFileSync(
    join(devDir, 'lilyco.app.json'),
    JSON.stringify({
      schema_version: 1,
      id: 'devdemo',
      name: '日志演示',
      description: 'SSE 全栈验收',
      binary: 'sh',
      dev: { tasks: { stream: ['sh', '-c', 'echo first-line; sleep 1; echo second-line'] } },
    }),
  )
  child = spawn(binary, [], {
    env: {
      ...process.env,
      LILYCO_MANAGER_DB: join(temp, 'runs.sqlite'),
      LILYCO_MANAGER_APPS: appsDir,
      LILYCO_MANAGER_NO_BROWSER: '1',
      LILYCO_MANAGER_TOKEN: token,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  child.stderr.on('data', () => {})
  baseUrl = await waitForUrl(child.stdout)
})

test.afterAll(async () => {
  child?.kill()
  if (temp) rmSync(temp, { recursive: true, force: true })
})

test('真实后端 + 浏览器：工具执行、历史与积木页', async ({ page }) => {
  test.skip(!binary, '设置 LILYCO_MANAGER_E2E_BIN 后运行全栈用例')

  const consoleErrors: string[] = []
  const jobRequests: string[] = []
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text())
  })
  page.on('pageerror', (error) => consoleErrors.push(error.message))
  page.on('request', (request) => {
    if (request.url().includes('/api/dev/jobs/')) jobRequests.push(request.url())
  })

  await page.goto(`${baseUrl}/#token=${token}`)
  await page.getByRole('button', { name: '概览' }).click()
  await expect(page.getByText('工具注册表')).toBeVisible()
  await expect(page.getByText('manager.health')).toBeVisible()

  // 真实 POST /api/tools/manager.health/run，结果与历史都来自后端。
  await page.getByRole('button', { name: '运行', exact: true }).first().click()
  await expect(page.locator('pre').filter({ hasText: '"status": "ok"' })).toBeVisible()
  await expect(page.locator('.card').filter({ hasText: '最近执行记录' }).getByText('1', { exact: true })).toBeVisible()

  await page.getByRole('button', { name: '运行历史' }).click()
  await expect(page.getByText('manager.health').last()).toBeVisible()

  // 开发任务日志通过 SSE 实时到达：第一行出现时任务仍在运行。
  await page.getByRole('button', { name: '应用' }).click()
  const devCard = page.locator('article').filter({ hasText: '日志演示' })
  await devCard.getByRole('button', { name: /开发任务/ }).click()
  await devCard.getByRole('button', { name: 'stream' }).click()
  await expect(devCard.getByText('运行中')).toBeVisible()
  await expect(devCard.locator('pre')).toContainText('first-line')
  // 证明浏览器用的是带一次性票据的 EventSource，而不是轮询 /logs。
  expect(jobRequests.some((url) => url.includes('/stream?ticket='))).toBe(true)
  await expect(devCard.locator('pre')).toContainText('second-line', { timeout: 5000 })
  await expect(devCard.getByText('完成')).toBeVisible()
  await expect(devCard.getByText('SSE 实时')).toBeVisible()

  // 项目/方案落真实 SQLite：创建 → 改状态 → 刷新页面后仍在。
  await page.getByRole('button', { name: '项目/方案' }).click()
  await expect(page.getByTestId('projects-empty')).toBeVisible()
  await page.getByLabel('项目名称').fill('管理器 v0.4')
  await page.getByLabel('描述').fill('真实后端持久化验收')
  await page.getByRole('button', { name: '创建项目' }).click()
  let projectCard = page.getByTestId('project-card').filter({ hasText: '管理器 v0.4' })
  await expect(projectCard).toBeVisible()
  await page.getByLabel('新方案标题：管理器 v0.4').fill('补齐项目/方案视图')
  await page.getByLabel('新方案说明：管理器 v0.4').fill('契约 + shadcn + Playwright')
  await projectCard.getByRole('button', { name: '添加方案' }).click()
  const fullstackPlan = projectCard.getByTestId('plan-row').filter({ hasText: '补齐项目/方案视图' })
  await expect(fullstackPlan).toBeVisible()
  await expect(fullstackPlan.getByTestId('plan-status-badge')).toHaveText('草稿')
  await fullstackPlan.getByLabel('方案状态：补齐项目/方案视图').selectOption('doing')
  await expect(fullstackPlan.getByTestId('plan-status-badge')).toHaveText('进行中')

  // 真实后端审计：项目/方案变更进入运行历史，展开可见结果状态。
  await page.getByRole('button', { name: '运行历史' }).click()
  await page.getByRole('button', { name: '刷新' }).click()
  const auditRow = page.getByTestId('run-row').filter({ hasText: 'plan.update' }).first()
  await expect(auditRow).toBeVisible()
  await auditRow.click()
  await expect(auditRow.locator('..')).toContainText('"status": "doing"')

  await page.goto(`${baseUrl}/#token=${token}`)
  await page.getByRole('button', { name: '项目/方案' }).click()
  projectCard = page.getByTestId('project-card').filter({ hasText: '管理器 v0.4' })
  await expect(projectCard).toBeVisible()
  await expect(projectCard).toContainText('真实后端持久化验收')
  await expect(projectCard.getByTestId('plan-row').filter({ hasText: '补齐项目/方案视图' })).toBeVisible()
  await expect(projectCard.getByTestId('plan-row').getByTestId('plan-status-badge')).toHaveText('进行中')

  // 积木原型台是纯前端，但要在真实页面里证明没被后端/主题打断。
  await page.getByRole('button', { name: '设计积木' }).click()
  await page.getByRole('tab', { name: '页面组合' }).click()
  await expect(page.getByTestId('page-canvas')).toContainText('未命名原型页')

  // 接口文档读的是真实 /api/openapi.json：路由数与契约测试同源。
  await page.getByRole('button', { name: '接口文档' }).click()
  await expect(page.getByRole('heading', { name: 'Lilyco Manager API' })).toBeVisible()
  await expect(page.getByTestId('api-operation').first()).toBeVisible()
  expect(await page.getByTestId('api-operation').count()).toBeGreaterThanOrEqual(30)

  // 试调用打真后端：GET /api/tools 带会话 token 返回 200 与工具列表。
  await page.getByRole('button', { name: '试调用 /api/tools' }).click()
  await page.getByRole('button', { name: '发送 GET' }).click()
  const result = page.getByTestId('api-result')
  await expect(result).toContainText('HTTP 200')
  await expect(result).toContainText('manager.health')

  expect(consoleErrors).toEqual([])
})
