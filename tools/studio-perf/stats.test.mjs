import assert from 'node:assert/strict';
import test from 'node:test';
import { reduceEnvelopeObservation, summarize } from './stats.mjs';

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
