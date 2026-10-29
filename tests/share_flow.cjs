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
  const temporary = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'microbin-browser-'));
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
    // More than 64 MiB in several files must use chunks, even if each file is small.
    await page.goto(base, {waitUntil: 'networkidle'});
    const MiB = 1024 * 1024;
    const payloads = [Buffer.alloc(33 * MiB, 0x5a), Buffer.alloc(33 * MiB, 0x7b), Buffer.alloc(0)];
    const names = ['分片一.bin', '分片二.bin', '空文件.bin'];
    const paths = payloads.map((buffer, i) => { const target = path.join(temporary, names[i]); fs.writeFileSync(target, buffer); return target; });
    await page.locator('#file').setInputFiles(paths);
    const offsets = [];
    let failures = 0;
    let maximumBody = 0;
    await page.route('**/upload/chunks/*/*', async route => {
      if (route.request().method() !== 'PUT') return route.continue();
      offsets.push(new URL(route.request().url()).pathname.split('/').pop());
      maximumBody = Math.max(maximumBody, route.request().postDataBuffer().length);
      if (failures++ === 0) return route.abort('failed');
      if (failures === 2) {
        await route.fetch(); // Server accepted the chunk; simulate losing its response.
        return route.abort('failed');
      }
      return route.continue();
    });
    await Promise.all([page.waitForURL(/\/upload\//, {timeout: 60000}), page.locator('#submit-button').click()]);
    assert.deepEqual(offsets.slice(0, 3), ['0', '0', '0']);
    assert(maximumBody <= 16 * MiB);
    const chunkedSlug = new URL(page.url()).pathname.split('/').pop();
    const crypto = require('node:crypto');
    const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
    for (let i = 0; i < names.length; i++) {
      const response = await context.request.get(`${base}/file/${chunkedSlug}?fname=${encodeURIComponent(names[i])}`);
      assert.equal(response.status(), 200);
      assert.equal(digest(await response.body()), digest(payloads[i]), names[i]);
    }
    await page.unroute('**/upload/chunks/*/*');
    // A stalled chunk can be cancelled without leaving the submit button disabled.
    await page.goto(base, {waitUntil: 'networkidle'});
    const cancelledFile = path.join(temporary, '取消.bin');
    const fd = fs.openSync(cancelledFile, 'w'); fs.ftruncateSync(fd, 65 * MiB); fs.closeSync(fd);
    await page.locator('#file').setInputFiles(cancelledFile);
    let stalled;
    const pending = new Promise(resolve => { stalled = resolve; });
    await page.route('**/upload/chunks/*/*', async route => { stalled(route); });
    await page.locator('#submit-button').click();
    const route = await pending;
    assert(await page.locator('#upload-progress').isVisible());
    await page.locator('#upload-cancel').click();
    await page.waitForFunction(() => !document.getElementById('submit-button').disabled);
    assert((await page.locator('#upload-status').innerText()).includes('已取消上传'));
    assert(!await page.locator('#upload-cancel').isVisible());
    await route.abort('aborted');
    await page.unroute('**/upload/chunks/*/*');
    await page.goto(base, {waitUntil: 'networkidle'});
    await page.locator('#file').setInputFiles(cancelledFile);
    let rejected = 0;
    await page.route('**/upload/chunks/*/*', async route => {
      rejected++;
      await route.fulfill({status: 413, body: 'Too large'});
    });
    await page.locator('#submit-button').click();
    await page.waitForFunction(() => !document.getElementById('submit-button').disabled && document.getElementById('upload-status').textContent.includes('大小限制'));
    assert.equal(rejected, 1, 'Permanent HTTP errors must not retry');
    await page.unroute('**/upload/chunks/*/*');
    await page.goto(base, {waitUntil: 'networkidle'});
    const oversized = path.join(temporary, '超限.bin');
    const hugeFd = fs.openSync(oversized, 'w'); fs.ftruncateSync(hugeFd, 2048 * MiB + 1); fs.closeSync(hugeFd);
    await page.locator('#file').setInputFiles([paths[2], oversized]);
    await page.locator('#submit-button').click();
    assert((await page.locator('#attach-file-button').innerText()).includes('超限.bin 超过单文件'));
    assert(!await page.locator('#upload-progress').isVisible());
    assert.deepEqual(errors, []);
    console.log('PASS: browser chunk selection, bounded requests, network retry, lost response, exact file hashes and cancellation');
    console.log('PASS: browser upload, Chinese content/filename, copy controls, inline/standalone QR decoding, mobile layout and guide');
  } finally {
    await browser.close();
    fs.rmSync(temporary, {recursive: true, force: true});
  }
})().catch(error => {console.error(error); process.exit(1);});
