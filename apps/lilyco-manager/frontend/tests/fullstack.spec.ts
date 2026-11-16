import { test, expect } from '@playwright/test'
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs'
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
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text())
  })
  page.on('pageerror', (error) => consoleErrors.push(error.message))

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

  // 积木原型台是纯前端，但要在真实页面里证明没被后端/主题打断。
  await page.getByRole('button', { name: '设计积木' }).click()
  await page.getByRole('tab', { name: '页面组合' }).click()
  await expect(page.getByTestId('page-canvas')).toContainText('未命名原型页')

  // 接口文档读的是真实 /api/openapi.json：路由数与契约测试同源。
  await page.getByRole('button', { name: '接口文档' }).click()
  await expect(page.getByRole('heading', { name: 'Lilyco Manager API' })).toBeVisible()
  await expect(page.getByTestId('api-operation').first()).toBeVisible()
  expect(await page.getByTestId('api-operation').count()).toBeGreaterThanOrEqual(20)

  expect(consoleErrors).toEqual([])
})
