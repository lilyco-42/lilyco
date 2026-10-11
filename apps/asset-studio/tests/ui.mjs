import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const browser = await chromium.launch();
await fs.mkdir('evidence', {recursive:true});
for (const [label,width,height] of [['desktop',1440,1000],['mobile',390,844]]) {
  const page = await browser.newPage({viewport:{width,height}});
  const errors=[]; page.on('pageerror',e=>errors.push(e.message));
  await page.goto('http://127.0.0.1:8043/');
  await page.getByRole('button',{name:'切换浅色外观'}).click();
  assert.equal(await page.locator('html').evaluate(el=>el.classList.contains('dark')),false);
  await page.getByRole('button',{name:'切换深色外观'}).click();
  await page.locator('#files').setInputFiles('tests/demo.pxls');
  await page.locator('#inspect').click();
  await page.locator('#status').filter({hasText:'已读取 1 个动作'}).waitFor();
  await page.locator('#render').click();
  await page.locator('#status').filter({hasText:'处理完成'}).waitFor({timeout:30000});
  const image = page.locator('#gallery img').first();
  await image.waitFor();
  await page.waitForFunction(()=>document.querySelector('#gallery img')?.naturalWidth>0);
  assert(await image.evaluate(img=>img.complete && img.naturalWidth>0));
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  assert((await page.locator('#gallery').boundingBox()).height<=680, 'preview must not expand the entire page');
  const download=await page.request.get('http://127.0.0.1:8043'+await page.locator('#download').getAttribute('href'));
  assert(download.ok()); assert((await download.body()).subarray(0,2).equals(Buffer.from('PK')));
  await page.screenshot({path:`evidence/${label}.png`,fullPage:true});
  await page.locator('#spineTab').click();
  if (process.env.LAIN42_LYCO_SPINE_RENDER) {
    await page.locator('#files').setInputFiles(['tests/demo.json','tests/demo.atlas','tests/demo.png']);
    await page.locator('#inspect').click();
    await page.locator('#status').filter({hasText:'已读取 1 个动作'}).waitFor();
    assert.equal(await page.locator('#motion').inputValue(),'wave');
    await page.locator('#render').click();
    await page.locator('#status').filter({hasText:'处理完成'}).waitFor({timeout:30000});
    await page.waitForFunction(()=>document.querySelector('#gallery img')?.naturalWidth===512);
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.screenshot({path:`evidence/spine-${label}.png`,fullPage:true});
  } else {
    await page.locator('#engineNotice').filter({hasText:'不包含官方 Spine'}).waitFor();
    assert(await page.locator('#render').isDisabled());
  }
  await page.locator('#delete').click();
  await page.locator('#status').filter({hasText:'工作区已清理'}).waitFor();
  assert.equal(await page.locator('#gallery img').count(),0);
  assert.deepEqual(errors,[]);
  await page.close();
}
await browser.close();
