import assert from 'node:assert/strict';
import test from 'node:test';
import { summarize } from './stats.mjs';

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
  for (const samples of [[], [NaN], [1, Infinity], [-Infinity, 1], [1, '2']]) {
    assert.throws(() => summarize(samples), { name: 'TypeError' });
  }
});
