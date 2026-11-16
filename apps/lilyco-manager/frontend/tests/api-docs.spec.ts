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
      if (path === '/api/tools') json(response, { tools: [] })
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
  await expect(rows).toHaveCount(3)

  // 分组顺序固定：工具 → 发布 → 元数据。
  const groups = await page.locator('h3').allInnerTexts()
  expect(groups.map(text => text.split('\n')[0].trim())).toEqual(['工具', '发布', '元数据'])

  // 方法徽标与路径逐行对应。
  await expect(rows.nth(0)).toContainText('GET')
  await expect(rows.nth(0)).toContainText('/api/tools')
  await expect(rows.nth(0)).toContainText('列出已注册工具')
  await expect(rows.nth(1)).toContainText('POST')
  await expect(rows.nth(1)).toContainText('/api/apps/{id}/install')
  await expect(rows.nth(2)).toContainText('/api/openapi.json')

  // 刷新重新拉一次文档，不缓存旧结果。
  const before = documentRequests
  await page.getByRole('button', { name: '刷新' }).click()
  await expect.poll(() => documentRequests).toBeGreaterThan(before)

  expect(errors).toEqual([])
})
