import { test, expect } from '@playwright/test'
import { createServer, type Server } from 'node:http'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { AddressInfo } from 'node:net'

test.use({ channel: 'chrome' })

const pageHtml = readFileSync(resolve('dist/index.html'))
let server: Server
let baseUrl = ''

function json(response: import('node:http').ServerResponse, body: unknown) {
  response.writeHead(200, { 'content-type': 'application/json; charset=utf-8' })
  response.end(JSON.stringify(body))
}

test.beforeAll(async () => {
  server = createServer((request, response) => {
    const path = new URL(request.url ?? '/', 'http://127.0.0.1').pathname
    if (!path.startsWith('/api/')) {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
      response.end(pageHtml)
      return
    }
    if (path === '/api/tools') json(response, { tools: [] })
    else if (path === '/api/settings') json(response, { read_only: false, version: 'test' })
    else if (path === '/api/history') json(response, { runs: [] })
    else if (path === '/api/ecosystem') json(response, { projects: [] })
    else if (path === '/api/adapters') json(response, { adapters: [] })
    else json(response, { error: 'not found' })
  })
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
  baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
})

test.afterAll(async () => {
  await new Promise<void>(resolve => server.close(() => resolve()))
})

test('积木工作台：属性编辑、切换积木与 JSON 校验', async ({ page }) => {
  await page.goto(`${baseUrl}/#token=${'t'.repeat(32)}`)
  await page.getByRole('button', { name: '设计积木' }).click()

  await expect(page.getByRole('heading', { name: '设计积木', level: 1 })).toBeVisible()
  await expect(page.getByTestId('block-preview')).toContainText('本地优先的工具工作台')
  await expect(page.getByText('6 块积木')).toBeVisible()

  // 属性面板改标题，预览即时更新。
  await page.getByLabel('标题').fill('新的原型标题')
  await expect(page.getByTestId('block-preview')).toContainText('新的原型标题')

  // 切换积木类型。
  await page.getByRole('button', { name: '切换到积木：日志面板' }).click()
  await expect(page.getByTestId('block-preview')).toContainText('最近输出')
  await expect(page.getByLabel('积木 JSON')).toHaveValue(/"kind": "logs"/)

  // 非法 JSON：就地报错，不覆盖当前预览。
  await page.getByLabel('积木 JSON').fill('{"kind":"nope","props":{}}')
  await page.getByRole('button', { name: '应用 JSON' }).click()
  await expect(page.getByText('未知积木类型：nope')).toBeVisible()
  await expect(page.getByTestId('block-preview')).toContainText('最近输出')

  // 合法 JSON：应用后渲染指标卡。
  await page.getByLabel('积木 JSON').fill(
    JSON.stringify({ kind: 'stats', props: { label: '已注册工具', value: '7', hint: '来自 /api/tools', tone: 'brand' } }),
  )
  await page.getByRole('button', { name: '应用 JSON' }).click()
  await expect(page.getByTestId('block-preview')).toContainText('7')
  await expect(page.getByLabel('积木 JSON')).toHaveValue(/"kind": "stats"/)

  // 重置回到该积木的默认属性。
  await page.getByRole('button', { name: '重置' }).click()
  await expect(page.getByTestId('block-preview')).toContainText('已注册工具')
})

test('积木工作台：深色主题沿用 shadcn 令牌', async ({ page }) => {
  await page.goto(`${baseUrl}/#token=${'t'.repeat(32)}`)
  await page.getByRole('button', { name: '设计积木' }).click()
  await page.getByRole('button', { name: '切换到深色主题' }).first().click()
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  await expect(page.getByTestId('preview-canvas')).toBeVisible()
})

test('页面组合：追加、排序、删除与整页 JSON 校验', async ({ page }) => {
  await page.goto(`${baseUrl}/#token=${'t'.repeat(32)}`)
  await page.getByRole('button', { name: '设计积木' }).click()
  await page.getByRole('tab', { name: '页面组合' }).click()

  await expect(page.getByTestId('page-canvas')).toContainText('未命名原型页')
  await expect(page.getByTestId('page-canvas')).toContainText('本地优先的工具工作台')
  const json = page.getByLabel('页面 JSON')

  // 追加一块，页面 JSON 从 3 块变成 4 块。
  await page.getByRole('button', { name: '添加积木：日志面板' }).click()
  await expect(page.getByTestId('page-canvas')).toContainText('最近输出')
  await expect(json).toHaveValue(/"kind": "logs"/)

  // 上移后顺序变化：第 4 块（logs）移到第 3 位。
  await page.getByRole('button', { name: '上移第 4 块' }).click()
  const moved = JSON.parse((await json.inputValue()) as string)
  expect(moved.blocks.map((block: { kind: string }) => block.kind)).toEqual(['hero', 'stats', 'logs', 'empty'])

  // 删除第 1 块，剩 3 块。
  await page.getByRole('button', { name: '删除第 1 块' }).click()
  const removed = JSON.parse((await json.inputValue()) as string)
  expect(removed.blocks).toHaveLength(3)

  // 整页 JSON 应用：标题与指标值都更新。
  await json.fill(
    JSON.stringify({
      title: '登录原型',
      blocks: [
        { kind: 'hero', props: { title: '欢迎回来', description: '本地优先的登录页。', badge: 'DEMO', tone: 'brand' } },
        { kind: 'stats', props: { label: '在线设备', value: '3', hint: '本机回环', tone: 'neutral' } },
      ],
    }),
  )
  await page.getByRole('button', { name: '应用 JSON' }).click()
  await expect(page.getByTestId('page-canvas')).toContainText('登录原型')
  await expect(page.getByTestId('page-canvas')).toContainText('欢迎回来')
  await expect(page.getByTestId('page-canvas')).toContainText('在线设备')

  // 空 blocks：就地报错，不覆盖已应用的原型页。
  await json.fill(JSON.stringify({ title: '空页', blocks: [] }))
  await page.getByRole('button', { name: '应用 JSON' }).click()
  await expect(page.getByText('blocks 至少要有 1 块积木')).toBeVisible()
  await expect(page.getByTestId('page-canvas')).toContainText('登录原型')
})
