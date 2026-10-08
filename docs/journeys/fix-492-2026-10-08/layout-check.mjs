// #491 observer: the owner's arrangement at 1290x800 (Projects rail open, chat open, the planner's
// direct conversation open), in a real headless Chromium. Reports the canvas width, whether the
// right panel shows, and every canvas control that overlaps right-panel text; saves a screenshot.
// Usage: node layout-check.mjs <url> <out.png>
import { chromium } from '@playwright/test';

const [url, out] = process.argv.slice(2);
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1290, height: 800 } });
await page.goto(url);
await page.getByRole('tab', { name: 'Journey', exact: true }).waitFor({ timeout: 30000 });
if (!(await page.locator('.app.projects-open').count())) await page.getByRole('button', { name: 'Toggle projects' }).click();
const side = page.locator('.studio-columns > .chat-col:not([hidden]), .studio-columns > .node-col:not([hidden])');
for (const opener of [page.getByRole('button', { name: /^planner, / }).first(), page.getByRole('button', { name: 'Details' }).first(), page.getByRole('button', { name: /^planner asked you/ }).first()]) {
  if (await side.count()) break;
  if (await opener.count()) { await opener.click().catch(() => {}); await page.waitForTimeout(1200); }
}
if (!(await side.count())) throw new Error('OBSERVER_MISSING: could not open the direct conversation');
await page.waitForTimeout(800);
const report = await page.evaluate(() => {
  const rect = (el) => el.getBoundingClientRect();
  const canvas = document.querySelector('.canvas-column');
  const right = document.querySelector('.studio-columns > .right-panel');
  const rightShown = right !== null && getComputedStyle(right).display !== 'none' && rect(right).width > 0;
  const names = ['Team (live)', 'Journey', 'Reset layout', 'Graph file', 'Run actions'];
  const controls = [...document.querySelectorAll('.canvas-column button, .canvas-column [role=tab]')]
    .filter((el) => names.includes(el.textContent.trim()) && rect(el).width > 0);
  const overlaps = [];
  if (rightShown) {
    const rr = rect(right);
    for (const c of controls) {
      const cr = rect(c);
      if (cr.right > rr.left + 1 && cr.left < rr.right && cr.bottom > rr.top && cr.top < rr.bottom) overlaps.push(c.textContent.trim());
    }
  }
  return {
    canvasWidth: Math.round(rect(canvas).width),
    rightPanelShown: rightShown,
    railOpen: document.querySelector('.app.projects-open') !== null,
    directConversationOpen: document.querySelector('.studio-columns > .chat-col:not([hidden]), .studio-columns > .node-col:not([hidden])') !== null,
    controlsOverRightPanel: overlaps,
    pageScrollsSideways: document.documentElement.scrollWidth > innerWidth,
  };
});
await page.screenshot({ path: out });
console.log(JSON.stringify(report));
await browser.close();
