import { test, expect } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

test.use({ channel: 'chrome', acceptDownloads: true })

test('real optional local background removal inference', async ({ page }) => {
  test.skip(process.env.LIVE_AI_TEST !== '1', 'Live model test requires explicit client opt-in')
  test.setTimeout(210000)
  page.on('pageerror', e => console.log('PAGE_ERROR:', e.message))
  page.on('console', e => { if (e.type() === 'error') console.log('CONSOLE_ERROR:', e.text()) })
  page.on('requestfailed', e => console.log('REQUEST_FAILED:', e.url(), e.failure()?.errorText))
  const uploadRequests: string[] = []
  page.on('request', r => { if (['POST', 'PUT'].includes(r.method())) uploadRequests.push(r.url()) })
  await page.goto(pathToFileURL(resolve('dist/index.html')).href)
  await page.locator('input[type=file]').setInputFiles([{
    name: 'sample.png', mimeType: 'image/png',
    buffer: readFileSync(resolve('tests/fixtures/sample.png'))
  }])
  const model = process.env.LIVE_AI_MODEL === 'product' ? 'product' : 'portrait'
  if (model === 'product') {
    await page.getByRole('combobox', { name: '模型类型' }).selectOption('product')
  } else {
    await page.getByRole('checkbox', { name: '优先 WebGPU' }).uncheck().catch(() => {})
  }
  await page.getByRole('button', { name: '对选中图片 AI 抠图' }).click()
  await expect.poll(async () => {
    const done = await page.getByText(/完成：(Xenova\/modnet|onnx-community\/ormbg-ONNX)/).count()
    if (done) return 'completed'
    const alerts = await page.locator('[role=alert]').allTextContents()
    return alerts[0] ?? 'loading'
  }, { timeout: 180000, intervals: [1000, 2000, 5000] }).toBe('completed')
  expect(await page.locator('[role=alert]').allTextContents()).toEqual([])
  expect(uploadRequests).toEqual([])
})
