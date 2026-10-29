// Optional browser checks; launched by share_flow.py --browser.
const assert = require('node:assert/strict');
const { chromium } = require('playwright');
const { PNG } = require('pngjs');
const jsQR = require('jsqr');
const fs = require('node:fs');
const path = require('node:path');

(async () => {
  const [base, serializedIds] = process.argv.slice(2);
  const ids = JSON.parse(serializedIds);
  const browser = await chromium.launch({
    ...(process.env.CHROMIUM_PATH ? {executablePath: process.env.CHROMIUM_PATH} : {}),
    args: ['--no-sandbox'],
  });
  try {
    const context = await browser.newContext({viewport: {width: 1280, height: 1000}, permissions: ['clipboard-read', 'clipboard-write']});
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(base, {waitUntil: 'networkidle'});
    assert.equal(await page.locator('html').getAttribute('lang'), 'zh-CN');
    assert.equal(await page.locator('#submit-button').inputValue(), '创建分享');
    const original = 'Copy\nContent\n中文原文 ` ${value} <script> & \\';
    await page.locator('#content-input').fill(original);
    await page.locator('#file').setInputFiles({name: '中文图片.png', mimeType: 'image/png', buffer: PNG.sync.write(new PNG({width: 2, height: 2}))});
    assert((await page.locator('#attach-file-button').innerText()).includes('中文图片.png'));
    await Promise.all([page.waitForURL(/\/upload\//), page.locator('#submit-button').click()]);
    await page.waitForLoadState('networkidle');
    const slug = new URL(page.url()).pathname.split('/').pop();
    assert.equal(await page.locator('#share-url').inputValue(), `${base}/${slug}`);
    assert.equal(await page.locator('#management-url').inputValue(), `${base}/upload/${slug}`);
    await page.locator('#copy-url-button').click();
    await page.waitForFunction(() => document.querySelector('#copy-url-button').textContent === '已复制');
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), `${base}/${slug}`);
    await page.locator('#copy-text-button').click();
    await page.waitForFunction(() => document.querySelector('#copy-text-button').textContent === '已复制');
    assert.equal((await page.evaluate(() => navigator.clipboard.readText())).replace(/\r\n/g, '\n'), original);
    const png = PNG.sync.read(await page.locator('.share-qr svg').screenshot());
    const qr = jsQR(new Uint8ClampedArray(png.data), png.width, png.height);
    assert.equal(qr?.data, `${base}/${slug}`, 'QR must contain the exact public short link');
    await page.locator('.more-links summary').click();
    await page.locator('[data-copy-target="management-url"]').click();
    await page.waitForTimeout(100);
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), `${base}/upload/${slug}`);
    await page.locator('[data-markdown]').click();
    await page.waitForTimeout(100);
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), `![图片](<${base}/file/${slug}?preview=true>)`);
    for (const width of [1280, 390]) {
      await page.setViewportSize({width, height: 1000});
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `Overflow at ${width}px`);
      assert(await page.locator('.share-qr').isVisible());
      if (process.env.SCREENSHOT_DIR) {
        fs.mkdirSync(process.env.SCREENSHOT_DIR, {recursive: true});
        await page.screenshot({path: path.join(process.env.SCREENSHOT_DIR, `share-${width}.png`), fullPage: true});
      }
    }
    await page.goto(`${base}/qr/${slug}`, {waitUntil: 'networkidle'});
    const standalone = PNG.sync.read(await page.locator('.qr-card svg').screenshot());
    assert.equal(jsQR(new Uint8ClampedArray(standalone.data), standalone.width, standalone.height)?.data, `${base}/${slug}`);
    await page.goto(`${base}/edit/${slug}`, {waitUntil: 'networkidle'});
    assert.equal(await page.locator('#content').inputValue(), original);
    assert.equal(await page.locator('#submit-button').inputValue(), '保存修改');
    await page.goto(`${base}/list`, {waitUntil: 'networkidle'});
    await page.locator(`.copy-button[data-url="${base}/${ids.url}"]`).click();
    await page.waitForTimeout(100);
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), `${base}/${ids.url}`);
    await page.goto(`${base}/guide`, {waitUntil: 'networkidle'});
    assert((await page.locator('.help').innerText()).includes('结果页直接显示分享二维码'));
    assert.deepEqual(errors, []);
    console.log('PASS: browser upload, Chinese content/filename, copy controls, inline/standalone QR decoding, mobile layout and guide');
  } finally {
    await browser.close();
  }
})().catch(error => {console.error(error); process.exit(1);});
