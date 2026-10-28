#!/usr/bin/env node
// Studio board reload observer (#1703). Times N serial authenticated reloads of the Studio's
// Lanes board against a Studio and Runtime the caller already started (the studio journey
// fixture), and writes one JSON receipt. It only reads: it never writes to the Runtime.
//
//   node reload.mjs --studio http://127.0.0.1:<port> --runtime http://127.0.0.1:<port>
//     --session <nonce file> --token <token file> --execution demo --condition idle|builds
//     --playwright <dir whose node_modules has @playwright/test> --out <receipt.json>
//     [--attempts 20] [--timeout-ms 30000]
//
// The session nonce and bearer token are read from files and never written to the receipt or
// to stdout/stderr. Every child process runs hidden (windowsHide).
import { execFile } from 'node:child_process';
import { createRequire } from 'node:module';
import { readFileSync, writeFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import path from 'node:path';

export const BOARD_READY = 'section#panel-lanes.lanes';
export const READ_FAILURE = ':text("could not be read")';
const FORBIDDEN_PORTS = new Set(['5183', '8793']);

/** Nearest-rank percentile over numbers; null for an empty list. */
export function percentile(values, p) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.max(1, Math.ceil((p / 100) * sorted.length));
  return sorted[rank - 1];
}

/** Stats over `success` attempts only; every outcome is counted. */
export function summarize(attempts) {
  const counts = { success: 0, timeout: 0, error: 0, excluded: 0 };
  for (const a of attempts) counts[a.outcome] = (counts[a.outcome] ?? 0) + 1;
  const valid = attempts.filter((a) => a.outcome === 'success').map((a) => a.usableMs);
  return {
    attempts: attempts.length,
    counts,
    validSamples: valid.length,
    p50Ms: percentile(valid, 50),
    p95Ms: percentile(valid, 95),
    maxMs: valid.length ? Math.max(...valid) : null,
  };
}

/** Outcome of one attempt from what was observed, in a fixed precedence. */
export function classify({ threw, ready, readFailure, badResponses, excludedReason }) {
  if (excludedReason) return { outcome: 'excluded', reason: excludedReason };
  if (threw) return { outcome: 'error', reason: `navigation: ${threw}` };
  if (badResponses.length) return { outcome: 'error', reason: `runtime answered ${badResponses.map((r) => `${r.status} ${r.route}`).join(', ')}` };
  if (readFailure) return { outcome: 'error', reason: 'the page shows a read failure' };
  if (!ready) return { outcome: 'timeout', reason: 'board not usable before the timeout' };
  return { outcome: 'success', reason: null };
}

/** A Runtime route as the receipt shows it: path only, no query (the session nonce rides there). */
export function routeOf(url) {
  const u = new URL(url);
  return u.pathname;
}

/** Build processes from `name\tcommandline` lines: crate or cargo subcommand names, no paths. */
export function parseBuilds(lines) {
  const builds = [];
  for (const line of lines) {
    const [name, ...rest] = line.split('\t');
    const cmd = rest.join('\t');
    const base = (name || '').toLowerCase().replace(/\.exe$/, '');
    if (base === 'rustc') {
      const m = cmd.match(/--crate-name\s+(\S+)/);
      if (m) builds.push({ tool: 'rustc', what: m[1] });
    } else if (base === 'cargo') {
      const m = cmd.match(/\bcargo(?:\.exe)?"?\s+(?:\+\S+\s+)?([a-z-]+)/i);
      builds.push({ tool: 'cargo', what: m ? m[1] : 'unknown' });
    }
  }
  builds.sort((a, b) => (a.tool + a.what).localeCompare(b.tool + b.what));
  return builds;
}

function snapshotBuilds() {
  return new Promise((resolve) => {
    const done = (err, out) => resolve(err ? { error: 'process list unavailable', builds: [] } : { builds: parseBuilds(out.split(/\r?\n/).filter(Boolean)) });
    if (process.platform === 'win32') {
      const ps = "Get-CimInstance Win32_Process -Filter \"Name='cargo.exe' or Name='rustc.exe'\" | ForEach-Object { $_.Name + \"`t\" + $_.CommandLine }";
      execFile('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', ps], { windowsHide: true, timeout: 20000 }, done);
    } else {
      execFile('ps', ['-eo', 'comm=,args='], { windowsHide: true, timeout: 20000 }, (err, out) =>
        done(err, err ? '' : out.split('\n').map((l) => l.trim().replace(/\s+/, '\t')).join('\n')));
    }
  });
}

