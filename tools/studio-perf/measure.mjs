import { createRequire } from 'node:module';
import { writeFile } from 'node:fs/promises';
import { availableParallelism, cpus, platform, release } from 'node:os';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { summarize } from './stats.mjs';

const args = process.argv.slice(2);
const options = {};
for (let i = 0; i < args.length; i += 2) {
  if (!['--base', '--runs', '--out'].includes(args[i]) || !args[i + 1] || options[args[i]]) {
    throw new Error('Usage: node tools/studio-perf/measure.mjs --base <fixture URL> --runs <1..20> --out <file>');
  }
  options[args[i]] = args[i + 1];
}
const runs = Number(options['--runs']);
if (!Number.isInteger(runs) || runs < 1 || runs > 20 || !options['--out']) {
  throw new Error('--runs must be an integer from 1 to 20; --out is required');
}
const base = new URL(options['--base']);
if (base.protocol !== 'http:' || !['localhost', '127.0.0.1'].includes(base.hostname)
    || !base.port || ['5183', '5196', '8793'].includes(base.port) || base.username || base.password) {
  throw new Error('--base must be an isolated local fixture port (never 5183, 5196 or 8793)');
}
base.searchParams.set('session', 'studio-fixture');
const project = process.env.GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT
  ?? fileURLToPath(new URL('../../apps/studio/', import.meta.url));
const { chromium } = createRequire(resolve(project, 'package.json'))('@playwright/test');
const viewport = { width: 1440, height: 900 };
const browser = await chromium.launch({ headless: true });
const observations = [];
try {
  for (let run = 1; run <= runs; run++) {
    // A fresh context per run resets session/UI state; one browser, no parallel work.
    const context = await browser.newContext({ viewport });
    try {
      await context.addInitScript(() => {
        window.studioPerfVisible = (selector, started, input, text) => new Promise((resolve) => {
          let finished = false;
          let frame;
          const finish = (result) => {
            if (finished) return;
            finished = true;
            clearTimeout(timer);
            cancelAnimationFrame(frame);
            resolve(result);
          };
          // Cooperative ceiling: a blocked page can delay this timer; late frames
          // are still rejected below rather than reported as numeric observations.
          const timer = setTimeout(() => finish({ status: 'timeout' }), 10000);
          const check = () => {
            if (performance.now() - started >= 10000) return finish({ status: 'timeout' });
            const visible = () => [...document.querySelectorAll(selector)].some(element =>
              (!text || element.textContent.trim() === text)
              && element.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true }));
            const wasVisible = visible();
            frame = requestAnimationFrame(() => {
              if (wasVisible && visible()) {
                const ms = performance.now() - started;
                finish(ms < 10000 ? { status: 'ok', ms, ended: performance.timeOrigin + performance.now() }
                  : { status: 'timeout' });
              } else check();
            });
          };
          if (input) input.click();
          check();
        });
        window.studioPerfLoad = window.studioPerfVisible('summary[role="button"]', 0, null, 'Run actions');
      });
      const page = await context.newPage();
      page.setDefaultTimeout(10000);
      // Both endpoints use the browser performance clock. Include navigation dispatch,
      // not just DOMContentLoaded; timeOrigin bridges the old and new document clocks.
      const started = await page.evaluate(() => performance.timeOrigin + performance.now());
      let load;
      try {
        await page.goto(base.href, { waitUntil: 'commit', timeout: 10000 });
        const result = await page.evaluate(() => window.studioPerfLoad);
        const ms = result.ended - started;
        load = result.status === 'ok' && ms < 10000 ? { status: 'ok', ms } : { status: 'timeout' };
      } catch (error) {
        if (error.name !== 'TimeoutError') throw error;
        load = { status: 'timeout' };
      }
      const sample = { run, load, node: { status: 'skipped' }, chat: { status: 'skipped' } };
      if (load.status === 'ok') {
        await page.getByRole('button', { name: 'Run actions', exact: true }).waitFor();
        const step = page.getByRole('button', { name: 'Deploy · ready', exact: true });
        await step.waitFor();
        sample.node = await step.evaluate(element =>
          window.studioPerfVisible('section[aria-label="Node deploy"]', performance.now(), element));
        if (sample.node.status === 'ok') {
          await page.getByRole('button', { name: 'Close this node', exact: true }).click();
          const chat = page.getByRole('button', { name: 'Chat', exact: true });
          await chat.waitFor();
          sample.chat = await chat.evaluate(element =>
            window.studioPerfVisible('textarea#chat-message', performance.now(), element));
        }
      }
      for (const result of Object.values(sample)) {
        if (typeof result === 'object') delete result.ended;
      }
      observations.push(sample);
    } finally {
      await context.close();
    }
  }
  const interactions = Object.fromEntries(['load', 'node', 'chat'].map(name => {
    const samples = observations.map(run => run[name]);
    const values = samples.filter(sample => sample.status === 'ok').map(sample => sample.ms);
    return [name, {
      completed: values.length,
      timeouts: samples.filter(sample => sample.status === 'timeout').length,
      skipped: samples.filter(sample => sample.status === 'skipped').length,
      // Partial samples must not masquerade as statistics over N successful runs.
      milliseconds: values.length === runs ? summarize(values) : null,
    }];
  }));
  const report = {
    runs, base: base.origin, environment: {
      cpuCount: cpus().length, availableParallelism: availableParallelism(),
      node: process.version, chromium: browser.version(), viewport, platform: platform(), release: release(),
    }, interactions, observations,
  };
  await writeFile(options['--out'], JSON.stringify(report, null, 2) + '\n');
  console.log(Object.entries(interactions).map(([name, result]) => result.milliseconds
    ? `${name}: median/p90/max ${Object.values(result.milliseconds).map(ms => ms.toFixed(1)).join('/')} ms`
    : `${name}: ${result.timeouts} timeout, ${result.skipped} skipped`).join('; '));
} finally {
  await browser.close();
}
