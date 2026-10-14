import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { reduceCpuProfile, reduceEnvelopeObservation, summarize } from './stats.mjs';

// Pure arithmetic, no browser or I/O; normally under one second for the suite.
test('odd samples use numeric order and do not change the observations', () => {
  const samples = [100, 2, 10];
  assert.deepEqual(summarize(samples), { median: 10, p90: 100, max: 100 });
  assert.deepEqual(samples, [100, 2, 10]);
});

test('even samples average the middle pair; p90 uses nearest rank', () => {
  assert.deepEqual(summarize([10, 1, 9, 2, 8, 3, 7, 4, 6, 5]),
    { median: 5.5, p90: 9, max: 10 });
  assert.deepEqual(summarize([7]), { median: 7, p90: 7, max: 7 });
});

test('empty and non-finite observations cannot produce a numeric report', () => {
  for (const samples of [[], [NaN], [1, Infinity], [-Infinity, 1], [1, '2'], Array(1)]) {
    assert.throws(() => summarize(samples), { name: 'TypeError' });
  }
});

test('envelope gap uses body completion for the selected execution and excludes later polls', () => {
  const result = reduceEnvelopeObservation({
    executionId: 'gh-team', sourceHead: 'abc12345', navigationStartedAt: 0,
    requests: [
      { executionId: 'gh-team', kind: 'single', responseEnd: 100, status: 200 },
      { executionId: 'other', kind: 'batch', responseEnd: 900, status: 200 },
      { executionId: 'gh-team', kind: 'batch', responseEnd: 200, status: 200 },
      { executionId: 'gh-team', kind: 'single', responseEnd: 700, status: 200, poll: true },
    ],
    checkingStartedAt: 210, settledAt: 350, cutoffAt: 800, eventHeadChanged: false,
  });
  assert.equal(result.status, 'ok');
  assert.equal(result.postEvidenceGapMs, 150);
  assert.equal(result.sourceHead, 'abc12345');
  assert.deepEqual(result.requestCounts, { single: 1, batch: 1, failed: 0 });
});

test('pending requests are scoped to the selected execution', () => {
  const result = reduceEnvelopeObservation({
    executionId: 'gh-team', sourceHead: 'abc12345', navigationStartedAt: 0,
    requests: [{ executionId: 'gh-team', kind: 'single', responseEnd: 100, status: 200 }],
    checkingStartedAt: 110, settledAt: 150, cutoffAt: 200, eventHeadChanged: false,
    pendingRequests: [{ executionId: 'other', responseEnd: null }],
  });
  assert.equal(result.status, 'ok');
  assert.equal(result.postEvidenceGapMs, 50);

  const selectedPending = reduceEnvelopeObservation({
    executionId: 'gh-team', sourceHead: 'abc12345', navigationStartedAt: 0,
    requests: [{ executionId: 'gh-team', kind: 'single', responseEnd: 100, status: 200 }],
    checkingStartedAt: 110, settledAt: 150, cutoffAt: 200, eventHeadChanged: false,
    pendingRequests: [{ executionId: 'gh-team', responseEnd: null }],
  });
  assert.equal(selectedPending.status, 'unknown');
  assert.equal(selectedPending.reason, 'evidence requests remained pending at cutoff');
});

test('ambiguous or incomplete envelope boundaries stay unknown', () => {
  const base = {
    executionId: 'gh-team', sourceHead: 'abc12345', navigationStartedAt: 0,
    requests: [{ executionId: 'gh-team', kind: 'single', responseEnd: 100, status: 200 }],
    checkingStartedAt: 110, settledAt: 150, cutoffAt: 200, eventHeadChanged: false,
  };
  for (const change of [
    { settledAt: null }, { checkingStartedAt: null },
    { requests: [{ executionId: 'gh-team', kind: 'batch', responseEnd: 100, status: 500 }] },
    { requests: [{ executionId: 'gh-team', kind: 'single', responseEnd: null, status: 200 }] },
    { settledAt: 250, cutoffAt: 200 },
    { eventHeadChanged: true }, { eventHeadChanged: null }, { cacheOnly: true }, { timingBufferLost: true },
    { pendingRequests: 1 },
  ]) {
    const result = reduceEnvelopeObservation({ ...base, ...change });
    assert.equal(result.status, 'unknown');
    assert.equal(result.postEvidenceGapMs, null);
    assert.ok(result.reason);
  }
});

