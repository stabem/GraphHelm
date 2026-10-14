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

/** Reduce one browser-clock envelope observation without guessing missing boundaries. */
export function reduceEnvelopeObservation(input) {
  const requests = input.requests.filter(request => request.executionId === input.executionId && !request.poll);
  const requestCounts = {
    single: requests.filter(request => request.kind === 'single').length,
    batch: requests.filter(request => request.kind === 'batch').length,
    failed: requests.filter(request => request.status < 200 || request.status >= 300).length,
  };
  let reason = null;
  if (input.eventHeadChanged === true) reason = 'event head changed during observation';
  else if (input.eventHeadChanged !== false) reason = 'event head stability was not observed';
  else if (input.cacheOnly) reason = 'no evidence response was observed';
  else if (input.timingBufferLost) reason = 'resource timing buffer was lost';
  else if (input.checkingStartedAt == null || input.settledAt == null) reason = 'checking beacon boundary was missing';
  else if (input.settledAt > input.cutoffAt) reason = 'beacon settled after observation cutoff';
  else if (input.pendingRequests > 0) reason = 'evidence requests remained pending at cutoff';
  else if (requests.length === 0) reason = 'no initial-load evidence responses were attributed';
  else if (requests.some(request => request.status < 200 || request.status >= 300)) reason = 'an evidence request failed';
  else if (requests.some(request => !Number.isFinite(request.responseEnd))) reason = 'an evidence body completion time was missing';
  if (reason) return {
    status: 'unknown', reason, executionId: input.executionId, sourceHead: input.sourceHead,
    navigationStartedAt: input.navigationStartedAt, checkingStartedAt: input.checkingStartedAt ?? null,
    settledAt: input.settledAt ?? null, cutoffAt: input.cutoffAt, requestCounts, postEvidenceGapMs: null,
  };
  const lastEvidenceBodyAt = Math.max(...requests.map(request => request.responseEnd));
  const gap = input.settledAt - lastEvidenceBodyAt;
  if (gap < 0) return {
    status: 'unknown', reason: 'beacon settled before the last evidence body completed', executionId: input.executionId,
    sourceHead: input.sourceHead, navigationStartedAt: input.navigationStartedAt,
    checkingStartedAt: input.checkingStartedAt, settledAt: input.settledAt, cutoffAt: input.cutoffAt,
    lastEvidenceBodyAt, requestCounts, postEvidenceGapMs: null,
  };
  return {
    status: 'ok', reason: null, executionId: input.executionId, sourceHead: input.sourceHead,
    navigationStartedAt: input.navigationStartedAt, checkingStartedAt: input.checkingStartedAt,
    settledAt: input.settledAt, cutoffAt: input.cutoffAt, lastEvidenceBodyAt,
    requestCounts, postEvidenceGapMs: gap,
  };
}
