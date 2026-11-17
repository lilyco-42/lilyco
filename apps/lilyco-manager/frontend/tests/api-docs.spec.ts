import { test, expect } from '@playwright/test'
import { createServer, type Server } from 'node:http'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { AddressInfo } from 'node:net'

test.use({ channel: 'chrome' })

const pageHtml = readFileSync(resolve('dist/index.html'))
let server: Server
let baseUrl = ''
let documentRequests = 0
let toolsRequests = 0
let lastToolsToken = ''

function json(response: import('node:http').ServerResponse, body: unknown) {
  response.writeHead(200, { 'content-type': 'application/json; charset=utf-8' })
  response.end(JSON.stringify(body))
}

// 与后端 api_spec() 同形状的最小文档：两条 GET、一条 POST，覆盖分组与徽标。
const document = {
  openapi: '3.1.0',
  info: { title: 'Lilyco Manager API', version: '0.2.0', description: '本机管理器接口' },
  paths: {
    '/api/tools': {
      get: { summary: '列出已注册工具', tags: ['tools'] },
    },
    '/api/apps/{id}/install': {
      post: { summary: '安装或更新应用', tags: ['release'] },
    },
    '/api/projects/{id}': {
      get: { summary: '读取项目与方案', tags: ['projects'] },
    },
    '/api/dev/jobs/{job}/logs': {
      get: { summary: '读取开发任务输出与退出码', tags: ['dev'] },
    },
    '/api/openapi.json': {
      get: { summary: '导出接口清单', tags: ['meta'] },
    },
  },
}

test.beforeAll(async () => {
  server = createServer((request, response) => {
    const path = new URL(request.url ?? '/', 'http://127.0.0.1').pathname
    if (path === '/api/openapi.json') {
      documentRequests += 1
      expect(request.headers['x-lilyco-token']).toBe('t'.repeat(32))
      json(response, document)
      return
    }
    if (path.startsWith('/api/')) {
      if (path === '/api/tools') {
        toolsRequests += 1
        lastToolsToken = String(request.headers['x-lilyco-token'] ?? '')
        json(response, { tools: [{ name: 'manager.health', read_only: true }] })
      }
      else if (path === '/api/history') json(response, { runs: [] })
      else if (path === '/api/ecosystem') json(response, { projects: [] })
      else if (path === '/api/adapters') json(response, { adapters: [] })
      else json(response, { error: 'not found' })
      return
    }
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
    response.end(pageHtml)
  })
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
  baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
})

test.afterAll(async () => {
  await new Promise<void>(resolve => server.close(() => resolve()))
})

test('接口文档：按 tag 分组渲染方法与路径', async ({ page }) => {
  const errors: string[] = []
  page.on('console', message => {
    if (message.type() === 'error') errors.push(message.text())
  })
  page.on('pageerror', error => errors.push(error.message))

  await page.goto(`${baseUrl}/#token=${'t'.repeat(32)}`)
  await page.getByRole('button', { name: '接口文档' }).click()

  // 标题、版本与「需 token」徽标来自文档本身。
  await expect(page.getByRole('heading', { name: 'Lilyco Manager API' })).toBeVisible()
  await expect(page.getByText('v0.2.0', { exact: false })).toBeVisible()
  await expect(page.getByText('需 token')).toBeVisible()

  const rows = page.getByTestId('api-operation')
  await expect(rows).toHaveCount(5)

  // 分组顺序固定：工具 → 项目 → 发布 → 开发任务 → 元数据。
  const groups = await page.locator('h3').allInnerTexts()
  expect(groups.map(text => text.split('\n')[0].trim())).toEqual(['工具', '项目', '发布', '开发任务', '元数据'])

  // 方法徽标与路径逐行对应。
  await expect(rows.nth(0)).toContainText('GET')
  await expect(rows.nth(0)).toContainText('/api/tools')
  await expect(rows.nth(0)).toContainText('列出已注册工具')
  await expect(rows.nth(1)).toContainText('GET')
  await expect(rows.nth(1)).toContainText('/api/projects/{id}')
  await expect(rows.nth(1)).toContainText('读取项目与方案')
  await expect(rows.nth(2)).toContainText('POST')
  await expect(rows.nth(2)).toContainText('/api/apps/{id}/install')
  await expect(rows.nth(3)).toContainText('/api/dev/jobs/{job}/logs')
  await expect(rows.nth(4)).toContainText('/api/openapi.json')

  // 刷新重新拉一次文档，不缓存旧结果。
  const before = documentRequests
  await page.getByRole('button', { name: '刷新' }).click()
  await expect.poll(() => documentRequests).toBeGreaterThan(before)

  expect(errors).toEqual([])
})

test('接口文档：GET 可试调用，写操作不暴露试调用', async ({ page }) => {
  await page.goto(`${baseUrl}/#token=${'t'.repeat(32)}`)
  await page.getByRole('button', { name: '接口文档' }).click()

  const rows = page.getByTestId('api-operation')
  // 每个 GET 行都有试调用；POST 行没有，避免文档页成为一键改机入口。
  const getRow = rows.filter({ hasText: '/api/tools' })
  const postRow = rows.filter({ hasText: '/api/apps/{id}/install' })
  await expect(getRow.getByRole('button', { name: '试调用 /api/tools' })).toBeVisible()
  await expect(postRow.getByRole('button', { name: /试调用/ })).toHaveCount(0)

  const before = toolsRequests
  await getRow.getByRole('button', { name: '试调用 /api/tools' }).click()
  await page.getByRole('button', { name: '发送 GET' }).click()

  const result = page.getByTestId('api-result')
  await expect(result).toContainText('HTTP 200')
  await expect(result).toContainText('manager.health')
  await expect.poll(() => toolsRequests).toBe(before + 1)
  // 试调用带的是当前会话 token，不是匿名请求。
  expect(lastToolsToken).toBe('t'.repeat(32))

  // 带路径参数的 GET 会先要求填参数，默认值可直接发送。
  await rows.filter({ hasText: '/api/dev/jobs/{job}/logs' }).getByRole('button', { name: /试调用/ }).click()
  await expect(page.getByRole('textbox', { name: 'job' })).toHaveValue('none')

  // 再点一次同一行收起面板（同一时间只展开一个）。
  await rows
    .filter({ hasText: '/api/dev/jobs/{job}/logs' })
    .getByRole('button', { name: /试调用/ })
    .click()
  await expect(page.getByTestId('api-try')).toHaveCount(0)
})

test('接口文档：375px 窄屏不横向溢出，深色令牌生效', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 720 })
  await page.goto(`${baseUrl}/#token=${'t'.repeat(32)}`)
  await page.getByRole('button', { name: '接口文档' }).click()
  await expect(page.getByTestId('api-operation').first()).toBeVisible()

  // 窄屏下试调用面板仍可用，结果区立即可见。
  await page.getByRole('button', { name: '试调用 /api/tools' }).click()
  await page.getByRole('button', { name: '发送 GET' }).click()
  await expect(page.getByTestId('api-result')).toContainText('HTTP 200')

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  )
  expect(overflow).toBeLessThanOrEqual(0)

  // 深色主题走同一套令牌：卡片底色必须真的变，而不是只切了个属性。
  const card = page.locator('[data-testid="api-doc-card"]')
  const light = await card.evaluate(node => getComputedStyle(node).backgroundColor)
  await page.getByRole('button', { name: '切换到深色主题' }).click()
  const dark = await card.evaluate(node => getComputedStyle(node).backgroundColor)
  expect(dark).not.toBe(light)
})