async function eventHead(runtime, token, execution) {
  try {
    const r = await fetch(new URL(`/v1/executions/${encodeURIComponent(execution)}/tasks`, runtime), {
      headers: { Authorization: `Bearer ${token}` }, signal: AbortSignal.timeout(10000) });
    if (!r.ok) return { status: r.status, head: null };
    const body = await r.json();
    const seqs = JSON.stringify(body).match(/"(?:lastSequence|sequence|headSequence)":(\d+)/g) ?? [];
    const head = seqs.reduce((m, s) => Math.max(m, Number(s.split(':')[1])), 0);
    return { status: r.status, head };
  } catch (e) { return { status: null, head: null, error: e.name }; }
}

function git(args, cwd) {
  return new Promise((resolve) => execFile('git', args, { cwd, windowsHide: true, timeout: 10000 }, (e, out) => resolve(e ? null : out.trim())));
}

function checkLocal(url, what) {
  const u = new URL(url);
  if (u.protocol !== 'http:' || !['127.0.0.1', 'localhost', '[::1]'].includes(u.hostname)) throw new Error(`${what}: loopback http only`);
  if (FORBIDDEN_PORTS.has(u.port)) throw new Error(`${what}: port ${u.port} belongs to the live Studio/Runtime`);
  return u;
}

