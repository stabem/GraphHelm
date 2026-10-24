// #1991 observer: the Lanes board at 1440x900 and 1600x1000 in headless Chromium. Every board
// column's right edge must sit inside the viewport and the page must not scroll sideways.
// Usage: node lanes-layout-check.mjs <studio url> <out dir>   (@playwright/test must resolve)
import { createRequire } from 'node:module';
const require = createRequire(process.env.PW_FROM || import.meta.url);
const { chromium } = require('@playwright/test');
const [url, out] = process.argv.slice(2);
const browser = await chromium.launch();
let bad = 0;
for (const [width, height] of [[1440, 900], [1600, 1000]]) {
  const page = await browser.newPage({ viewport: { width, height } });
  await page.goto(url);
  await page.getByText('Lanes', { exact: true }).first().click({ timeout: 30000 });
  await page.locator('.board').waitFor({ timeout: 30000 });
  await page.waitForTimeout(800);
  const r = await page.evaluate(() => {
    const cols = [...document.querySelectorAll('.board > .board-column')].map((c) => Math.round(c.getBoundingClientRect().right));
    const widths = [...document.querySelectorAll('.board > .board-column')].map((c) => ({ folded: c.classList.contains('folded'), w: Math.round(c.getBoundingClientRect().width) }));
    const el = document.scrollingElement;
    const board = document.querySelector('.board');
    return { cols, widths, vw: window.innerWidth, pageScroll: el.scrollWidth > el.clientWidth, boardScroll: board.scrollWidth > board.clientWidth };
  });
  // #1996: a column with cards is at least 220px wide; one without folds to 44px.
  const ok = r.cols.length >= 6 && r.cols.every((x) => x <= r.vw) && !r.pageScroll && !r.boardScroll
    && r.widths.every((c) => (c.folded ? c.w === 44 : c.w >= 220));
  console.log(`${width}x${height}`, JSON.stringify(r), ok ? 'OK' : 'FAIL');
  if (!ok) bad = 1;
  await page.screenshot({ path: `${out}/lanes-${width}x${height}.png` });
  await page.close();
}
await browser.close();
process.exit(bad);
