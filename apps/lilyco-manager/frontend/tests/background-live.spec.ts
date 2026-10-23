import { test, expect } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

test.use({ channel: 'chrome', acceptDownloads: true })

test('real optional local MODNet inference', async ({ page }) => {
  test.skip(process.env.LIVE_AI_TEST !== '1', 'Live model test requires explicit client opt-in')
  test.setTimeout(330000)
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
  await page.getByRole('button', { name: '对选中图片 AI 抠图' }).click()
  await expect(page.getByText(/完成：Xenova\/modnet/)).toBeVisible({ timeout: 300000 })
  expect(await page.locator('[role=alert]').allTextContents()).toEqual([])
  expect(uploadRequests).toEqual([])
})
