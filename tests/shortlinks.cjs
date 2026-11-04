const assert = require('node:assert/strict');
const {chromium} = require('playwright');
(async () => {
  const base = process.argv[2];
  const browser = await chromium.launch({...(process.env.CHROMIUM_PATH ? {executablePath: process.env.CHROMIUM_PATH} : {}), args: ['--no-sandbox']});
  try {
    const context = await browser.newContext({permissions: ['clipboard-read', 'clipboard-write'], viewport: {width: 1280, height: 1000}});
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    if (process.argv.includes('--immutable')) {
      await page.goto(base + '/list', {waitUntil: 'networkidle'});
      await page.locator('#select-all').check();
      assert.equal(await page.locator('#delete-selected').isDisabled(), true);
      assert.equal(await page.locator('#copy-selected').isDisabled(), false);
      assert((await page.locator('#batch-status').textContent()).includes('受保护'));
      console.log('PASS: immutable public shares allow batch copying and block batch deletion');
      await context.close();
      return;
    }
    await page.goto(base, {waitUntil: 'networkidle'});
    assert.equal(await page.locator('#short-link-format').inputValue(), 'lower5');
    await page.locator('#short-link-format').selectOption('custom');
    await page.locator('#short-link-custom').fill('browser-[2-3]{2}');
    await page.waitForFunction(() => document.querySelector('#link-format-hint').textContent.includes('/browser-'));
    await page.locator('#content-input').fill('browser custom format');
    await Promise.all([page.waitForURL(/\/upload\/browser-[23]{2}$/), page.locator('#submit-button').click()]);
    assert.equal(await page.locator('#share-url').inputValue(), base + new URL(page.url()).pathname.replace('/upload', ''));
    await page.goto(base, {waitUntil: 'networkidle'});
    await page.locator('#short-link-format').selectOption('custom');
    await page.locator('#short-link-custom').fill('list');
    await page.waitForFunction(() => document.querySelector('#link-format-hint').textContent.includes('耗尽'));
    assert.equal(await page.locator('#short-link-format').evaluate(select => select.checkValidity()), false);
    let uploads = 0;
    page.on('request', req => { if (req.method() === 'POST' && req.url() === base + '/upload') uploads++; });
    await page.locator('#content-input').fill('must not upload');
    await page.locator('#submit-button').click();
    await page.waitForTimeout(100);
    assert.equal(uploads, 0);
    await page.goto(base + '/list', {waitUntil: 'networkidle'});
    const rows = page.locator('.share-select');
    const count = await rows.count();
    assert(count >= 5);
    await rows.nth(0).click();
    await rows.nth(3).click({modifiers: ['Shift']});
    assert.equal(await page.locator('.share-select:checked').count(), 4);
    assert.equal(await page.locator('#select-all').evaluate(box => box.indeterminate), true);
    await page.locator('#clear-selection').click();
    await rows.nth(1).click();
    await page.locator('#select-range').click();
    await rows.nth(4).click();
    assert.equal(await page.locator('.share-select:checked').count(), 4);
    assert.equal(await page.locator('#select-range').getAttribute('aria-pressed'), 'false');
    await page.locator('#select-all').check();
    assert.equal(await page.locator('.share-select:checked').count(), count);
    assert.equal(await page.locator('#delete-selected').isDisabled(), false);
    await page.locator('#copy-selected').click();
    await page.waitForFunction(() => document.querySelector('#batch-status').textContent.includes('已复制'));
    const clipboard = await page.evaluate(() => navigator.clipboard.readText());
    assert.equal(clipboard.split('\n').length, count);
    assert(clipboard.split('\n').every(url => url.startsWith(base + '/')));
    await page.locator('#clear-selection').click();
    await page.keyboard.press('Control+a');
    assert.equal(await page.locator('.share-select:checked').count(), count);
    await page.locator('#clear-selection').click();
    const eligible = page.locator('.share-select[data-deletable="true"]');
    const deleting = await eligible.evaluateAll(boxes => boxes.slice(0, 2).map(box => box.value));
    await eligible.nth(0).check(); await eligible.nth(1).check();
    page.once('dialog', dialog => dialog.dismiss());
    await page.locator('#delete-selected').click();
    assert.equal(await rows.count(), count, 'Cancel preserves the list');
    page.once('dialog', dialog => dialog.accept());
    await page.locator('#delete-selected').click();
    await page.waitForFunction(expected => document.querySelectorAll('.share-select').length === expected, count - 2);
    for (const slug of deleting) assert.equal((await context.request.get(base + '/' + slug, {maxRedirects: 0})).status(), 404);
    for (const width of [1280, 390]) {
      await page.setViewportSize({width, height: 1000});
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `List overflow at ${width}`);
      assert(await page.locator('#select-all').isVisible());
      if (process.env.SCREENSHOT_DIR) {
        require('node:fs').mkdirSync(process.env.SCREENSHOT_DIR, {recursive: true});
        await page.screenshot({path: `${process.env.SCREENSHOT_DIR}/list-${width}.png`, fullPage: true});
      }
    }
    assert.deepEqual(errors, []);
    console.log('PASS: custom-format upload/exhaustion, Shift/touch range selection, select all/keyboard, clipboard, protected rows, confirmed batch deletion and mobile layout');
    await context.close();
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
