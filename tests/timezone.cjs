// Browser timezone regression: NODE_PATH=... node tests/timezone.cjs
const assert = require('node:assert/strict');
const path = require('node:path');
const {chromium} = require('playwright');

(async () => {
  const browser = await chromium.launch({
    ...(process.env.CHROMIUM_PATH ? {executablePath: process.env.CHROMIUM_PATH} : {}),
    args: ['--no-sandbox'],
  });
  try {
    // Winter and summer exercise New York's DST; Shanghai crosses into the next year.
    for (const [timezoneId, winter, summer] of [
      ['Asia/Shanghai', '01-01 04:30', '07-01 04:30'],
      ['America/New_York', '12-31 15:30', '06-30 16:30'],
      ['UTC', '12-31 20:30', '06-30 20:30'],
    ]) {
      const context = await browser.newContext({timezoneId});
      const page = await context.newPage();
      const dates = ['2025-12-31T20:30:00Z', '2026-06-30T20:30:00Z'];
      await page.setContent(dates.map(date => `<time data-unix="${Date.parse(date) / 1000}">UTC fallback</time>`).join('') + '<span id="never">永久</span>');
      await page.addScriptTag({path: path.resolve(__dirname, '../templates/assets/theme.js')});
      assert.deepEqual(await page.locator('time').allTextContents(), [winter, summer]);
      assert.deepEqual(await page.locator('time').evaluateAll(times => times.map(time => time.dateTime)), dates.map(date => new Date(date).toISOString()));
      assert((await page.locator('time').first().getAttribute('title')).includes(timezoneId));
      assert.equal(await page.locator('#never').innerText(), '永久');
      await context.close();
    }
    console.log('PASS: browser timezone conversion handles Shanghai, New York DST, UTC and year/day boundaries');
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exit(1); });
