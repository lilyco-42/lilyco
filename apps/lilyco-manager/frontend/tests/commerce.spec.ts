import { test, expect } from '@playwright/test'
import { unzipSync } from 'fflate'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

test.use({ channel: 'chrome', acceptDownloads: true })
test('offline browser creates deliverable image ZIP with no image uploads', async ({ page }) => {
  const outgoing: string[] = []
  page.on('request', req => { if (req.method() !== 'GET') outgoing.push(req.url()) })
  await page.goto(pathToFileURL(resolve('dist/index.html')).href)
  await expect(page.getByText('跨境电商商品图工作台')).toBeVisible()
  const sample = readFileSync(resolve('tests/fixtures/sample.png'))
  await page.locator('input[type=file]').setInputFiles([{ name: 'sample.png', mimeType: 'image/png', buffer: sample }])
  await expect(page.getByText('1 / 50')).toBeVisible()
  await page.getByLabel('SKU 文件名前缀').fill('TEST')
  const [download] = await Promise.all([
    page.waitForEvent('download', { timeout: 20000 }),
    page.getByRole('button', { name: '导出商品图 ZIP' }).click()
  ])
  const archive = unzipSync(new Uint8Array(readFileSync(await download.path())))
  expect(Object.keys(archive)).toContain('TEST_001_sample.png')
  expect(Object.keys(archive)).toContain('delivery-manifest.json')
  expect(archive['TEST_001_sample.png']!.byteLength).toBeGreaterThan(100)
  const manifest = JSON.parse(new TextDecoder().decode(archive['delivery-manifest.json']))
  expect(manifest.items).toHaveLength(1)
  expect(outgoing).toEqual([])
})