export function parseArgs(argv) {
  const o = { attempts: 20, timeoutMs: 30000, execution: 'demo' };
  for (let i = 0; i < argv.length; i += 2) {
    const k = argv[i].replace(/^--/, '').replace(/-([a-z])/g, (_, c) => c.toUpperCase());
    o[k] = argv[i + 1];
  }
  o.attempts = Number(o.attempts); o.timeoutMs = Number(o.timeoutMs);
  for (const k of ['studio', 'runtime', 'session', 'token', 'condition', 'playwright', 'out']) if (!o[k]) throw new Error(`missing --${k}`);
  if (!['idle', 'builds'].includes(o.condition)) throw new Error('--condition is idle or builds');
  if (!(o.attempts > 0 && o.attempts <= 200)) throw new Error('--attempts 1..200');
  checkLocal(o.studio, '--studio'); checkLocal(o.runtime, '--runtime');
  return o;
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  const nonce = readFileSync(o.session, 'utf8').trim();
  const token = readFileSync(o.token, 'utf8').split(/\r?\n/)[0].trim();
  const require = createRequire(path.join(path.resolve(o.playwright), 'package.json'));
  const { chromium } = require('@playwright/test');
  const runtimeOrigin = new URL(o.runtime).origin;
  const studioOrigin = new URL(o.studio).origin;
  const repo = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '../..');

  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, serviceWorkers: 'block' });
  const page = await context.newPage();
  let current = null;
  page.on('requestfinished', async (req) => {
    if (!current) return;
    const u = new URL(req.url());
    if (!(u.origin === runtimeOrigin || (u.origin === studioOrigin && u.pathname.startsWith('/v1/')))) return;
    const rec = current;
    try {
      const res = await req.response(); const sizes = await req.sizes(); const t = req.timing();
      rec.routes.push({ method: req.method(), route: routeOf(req.url()), status: res ? res.status() : null,
        ms: t.responseEnd >= 0 ? Math.round(t.responseEnd) : null, bytes: sizes.responseBodySize });
    } catch { rec.routes.push({ method: req.method(), route: routeOf(req.url()), status: null, ms: null, bytes: null }); }
  });
  page.on('requestfailed', (req) => {
    if (current && new URL(req.url()).pathname.startsWith('/v1/')) current.routes.push({ method: req.method(), route: routeOf(req.url()), status: null, failed: req.failure()?.errorText ?? 'failed' });
  });

  const target = new URL(o.studio); target.searchParams.set('session', nonce); target.searchParams.set('view', 'lanes');
  await page.goto(target.href, { waitUntil: 'load', timeout: o.timeoutMs }).catch(() => {});
  await page.locator(BOARD_READY).waitFor({ state: 'visible', timeout: o.timeoutMs }).catch(() => {});

  const headBefore = await eventHead(o.runtime, token, o.execution);
  const attempts = [];
  for (let n = 1; n <= o.attempts; n += 1) {
    const builds = await snapshotBuilds();
    const rec = { n, routes: [] };
    current = rec;
    let threw = null; let ready = false;
    const t0 = performance.now();
    try { await page.reload({ waitUntil: 'commit', timeout: o.timeoutMs }); } catch (e) { threw = e.name; }
    if (!threw) {
      const left = Math.max(1, o.timeoutMs - (performance.now() - t0));
      ready = await page.locator(BOARD_READY).waitFor({ state: 'visible', timeout: left }).then(() => true, () => false);
    }
    const usableMs = Math.round(performance.now() - t0);
    await page.waitForLoadState('load', { timeout: 5000 }).catch(() => {});
    await new Promise((r) => setTimeout(r, 250)); // let requestfinished handlers for the board's reads land
    current = null;
    const readFailure = await page.locator(READ_FAILURE).count().catch(() => 0) > 0;
    const nav = await page.evaluate(() => { const e = performance.getEntriesByType('navigation')[0]; return e ? { domContentLoadedMs: Math.round(e.domContentLoadedEventEnd), loadMs: Math.round(e.loadEventEnd), documentBytes: e.encodedBodySize } : null; }).catch(() => null);
    const bad = rec.routes.filter((r) => r.status === null || r.status >= 400);
    const { outcome, reason } = classify({ threw, ready, readFailure, badResponses: bad, excludedReason: null });
    attempts.push({ n, outcome, reason, usableMs: outcome === 'success' ? usableMs : null, elapsedMs: usableMs, navigation: nav, routes: rec.routes, builds: builds.builds, buildsError: builds.error ?? null });
    process.stderr.write(`attempt ${n}: ${outcome} ${usableMs} ms, builds ${builds.builds.length}\n`);
  }
  const headAfter = await eventHead(o.runtime, token, o.execution);
  await browser.close();

  const buildCounts = attempts.map((a) => a.builds.filter((b) => b.tool === 'cargo').length);
  const receipt = {
    schema: 'graphhelm-studio-reload-receipt/1',
    issue: 1703,
    condition: o.condition,
    source: { sha: await git(['rev-parse', 'HEAD'], repo), dirty: (await git(['status', '--porcelain'], repo)) ? true : false },
    machine: { platform: process.platform, arch: process.arch, node: process.version, browser: `chromium ${browser.version()}`, headless: true },
    subject: { studio: 'loopback, lane-started', runtime: 'loopback, lane-started', execution: o.execution, boardReady: BOARD_READY, timeoutMs: o.timeoutMs },
    eventHead: { before: headBefore.head, after: headAfter.head, route: '/v1/executions/{id}/tasks (max sequence)' },
    builds: { cargoMin: Math.min(...buildCounts), cargoMax: Math.max(...buildCounts), fourActiveThroughout: Math.min(...buildCounts) >= 4 },
    summary: summarize(attempts),
    attempts,
  };
  const text = JSON.stringify(receipt, null, 2);
  if (text.includes(nonce) || (token && text.includes(token))) throw new Error('receipt would carry a secret; not written');
  writeFileSync(o.out, `${text}\n`);
  process.stdout.write(`${JSON.stringify({ condition: o.condition, ...receipt.summary, builds: receipt.builds })}\n`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main().catch((e) => { process.stderr.write(`reload: ${e.message}\n`); process.exit(2); });
}
