import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
if (process.env.GITHUB_ACTIONS !== 'true') throw Error('Browser acceptance only in Actions');
await fs.mkdir('evidence', {recursive:true});
const browser = await chromium.launch({headless:true, executablePath:process.env.CHROME_PATH, chromiumSandbox:true});
const layouts = [];
try {
  for (const width of [1440, 1024, 768, 390, 320]) {
    const page = await browser.newPage({viewport:{width, height:960}});
    const errors = [], destinations = new Set();
    page.on('pageerror', error => errors.push(error.message));
    page.on('request', request => destinations.add(new URL(request.url()).hostname));
    await page.goto('http://127.0.0.1:8046/', {waitUntil:'domcontentloaded'});
    await page.locator('#media-file').setInputFiles('tests/source.mp4');
    await page.getByTestId('source-preview').waitFor();
    const font = await page.evaluate(async () => {
      const faces = await document.fonts.load('400 16px "Noto Sans SC Variable"', '视频工坊文件压缩');
      return faces.map(face => face.status);
    });
    assert(font.length && font.every(status => status === 'loaded'));
    const cases = [];
    for (const [name, expectedWidth, expectedDuration] of [
      ['压缩视频', 320, 2], ['调整尺寸', 160, 2], ['截取片段', 320, 0.75], ['转换格式', 320, 2],
    ]) {
      const control = page.getByRole('button', {name, exact:true});
      await control.focus();
      await page.keyboard.press('Enter');
      assert.equal(await control.getAttribute('aria-pressed'), 'true');
      assert.equal(await page.getByTestId('output-preview').count(), 0, 'Old results must clear when tool changes');
      if (name === '调整尺寸') await page.getByRole('spinbutton', {name:'宽度', exact:true}).fill('160');
      if (name === '截取片段') {
        await page.getByRole('spinbutton', {name:'开始秒数', exact:true}).fill('0.5');
        await page.getByRole('spinbutton', {name:'片段时长（秒）', exact:true}).fill('0.75');
      }
      if (name === '转换格式') await page.getByRole('combobox', {name:'输出格式', exact:true}).selectOption('webm');
      await page.getByRole('button', {name:'生成视频', exact:true}).click();
      const video = page.getByTestId('output-preview');
      await video.waitFor({timeout:90000});
      await page.waitForFunction(() => document.querySelector('[data-testid="output-preview"]')?.readyState >= 2);
      const decoded = await video.evaluate(async element => {
        element.muted = true;
        const frame = new Promise((resolve, reject) => {
          const timer = setTimeout(() => reject(Error('No decoded video frame')), 10000);
          element.requestVideoFrameCallback(() => {clearTimeout(timer); resolve(true);});
        });
        await element.play();
        await frame;
        element.pause();
        return {width:element.videoWidth, height:element.videoHeight, duration:element.duration};
      });
      assert.equal(decoded.width, expectedWidth);
      assert(Math.abs(decoded.duration - expectedDuration) < 0.2);
      const downloaded = await page.getByRole('link', {name:'下载视频', exact:true}).evaluate(async anchor => {
        const response = await fetch(anchor.href);
        return {status:response.status, size:(await response.arrayBuffer()).byteLength};
      });
      assert.equal(downloaded.status, 200); assert(downloaded.size > 100);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
      cases.push({tool:name, ...decoded, actualFrameDecoded:true, download:downloaded});
    }
    await page.screenshot({path:`evidence/media-${width}.png`, fullPage:true});
    await page.getByRole('button', {name:'切换主题', exact:true}).click();
    await page.screenshot({path:`evidence/media-dark-${width}.png`, fullPage:true});
    await page.getByRole('button', {name:'移除文件', exact:true}).click();
    await page.getByTestId('source-preview').waitFor({state:'detached'});
    assert.equal(await page.getByTestId('output-preview').count(), 0);
    assert.equal(await page.getByRole('link', {name:'下载视频', exact:true}).count(), 0);
    await page.locator('#media-file').setInputFiles({name:'broken.mp4', mimeType:'video/mp4', buffer:Buffer.from('not a video')});
    await page.getByRole('alert').filter({hasText:'无法读取视频'}).waitFor();
    assert.equal(await page.getByTestId('output-preview').count(), 0);
    assert.deepEqual(errors, []);
    assert.deepEqual([...destinations], ['127.0.0.1'], 'UI must not load external resources');
    layouts.push({width, cases, chineseFont:font, keyboard:true, deletion:true, invalidInput:true});
    await page.close();
  }
  await fs.writeFile('evidence/browser.json', JSON.stringify({commit:process.env.GITHUB_SHA,
    browser:await browser.version(), layouts}, null, 2));
} finally {await browser.close();}
