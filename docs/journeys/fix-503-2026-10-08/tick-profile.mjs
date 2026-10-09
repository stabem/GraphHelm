// #503 observer: what one quiet poll tick costs with the gh-team run open on the Team tab.
// Headless Chromium; after the task graph has drawn, it watches a fixed window and reports
// long tasks (main-thread blocks >50 ms), DOM mutations, Runtime requests and DOM size.
// Usage: node tick-profile.mjs <studio url with ?session=...> <seconds> <out.json>
import { chromium } from '@playwright/test';
import { writeFileSync } from 'node:fs';

const [url, secondsArg, out] = process.argv.slice(2);
const seconds = Number(secondsArg || 60);
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const requests = [];
page.on('request', (request) => { const u = new URL(request.url()); if (u.pathname.startsWith('/v1/')) requests.push({ t: Date.now(), raw: u.pathname + u.search, path: u.pathname.replace(/\/evidence\/[^/]+$/, '/evidence/:id') }); });
await page.goto(url);
await page.getByRole('tab', { name: /^Team/ }).first().waitFor({ timeout: 60000 });
// The task graph is drawn once an issue link shows (#496 waits for the same thing).
await page.getByText(/^Issue #\d+/).first().waitFor({ timeout: 600000 });
const settle = async () => {
  // Quiet = no envelope read for 10 s (the first load reads every sealed record once).
  const deadline = Date.now() + 20 * 60 * 1000;
  for (;;) {
    const last = Math.max(0, ...requests.filter((r) => r.path.endsWith('/evidence/:id')).map((r) => r.t));
    if (Date.now() - last > 10000) return;
    if (Date.now() > deadline) throw new Error('evidence reads never settled');
    await page.waitForTimeout(2000);
  }
};
const measure = async (label) => {
  await settle();
  await page.evaluate(() => {
    window.__prof = { longTasks: [], mutations: 0 };
    if (!window.__observed) {
      window.__observed = true;
      new PerformanceObserver((list) => { for (const e of list.getEntries()) window.__prof.longTasks.push(Math.round(e.duration)); }).observe({ type: 'longtask', buffered: false });
      new MutationObserver((records) => { window.__prof.mutations += records.length; }).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    }
  });
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Performance.enable');
  const metric = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
  const m0 = await metric();
  const start = Date.now();
  await page.waitForTimeout(seconds * 1000);
  const m1 = await metric();
  const prof = await page.evaluate(() => ({ ...window.__prof, domNodes: document.getElementsByTagName('*').length }));
  const inWindow = requests.filter((r) => r.t >= start);
  const byPath = {};
  for (const r of inWindow) byPath[r.path] = (byPath[r.path] ?? 0) + 1;
  return { label, scriptMs: Math.round((m1.ScriptDuration - m0.ScriptDuration) * 1000), taskMs: Math.round((m1.TaskDuration - m0.TaskDuration) * 1000), layoutMs: Math.round((m1.LayoutDuration - m0.LayoutDuration) * 1000), seconds, ticks: Math.round(seconds / 4), longTasks: prof.longTasks.length, longTaskMsTotal: prof.longTasks.reduce((a, b) => a + b, 0), domMutations: prof.mutations, domNodes: prof.domNodes, runtimeRequests: inWindow.length, requestsByPath: byPath, newEventsRead: inWindow.filter((r) => r.path.endsWith('/events')).map((r) => r.raw.split('?')[1]).filter((q, k, all) => all.indexOf(q) === k) };
};
const results = [await measure(process.env.FIRST_LABEL || 'first')];
const swap = process.env.SWAP_FILE;
if (swap) {
  const { existsSync } = await import('node:fs');
  console.error('waiting for ' + swap);
  while (!existsSync(swap)) await page.waitForTimeout(2000);
  results.push(await measure(process.env.SECOND_LABEL || 'second'));
}
const report = results;
writeFileSync(out, JSON.stringify(report, null, 2));
console.log(JSON.stringify(report));
await browser.close();