test('profile samples are clipped and attributed to leaf self time', () => {
  const profile = {
    startTime: 0, endTime: 100, nodes: [
      { id: 1, callFrame: { functionName: '(root)', url: '', lineNumber: -1 }, children: [2, 3] },
      { id: 2, callFrame: { functionName: 'parent', url: 'https://site.test/a.js?secret=x', lineNumber: 4 } },
      { id: 3, callFrame: { functionName: 'child', url: 'https://site.test/a.js?secret=x', lineNumber: 8 } },
    ], samples: [1, 2, 3, 2, 3], timeDeltas: [10, 20, 30, 20, 20],
  };
  const result = reduceCpuProfile(profile, { startUs: 25, endUs: 85, sourceHead: 'abc12345', alignment: 'same-load monotonic profile interval' });
  assert.equal(result.status, 'ok');
  assert.equal(result.intervalUs, 60);
  assert.deepEqual(result.frames.map(({ functionName, selfTimeUs, samples }) => ({ functionName, selfTimeUs, samples })), [
    { functionName: 'child', selfTimeUs: 35, samples: 2 },
    { functionName: 'parent', selfTimeUs: 25, samples: 2 },
  ]);
  assert.equal(result.unassignedTimeUs, 0);
  assert.ok(result.frames.every(frame => !JSON.stringify(frame).includes('secret')));
});

test('profile reducer refuses missing alignment and malformed or oversized data', () => {
  const valid = { startTime: 0, endTime: 10, nodes: [{ id: 1, callFrame: { functionName: 'x' } }], samples: [1], timeDeltas: [10] };
  assert.equal(reduceCpuProfile(valid, { startUs: 0, endUs: 10, sourceHead: 'abc12345' }).status, 'unknown');
  for (const profile of [
    { ...valid, timeDeltas: [] },
    { ...valid, samples: [99] },
    { ...valid, timeDeltas: [Infinity] },
  ]) assert.equal(reduceCpuProfile(profile, { startUs: 0, endUs: 10, sourceHead: 'abc12345', alignment: 'same-load' }).status, 'unknown');
  assert.equal(reduceCpuProfile(valid, { startUs: 0, endUs: 10, sourceHead: 'abc12345', alignment: 'same-load', limits: { maxSamples: 0 } }).status, 'unknown');
});

test('profile keeps idle, garbage collection, native, and unassigned weights separate', () => {
  const profile = {
    nodes: [
      { id: 1, callFrame: { functionName: '(root)', url: '' }, children: [2, 3, 4, 5, 6] },
      { id: 2, callFrame: { functionName: '(idle)', url: '' } },
      { id: 3, callFrame: { functionName: '(garbage collector)', url: '' } },
      { id: 4, callFrame: { functionName: 'nativeCall', scriptId: '0', url: '' } },
      { id: 5, callFrame: { functionName: '(program)', url: '' } },
      { id: 6, callFrame: { functionName: 'work', url: 'file:///app/work.js', lineNumber: 0 } },
    ],
    samples: [2, 3, 4, 5, 6], timeDeltas: [4, 5, 7, 11, 14],
  };
  const result = reduceCpuProfile(profile, { startUs: 0, endUs: 41, sourceHead: 'abc12345', alignment: 'same-load profile clock' });
  assert.equal(result.idleTimeUs, 4);
  assert.equal(result.garbageCollectionTimeUs, 5);
  assert.equal(result.nativeTimeUs, 7);
  assert.equal(result.unassignedTimeUs, 11);
  assert.equal(result.frames.reduce((sum, frame) => sum + frame.selfTimeUs, 0), 14);
});

test('offline profile command emits JSON without loading Playwright', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'studio-profile-'));
  try {
    const profilePath = join(directory, 'profile.json');
    const outputPath = join(directory, 'report.json');
    await writeFile(profilePath, JSON.stringify({ startTime: 0, endTime: 10,
      nodes: [{ id: 1, callFrame: { functionName: 'work', url: 'file:///private/user/a.js', lineNumber: 0 } }],
      samples: [1], timeDeltas: [10] }));
    const script = fileURLToPath(new URL('./measure.mjs', import.meta.url));
    const result = spawnSync(process.execPath, [script, '--mode', 'profile', '--profile', profilePath,
      '--source-head', 'abc12345', '--start-us', '0', '--end-us', '10', '--alignment', 'same-load mapping bearer sk-test-123 https://site.test/a?token=private', '--out', outputPath],
    { encoding: 'utf8', windowsHide: true });
    assert.equal(result.status, 0, result.stderr);
    const report = JSON.parse(await (await import('node:fs/promises')).readFile(outputPath, 'utf8'));
    assert.equal(report.mode, 'profile');
    assert.equal(report.observation.status, 'ok');
    assert.ok(!JSON.stringify(report).includes('private/user'));
    assert.ok(!JSON.stringify(report).includes('sk-test-123'));
    assert.ok(!JSON.stringify(report).includes('token=private'));
    const unknown = reduceCpuProfile({ nodes: [{ id: 1, callFrame: { functionName: 'work' } }], samples: [1], timeDeltas: [1] }, {
      startUs: 10, endUs: 0, sourceHead: 'abc12345', alignment: 'bearer sk-test-123 https://site.test/a?token=private',
    });
    assert.equal(unknown.status, 'unknown');
    assert.ok(!JSON.stringify(unknown).includes('sk-test-123'));
    assert.ok(!JSON.stringify(unknown).includes('token=private'));
  } finally { await rm(directory, { recursive: true, force: true }); }
});
