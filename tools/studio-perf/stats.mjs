/** Median, nearest-rank p90 and maximum; observations are never mutated. */
export function summarize(samples) {
  if (!Array.isArray(samples) || samples.length === 0 || ![...samples].every(Number.isFinite)) {
    throw new TypeError('Expected a nonempty array of finite numbers');
  }
  const sorted = [...samples].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return {
    median: sorted.length % 2 ? sorted[middle] : sorted[middle - 1] / 2 + sorted[middle] / 2,
    p90: sorted[Math.ceil(sorted.length * 0.9) - 1],
    max: sorted.at(-1),
  };
}
