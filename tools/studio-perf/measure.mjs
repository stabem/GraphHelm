import { createRequire } from 'node:module';
import { writeFile } from 'node:fs/promises';
import { availableParallelism, cpus, platform, release } from 'node:os';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { reduceEnvelopeObservation, summarize } from './stats.mjs';

const args = process.argv.slice(2);
const options = {};
for (let i = 0; i < args.length; i += 2) {
  if (!['--base', '--runs', '--out', '--mode', '--execution', '--source-head'].includes(args[i]) || !args[i + 1] || options[args[i]]) {
    throw new Error('Usage: node tools/studio-perf/measure.mjs --base <fixture URL> --runs <1..20> --out <file> [--mode clicks|envelopes --execution <id> --source-head <sha>]');
  }
  options[args[i]] = args[i + 1];
}
const runs = Number(options['--runs']);
const mode = options['--mode'] ?? 'clicks';
if (!Number.isInteger(runs) || runs < 1 || runs > 20 || !options['--out'] || !['clicks', 'envelopes'].includes(mode)) {
  throw new Error('--runs must be an integer from 1 to 20; --out is required');
}
if (mode === 'envelopes' && (!options['--execution'] || !options['--source-head'] || runs !== 1)) {
  throw new Error('envelopes mode requires --execution, --source-head, and --runs 1');
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
  if (mode === 'envelopes') {
    const context = await browser.newContext({ viewport });
    try {
      await context.addInitScript(({ executionId, sourceHead }) => {
        const state = window.studioEnvelopePerf = {
          executionId, sourceHead, requests: [], checkingStartedAt: null, settledAt: null,
          eventHead: null, eventHeadChanged: false, eventHeadObserved: false, timingBufferLost: false, startedAt: null,
        };
        const mark = () => performance.timeOrigin + performance.now();
        state.startedAt = mark();
        const observer = new PerformanceObserver(list => {
          for (const entry of list.getEntries()) {
            if (entry.entryType !== 'resource' || !entry.name.includes('/v1/executions/')) continue;
            let url;
            try { url = new URL(entry.name, location.href); } catch { continue; }
            if (url.searchParams.get('execution') !== executionId && !url.pathname.includes(`/executions/${executionId}/`)) continue;
            const pathname = url.pathname;
            const kind = pathname.includes('/evidence/') ? 'single'
              : pathname.endsWith('/evidence') && url.searchParams.has('ids') ? 'batch' : null;
            if (!kind) continue;
            const prior = state.requests.find(request => request.name === entry.name && request.responseEnd === null);
            if (prior) {
              prior.responseEnd = entry.responseEnd > 0 ? performance.timeOrigin + entry.responseEnd : null;
              prior.status ||= entry.responseStatus || (entry.responseEnd > 0 ? 200 : 0);
            } else {
              state.requests.push({ name: entry.name, kind,
                responseEnd: entry.responseEnd > 0 ? performance.timeOrigin + entry.responseEnd : null,
                status: entry.responseStatus || (entry.responseEnd > 0 ? 200 : 0), poll: state.checkingStartedAt !== null });
            }
          }
          state.timingBufferLost ||= performance.getEntriesByType('resource').length >= 250;
        });
        observer.observe({ type: 'resource', buffered: true });
        const originalFetch = window.fetch;
        window.fetch = async (...args) => {
          const input = args[0];
          const raw = typeof input === 'string' ? input : input?.url;
          let tracked;
          if (raw) {
            try {
              const url = new URL(raw, location.href);
              if (url.pathname.includes(`/executions/${executionId}/`) && url.pathname.includes('/evidence')) {
                const kind = url.pathname.endsWith('/evidence') && url.searchParams.has('ids') ? 'batch' : 'single';
                tracked = { name: url.href, kind, responseEnd: null, status: 0, poll: state.checkingStartedAt !== null };
                state.requests.push(tracked);
              }
            } catch { /* ignore non-URL fetch inputs */ }
          }
          const response = await originalFetch(...args);
          if (raw && /\/events(?:\?|$)/.test(raw)) {
            const observed = await response.clone().json().catch(() => null);
            const head = observed?.head;
            if (Number.isSafeInteger(head)) {
              state.eventHeadObserved = true;
              if (state.eventHead !== null && state.eventHead !== head) state.eventHeadChanged = true;
              state.eventHead = head;
            }
          }
          if (tracked) {
            tracked.status = response.status;
            response.clone().arrayBuffer().then(() => { tracked.responseEnd = mark(); }).catch(() => { tracked.status = 0; });
          }
          return response;
        };
        const beacon = () => {
          const candidates = [...document.querySelectorAll('[role="status"], [role="note"], p, span')];
          const checking = candidates.some(element => element.textContent.trim() === 'Checking question envelopes');
          const state = window.studioEnvelopePerf;
          if (checking && state.checkingStartedAt === null) state.checkingStartedAt = mark();
          if (!checking && state.checkingStartedAt !== null && state.settledAt === null) state.settledAt = mark();
        };
        new MutationObserver(beacon).observe(document, { subtree: true, childList: true, characterData: true });
        beacon();
        setTimeout(() => { state.cutoffAt = mark(); state.pendingRequests = state.requests.filter(request => request.responseEnd === null).length; }, 30000);
      }, { executionId: options['--execution'], sourceHead: options['--source-head'] });
      const page = await context.newPage();
      page.setDefaultTimeout(30000);
      const navigationStartedAt = await page.evaluate(url => {
        const started = performance.timeOrigin + performance.now();
        window.name = String(started);
        location.assign(url);
        return started;
      }, base.href);
      await page.waitForURL(url => url.origin === base.origin, { waitUntil: 'commit', timeout: 10000 });
      await page.waitForFunction(() => window.studioEnvelopePerf?.cutoffAt != null, { timeout: 35000 });
      const captured = await page.evaluate(() => ({ ...window.studioEnvelopePerf, navigationStartedAt: Number(window.name) }));
      const { executionId, sourceHead, requests, checkingStartedAt, settledAt, cutoffAt, pendingRequests, eventHeadChanged, eventHeadObserved, timingBufferLost } = captured;
      const result = reduceEnvelopeObservation({ executionId, sourceHead, navigationStartedAt,
        requests: requests.map(({ kind, responseEnd, status, poll }) => ({ executionId, kind, responseEnd, status, poll })),
        checkingStartedAt, settledAt, cutoffAt, pendingRequests, eventHeadChanged: eventHeadObserved ? eventHeadChanged : null, timingBufferLost });
      const report = { mode, base: base.origin, observation: result,
        totalNavigationMs: settledAt === null ? null : settledAt - navigationStartedAt,
        note: 'The post-evidence gap includes browser scheduling and rendering; it is not pure CPU time.' };
      await writeFile(options['--out'], JSON.stringify(report, null, 2) + '\n');
      console.log(result.status === 'ok'
        ? `envelopes: ${result.postEvidenceGapMs.toFixed(1)} ms after evidence bodies; navigation ${report.totalNavigationMs?.toFixed(1) ?? 'unknown'} ms`
        : `envelopes: unknown (${result.reason})`);
    } finally { await context.close(); }
  } else {
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
      let load;
      try {
        // Dispatch navigation in the same page task as the clock read. timeOrigin
        // bridges the old and new documents without including a driver round trip.
        const started = await page.evaluate(url => {
          const started = performance.timeOrigin + performance.now();
          location.assign(url);
          return started;
        }, base.href);
        await page.waitForURL(url => url.origin === base.origin, { waitUntil: 'commit', timeout: 10000 });
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
  }
} finally {
  await browser.close();
}
