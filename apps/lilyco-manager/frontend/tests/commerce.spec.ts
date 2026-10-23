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


test('CI embeds and executes verified WebAssembly exposure kernel on device', async ({ page }) => {
  test.skip(!process.env.CI, 'WASM is embedded by CI after compiling pinned source; local source build uses CPU fallback')
  await page.addInitScript(() => Object.defineProperty(navigator, 'gpu', {value:undefined,configurable:true}))
  await page.goto(pathToFileURL(resolve('dist/index.html')).href)
  await expect(page.locator('[data-wasm-ready="true"]')).toBeVisible()
  await page.locator('input[type=file]').setInputFiles([{
    name: 'wasm.png',
    mimeType: 'image/png',
    buffer: readFileSync(resolve('tests/fixtures/sample.png'))
  }])
  const slider = page.locator('input[type=range]').last()
  await slider.evaluate(el => {
    const input = el as HTMLInputElement
    input.value = '1.2'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    input.dispatchEvent(new Event('change', { bubbles: true }))
  })
  await expect(page.getByText('亮度 1.20 ×')).toBeVisible()
  const [download] = await Promise.all([
    page.waitForEvent('download', { timeout: 30000 }),
    page.getByRole('button', { name: '导出商品图 ZIP' }).click()
  ])
  expect(download.suggestedFilename()).toContain('commerce-assets.zip')
  await expect(page.getByText('上次执行引擎：WebAssembly')).toBeVisible()
})



