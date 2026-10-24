/**
 * Deterministic HTML capture on the user's own Chrome browser.
 * Driven by media_mcp.py. No network access is permitted during rendering.
 */
import { chromium } from 'playwright-core';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import fs from 'node:fs';

const [source, frameDir, frameCountStr, fpsStr, widthStr, heightStr] = process.argv.slice(2);
const frames = Number(frameCountStr);
const fps = Number(fpsStr);
const width = Number(widthStr);
const height = Number(heightStr);
if (!source || !frameDir || !Number.isInteger(frames) || frames < 1 || frames > 900 ||
    !Number.isFinite(fps) || fps < 1 || fps > 30 ||
    !Number.isInteger(width) || width < 320 || width > 1920 ||
    !Number.isInteger(height) || height < 240 || height > 1080) {
  throw Error('invalid HTML capture parameters');
}
const chrome = process.env.LILYCO_CHROME_BIN;
const browser = await chromium.launch({
  ...(chrome ? { executablePath: chrome } : { channel: 'chrome' }),
  headless: true,
  args: ['--disable-background-networking'],
});
try {
  const context = await browser.newContext({
    viewport: { width, height },
    deviceScaleFactor: 1,
    serviceWorkers: 'block',
  });
  await context.route('**/*', route => {
    const url = route.request().url();
    if (/^(file:|data:|blob:)/.test(url)) return route.continue();
    return route.abort();
  });
  const page = await context.newPage();
  await page.goto(pathToFileURL(path.resolve(source)).href, {waitUntil:'load', timeout:20000});
  if (!await page.evaluate(() => typeof window.seek === 'function')) {
    throw Error('HTML must provide window.seek(t) for deterministic rendering');
  }
  for (let i = 0; i < frames; i++) {
    await page.evaluate(t => window.seek(t), i / fps);
    await page.screenshot({
      path: path.join(frameDir, 'frame-' + String(i).padStart(5,'0') + '.png'),
      animations: 'disabled',
    });
  }
  console.log(JSON.stringify({frames, fps, width, height}));
} finally {
  await browser.close();
}
