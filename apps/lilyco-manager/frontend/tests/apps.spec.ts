import { test, expect } from '@playwright/test'
import { createServer, type Server } from 'node:http'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { AddressInfo } from 'node:net'

test.use({ channel: 'chrome' })

const pageHtml = readFileSync(resolve('dist/index.html'))
let server: Server
let baseUrl = ''
let installed = true

function json(response: import('node:http').ServerResponse, body: unknown, status = 200) {
  response.writeHead(status, { 'content-type': 'application/json; charset=utf-8' })
  response.end(JSON.stringify(body))
}

const app = {
  id: 'demo',
  name: 'Demo 应用',
  description: '用于验证管理面板交互的模拟应用',
  repository: 'https://github.com/lilyco-42/demo',
  binary: 'demo',
  installed: true,
  capabilities: { run: true, mcp: true, schema: true },
  url: 'http://127.0.0.1:8766',
  release: true,
  dev_tasks: ['check'],
  status: null,
}

function release() {
  return {
    configured: true,
    repo: 'lilyco-42/demo',
    platform: 'linux-x64',
    current_tag: installed ? 'v0.1.0' : null,
    latest_tag: 'v0.2.0',
    asset: 'demo-linux-x64.tar.gz',
    update_available: true,
    error: null,
  }
}

test.beforeAll(async () => {
  server = createServer((request, response) => {
    const url = new URL(request.url ?? '/', 'http://127.0.0.1')
    const path = url.pathname
    if (!path.startsWith('/api/')) {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
      response.end(pageHtml)
      return
    }
    if (path === '/api/tools') {
      json(response, { tools: [{ name: 'manager.health', description: '健康检查', read_only: true }] })
    } else if (path === '/api/history') {
      json(response, { runs: [] })
    } else if (path === '/api/ecosystem') {
      json(response, { projects: [] })
    } else if (path === '/api/adapters') {
      json(response, { adapters: [] })
    } else if (path === '/api/apps') {
      json(response, { apps: [{ ...app, installed }], warnings: [] })
    } else if (path === '/api/apps/demo/release') {
      json(response, { id: 'demo', release: release() })
    } else if (path === '/api/apps/demo/install' && request.method === 'POST') {
      installed = true
      json(response, { ok: true, tag: 'v0.2.0', binary: '/tmp/demo', updated: true })
    } else if (path === '/api/apps/demo/dev') {
      json(response, {
        id: 'demo',
        tasks: [{ name: 'check', argv: ['cargo', 'check'] }],
        jobs: [],
      })
    } else if (path === '/api/apps/demo/dev/check' && request.method === 'POST') {
      json(response, { ok: true, job: 'job-1' })
    } else if (path === '/api/dev/jobs/job-1/logs') {
      json(response, {
        job: {
          id: 'job-1',
          app: 'demo',
          task: 'check',
          running: false,
          exit_code: 0,
          started_at: 0,
          finished_at: 1,
          lines: ['Finished dev profile'],
        },
      })
    } else if (path === '/api/mcp/tools') {
      json(response, {
        tools: [
          {
            name: 'demo__echo',
            app: 'demo',
            original_name: 'echo',
            description: '回显输入',
            input_schema: { type: 'object' },
          },
        ],
        providers: [{ id: 'demo', ok: true, tools: 1 }],
      })
    } else if (path === '/api/mcp/config') {
      json(response, {
        servers: { demo: { command: '/tmp/demo', args: ['--mcp'] } },
        config_path: null,
        toolhub: null,
        client_config: null,
        hint: '模拟配置',
      })
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

test('apps panel drives release, dev task and MCP views offline', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text())
  })

  await page.goto(`${baseUrl}/#token=test-token-1234567890`)
  await expect(page.getByRole('heading', { name: '应用', level: 1 })).toBeVisible()
  await expect(page.getByText('Demo 应用')).toBeVisible()
  await expect(page.getByText('已安装', { exact: true })).toBeVisible()

  await page.getByRole('button', { name: '检查更新' }).click()
  await expect(page.getByText('有可用更新')).toBeVisible()
  await expect(page.getByText('v0.1.0')).toBeVisible()
  await expect(page.getByText('v0.2.0')).toBeVisible()

  await page.getByRole('button', { name: '安装更新' }).click()
  await expect(page.getByText('已安装 v0.2.0（新版本）')).toBeVisible()

  await page.getByRole('button', { name: /开发任务/ }).click()
  await page.getByRole('button', { name: 'check' }).click()
  await expect(page.getByText('完成')).toBeVisible()
  await expect(page.getByText('Finished dev profile')).toBeVisible()

  await page.getByRole('button', { name: 'MCP 服务' }).click()
  await expect(page.getByText('demo__echo')).toBeVisible()
  await expect(page.getByText('回显输入')).toBeVisible()

  await page.getByRole('button', { name: '切换到深色主题' }).first().click()
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')

  expect(errors).toEqual([])
})
