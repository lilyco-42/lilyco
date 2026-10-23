import {test,expect} from '@playwright/test'
import {readFileSync} from 'node:fs'
import {resolve} from 'node:path'
import {pathToFileURL} from 'node:url'

test.use({channel:'chrome',acceptDownloads:true})

test('AI worker is lazy, user-triggered, and never uploads source images',async({page,context})=>{
  const outgoing:string[]=[]
  page.on('request',request=>{if(request.method()==='POST'||request.method()==='PUT')outgoing.push(request.url())})
  let loaded=0
  await context.route('**/dist/transformers.web.min.js',route=>route.continue())
  await context.route('https://cdn.jsdelivr.net/npm/@huggingface/transformers@3.8.1/dist/transformers.web.min.js',async route=>{
    loaded++
    await route.fulfill({status:200,contentType:'text/javascript',headers:{'access-control-allow-origin':'*'},body:`
export const env = {};
export const RawImage = { async fromBlob(blob){ return { blob }; } };
export async function pipeline(task,model,options){
  options.progress_callback?.({status:'progress',progress:100});
  return async image => ({ async toBlob(){ return image.blob; } });
}
`})
  })
  await page.goto(pathToFileURL(resolve('dist/index.html')).href)
  await expect(page.getByText('本机 AI 抠图')).toBeVisible()
  expect(loaded).toBe(0)
  await page.locator('input[type=file]').setInputFiles([{name:'sample.png',mimeType:'image/png',buffer:readFileSync(resolve('tests/fixtures/sample.png'))}])
  await page.getByRole('button',{name:'对选中图片 AI 抠图'}).click()
  await expect(page.getByText(/完成：Xenova\/modnet/)).toBeVisible({timeout:12000})
  expect(loaded).toBe(1)
  expect(outgoing).toEqual([])
  await expect(page.getByRole('combobox',{name:'背景'})).toHaveValue('transparent')
  await expect(page.getByRole('combobox',{name:'导出格式'})).toHaveValue('png')
})
