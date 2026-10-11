// Run the actual frozen WebView2 window, not an ordinary Chromium replacement.
import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import net from 'node:net';
import { spawn, spawnSync } from 'node:child_process';
if (process.platform !== 'win32' || process.env.GITHUB_ACTIONS !== 'true') throw Error('CI-only Windows acceptance');
await fs.mkdir('evidence', {recursive:true});
const probe = net.createServer();
await new Promise(resolve=>probe.listen(0,'127.0.0.1',resolve));
const port = probe.address().port;
await new Promise(resolve=>probe.close(resolve));
const receiptFile = path.resolve('evidence/native-startup.json');
const env = {...process.env, LAIN42_DESKTOP_SMOKE:'true', LAIN42_DESKTOP_DEBUG_PORT:String(port), LAIN42_DESKTOP_RECEIPT:receiptFile};
delete env.LAIN42_LAIC; delete env.LAIN42_LYCO_SPINE_RENDER;
const binary=path.resolve(process.argv[2]);
const child=spawn(binary,[],{env,stdio:['ignore','pipe','pipe']});
let output=''; child.stdout.on('data',x=>output+=x); child.stderr.on('data',x=>output+=x);
let browser;
try {
  let receipt;
  for (let i=0;i<120;i++) {
    assert.equal(child.exitCode,null,output);
    try {receipt=JSON.parse(await fs.readFile(receiptFile,'utf8')); browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`); break;} catch {await new Promise(r=>setTimeout(r,500));}
  }
  assert(browser && receipt,'native WebView2 did not expose CI debugging endpoint: '+output);
  const page=browser.contexts()[0].pages()[0];
  assert(page,'actual native page missing');
  await page.waitForURL(receipt.url);
  await page.locator('#files').waitFor({state:'attached'});
  await page.locator('#files').setInputFiles('tests/demo.pxls');
  await page.locator('#inspect').click();
  await page.locator('#status').filter({hasText:'已读取 1 个动作'}).waitFor();
  await page.locator('#render').click();
  await page.locator('#status').filter({hasText:'处理完成'}).waitFor({timeout:30000});
  await page.waitForFunction(()=>document.querySelector('#gallery img')?.naturalWidth>0);
  // The WebView uses the same HttpOnly cookie; API output is a real downloadable ZIP.
  // CDP-attached WebView2 contexts don't expose their native cookie store to
  // Playwright's separate APIRequestContext. Read through the actual page so
  // its own HttpOnly cookie is sent, just as when the user clicks download.
  const exported=await page.evaluate(async()=>{
    const response=await fetch(document.querySelector('#download').getAttribute('href'));
    const bytes=new Uint8Array(await response.arrayBuffer());
    return {status:response.status,zip:bytes[0]===80&&bytes[1]===75,size:bytes.length};
  });
  assert.equal(exported.status,200,JSON.stringify(exported));
  assert(exported.zip && exported.size>100);
  await page.evaluate(()=>scrollTo(0,0));
  const layout=await page.evaluate(()=>({width:innerWidth,height:innerHeight,previewTop:document.querySelector('#gallery').getBoundingClientRect().top}));
  assert(layout.previewTop<layout.height,'native desktop preview must be visible without scrolling: '+JSON.stringify(layout));
  await page.screenshot({path:'evidence/windows-native.png',fullPage:true});
  const nativeTitle=await page.title();
  await fs.access(receipt.workspace_root);
  // Close via the normal window message. Do not terminate(): cleanup is part of acceptance.
  const close=spawnSync('powershell',['-NoProfile','-NonInteractive','-File',
    'tests/close-native-window.ps1','-Binary',binary],{encoding:'utf8'});
  assert.equal(close.status,0,close.stderr);
  for(let i=0;i<100 && child.exitCode===null;i++) await new Promise(r=>setTimeout(r,200));
  assert.equal(child.exitCode,0,'normal window close must exit cleanly: '+output);
  await assert.rejects(fs.access(receipt.workspace_root),'temporary workspace must be removed on window close');
  await assert.rejects(fetch(receipt.url),'loopback service must stop on window close');
  await fs.writeFile('evidence/windows-native.json',JSON.stringify({commit:process.env.APP_COMMIT||process.env.GITHUB_SHA,native_webview2:true,title:nativeTitle,layout,real_aic_render:true,real_export_zip:true,normal_close:true,workspace_removed:true,port_closed:true},null,2));
} finally {
  if(child.exitCode===null) child.kill();
  if(browser) await browser.close().catch(()=>{});
  await fs.writeFile('evidence/windows-native.log',output);
  if(child.exitCode!==0) console.error(output);
}
