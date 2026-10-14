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

const PROFILE_LIMITS = Object.freeze({ maxBytes: 20 * 1024 * 1024, maxNodes: 100_000, maxSamples: 1_000_000, maxDepth: 256 });

/** Attribute clipped Chromium CPU-profile sample weights without double-counting stacks. */
export function reduceCpuProfile(profile, options = {}) {
  const limits = { ...PROFILE_LIMITS, ...(options.limits ?? {}) };
  const alignment = typeof options.alignment === 'string' && options.alignment.trim()
    ? 'explicit same-load profile-clock mapping supplied' : null;
  const unknown = reason => ({ status: 'unknown', reason, sourceHead: options.sourceHead ?? null,
    alignment, frames: [], idleTimeUs: 0, garbageCollectionTimeUs: 0, nativeTimeUs: 0, unassignedTimeUs: 0, intervalUs: null });
  if (!options.alignment || typeof options.alignment !== 'string' || !options.alignment.trim()) return unknown('profile interval alignment was not supplied');
  if (!/^[a-f0-9]{8,64}$/i.test(options.sourceHead ?? '') || !Number.isSafeInteger(options.startUs)
      || !Number.isSafeInteger(options.endUs) || options.startUs < 0 || options.endUs <= options.startUs) return unknown('profile interval or source head is invalid');
  if (!profile || !Array.isArray(profile.nodes) || !Array.isArray(profile.samples) || !Array.isArray(profile.timeDeltas)
      || profile.nodes.length === 0 || profile.nodes.length > limits.maxNodes || profile.samples.length === 0
      || profile.samples.length > limits.maxSamples || profile.samples.length !== profile.timeDeltas.length) return unknown('profile arrays are missing, malformed, or exceed limits');
  const byId = new Map();
  if (Buffer.byteLength(JSON.stringify(profile), 'utf8') > limits.maxBytes) return unknown('profile exceeds byte limit');
  for (const node of profile.nodes) {
    if (!node || !Number.isSafeInteger(node.id) || byId.has(node.id) || !node.callFrame || typeof node.callFrame !== 'object') return unknown('profile node is malformed');
    byId.set(node.id, node);
  }
  const parents = new Map();
  for (const node of profile.nodes) {
    if (node.children !== undefined && !Array.isArray(node.children)) return unknown('profile children are malformed');
    for (const child of node.children ?? []) {
      if (!byId.has(child) || parents.has(child)) return unknown('profile node reference is invalid');
      parents.set(child, node.id);
    }
  }
  const totals = new Map();
  let cursor = 0;
  let idleTimeUs = 0;
  let garbageCollectionTimeUs = 0;
  let nativeTimeUs = 0;
  let unassignedTimeUs = 0;
  for (let index = 0; index < profile.samples.length; index++) {
    const weight = profile.timeDeltas[index];
    const id = profile.samples[index];
    if (!Number.isSafeInteger(weight) || weight < 0 || !byId.has(id)) return unknown('profile sample or delta is invalid');
    const next = cursor + weight;
    if (!Number.isSafeInteger(next)) return unknown('profile sample times are invalid');
    const clipped = Math.max(0, Math.min(next, options.endUs) - Math.max(cursor, options.startUs));
    cursor = next;
    if (!clipped) continue;
    let sampledNode = byId.get(id);
    let leaf = sampledNode;
    let depth = 0;
    const visited = new Set();
    while (parents.has(leaf.id) && parents.get(leaf.id) !== profile.nodes[0].id) {
      if (visited.has(leaf.id) || ++depth > limits.maxDepth) return unknown('profile stack is cyclic or too deep');
      visited.add(leaf.id);
      leaf = byId.get(parents.get(leaf.id));
    }
    const frame = sampledNode.callFrame;
    const name = String(frame.functionName ?? '').trim();
    if (!name || name === '(idle)') { idleTimeUs += clipped; continue; }
    if (name === '(garbage collector)') { garbageCollectionTimeUs += clipped; continue; }
    if (frame.scriptId === '0') { nativeTimeUs += clipped; continue; }
    if (name === '(program)' || name === '(root)') { unassignedTimeUs += clipped; continue; }
    const url = sanitizeProfileUrl(frame.url);
    const key = `${name}\0${url}\0${Number.isSafeInteger(frame.lineNumber) ? frame.lineNumber + 1 : null}`;
    const prior = totals.get(key) ?? { functionName: sanitizeText(name), url, line: Number.isSafeInteger(frame.lineNumber) && frame.lineNumber >= 0 ? frame.lineNumber + 1 : null, selfTimeUs: 0, samples: 0 };
    prior.selfTimeUs += clipped;
    prior.samples += 1;
    totals.set(key, prior);
  }
  if (cursor < options.endUs) unassignedTimeUs += options.endUs - Math.max(cursor, options.startUs);
  const frames = [...totals.values()].sort((a, b) => b.selfTimeUs - a.selfTimeUs || a.functionName.localeCompare(b.functionName));
  return { status: 'ok', reason: null, sourceHead: options.sourceHead, alignment,
    startUs: options.startUs, endUs: options.endUs, intervalUs: options.endUs - options.startUs,
    frames, idleTimeUs, garbageCollectionTimeUs, nativeTimeUs, unassignedTimeUs, sampleCount: profile.samples.length,
    note: 'Sample weights estimate self time; they are not exact durations or causal proof.' };
}

function sanitizeText(value) { return String(value).replace(/(?:[A-Za-z]:)?[\\/](?:[^\\/\s]+[\\/])+[^\\/\s]*/g, '[path]').slice(0, 160); }
function sanitizeProfileUrl(value) {
  if (typeof value !== 'string' || !value) return null;
  try {
    const url = new URL(value);
    const safePath = url.pathname.split('/').filter(Boolean).slice(-2).join('/');
    return `${url.protocol === 'file:' ? 'file:' : url.origin}/${safePath}`.slice(0, 240);
  } catch { return value.split(/[?#]/, 1)[0].replace(/^.*[\\/]/, '[local]/').slice(0, 240); }
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
  else if ((Array.isArray(input.pendingRequests)
    ? input.pendingRequests.some(request => request.executionId === input.executionId)
    : Number(input.pendingRequests) > 0)) reason = 'evidence requests remained pending at cutoff';
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
