import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
if(process.env.GITHUB_ACTIONS!=='true') throw Error('Browser acceptance only in Actions');
await fs.mkdir('evidence',{recursive:true});
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_PATH,chromiumSandbox:true});
const layouts=[];
try{
  for(const width of [1440,1028,768,390,320]){
    const height=width===1028?749:980;
    const page=await browser.newPage({viewport:{width,height},deviceScaleFactor:1});
    const errors=[];page.on('pageerror',e=>errors.push(e.message));
    await page.goto('http://127.0.0.1:8043/');
    await page.locator('#original').setInputFiles('tests/original.png');
    await page.locator('#cropStage').waitFor();
    if(width>=1024){
      const before=await page.locator('#cropStage').evaluate(e=>e.getBoundingClientRect().height);
      await page.getByRole('button',{name:'放大原图',exact:true}).click();
      assert((await page.locator('#cropStage').evaluate(e=>e.getBoundingClientRect().height))>before,'expanded crop should provide more selection space');
    }
    const drag=await page.locator('#cropStage').evaluate(svg=>{
      const matrix=svg.getScreenCTM();
      const a=new DOMPoint(24.25,20.25).matrixTransform(matrix),b=new DOMPoint(72.25,60.25).matrixTransform(matrix);
      return {a:{x:a.x,y:a.y},b:{x:b.x,y:b.y}};
    });
    await page.mouse.move(drag.a.x,drag.a.y);await page.mouse.down();
    await page.mouse.move(drag.b.x,drag.b.y,{steps:5});await page.mouse.up();
    assert.equal(await page.locator('#param-region').inputValue(),'24,20,72,60');
    if(width>=1024){await page.getByRole('button',{name:'收起原图',exact:true}).click();await page.evaluate(()=>scrollTo(0,0));}
    await page.locator('#render').click();
    await page.locator('#status').filter({hasText:'处理完成'}).waitFor({timeout:30000});
    await page.waitForFunction(()=>document.querySelector('#gallery img')?.naturalWidth===48);
    assert.equal(await page.locator('#gallery img').evaluate(e=>e.naturalHeight),40);
    if(width===1028) assert(await page.locator('#gallery img').evaluate(e=>e.getBoundingClientRect().bottom<=innerHeight),'native CSS viewport should show the whole cropped preview');
    const zip=await page.evaluate(async()=>{const r=await fetch(document.querySelector('#downloadZip').href);const b=new Uint8Array(await r.arrayBuffer());return{status:r.status,magic:Array.from(b.slice(0,2))};});
    assert.equal(zip.status,200);assert.deepEqual(zip.magic,[80,75]);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    await page.screenshot({path:`evidence/vision-${width}.png`,fullPage:true});
    await page.getByRole('button',{name:'主色提取',exact:true}).click();
    await page.locator('#render').click();
    await page.locator('[aria-label="提取的主色"]').waitFor();
    assert.match(await page.locator('[aria-label="提取的主色"]').innerText(),/#ff0000/i);
    await page.getByRole('button',{name:'位图转 SVG',exact:true}).click();
    await page.locator('#param-scale').fill('1');
    await page.locator('#render').click();
    await page.waitForFunction(()=>document.querySelector('#gallery img')?.src.endsWith('traced.svg')&&document.querySelector('#gallery img').naturalWidth>0);
    await page.getByRole('button',{name:'图片对比',exact:true}).click();
    await page.locator('#rebuilt').setInputFiles('tests/rebuilt.png');
    await page.locator('#render').click();
    await page.waitForFunction(()=>document.querySelector('#gallery img')?.src.endsWith('heatmap.png')&&document.querySelector('#gallery img').naturalWidth===96);
    await page.getByRole('button',{name:'清空',exact:true}).click();
    await page.locator('#originalPreview').waitFor({state:'detached'});
    assert.equal(await page.locator('#downloadZip').count(),0);
    await page.getByRole('button',{name:'HTML 截图',exact:true}).click();
    await page.locator('#original').setInputFiles('tests/page.html');
    await page.locator('#param-width').fill('640');await page.locator('#param-height').fill('480');
    await page.locator('#render').click();
    await page.waitForFunction(()=>document.querySelector('#gallery img')?.src.endsWith('screenshot.png')&&document.querySelector('#gallery img').naturalWidth===640,{},{timeout:40000});
    await page.getByRole('button',{name:'切换主题'}).click();
    await page.screenshot({path:`evidence/vision-dark-${width}.png`,fullPage:true});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    assert.deepEqual(errors,[]);
    layouts.push({width,pointer_crop:true,real_crop_pixels:true,colors:true,real_svg:true,comparison:true,html_screenshot:true,clear:true,pageErrors:errors});
    await page.close();
  }
  await fs.writeFile('evidence/browser.json',JSON.stringify({commit:process.env.GITHUB_SHA,layouts},null,2));
}finally{await browser.close();}
