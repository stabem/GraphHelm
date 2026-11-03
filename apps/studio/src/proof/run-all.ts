/** #1909 (review of #1908): how long Run all waits on one journey before it stops and says so. */
export const RUN_ALL_TIMEOUT_MS = 10 * 60_000;
export const RUN_ALL_POLL_MS = 1_000;
export const RUN_ALL_RETRY_MS = 2_000;
export const RUN_ALL_MAX_READ_FAILURES = 30;
export const RUN_ALL_RUN_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export interface Clock { now: () => number; sleep: (ms: number) => Promise<void> }

/** A continuation belongs only to the exact Proof view that started it. */
export function runAllViewIsCurrent(startedGeneration: number, currentGeneration: number): boolean {
  return startedGeneration === currentGeneration;
}

const realClock: Clock = { now: () => Date.now(), sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)) };

export type RunAllPhase = "not_started" | "start_pending" | "running" | "terminal" | "unknown";
export type RunAllReason = "read_failed" | "read_deadline" | "run_identity_unknown" | "start_rejected" | "post_outcome_unknown" | "reloaded" | "view_left" | "storage_failed";
const RUN_ALL_REASONS: readonly RunAllReason[] = ["read_failed", "read_deadline", "run_identity_unknown", "start_rejected", "post_outcome_unknown", "reloaded", "view_left", "storage_failed"];

export interface RunAllJourneyState {
  flowId: string;
  phase: RunAllPhase;
  runId?: string;
  deadlineAt?: number;
  reason?: RunAllReason;
  updatedAt: number;
}

export interface RunAllBatch {
  version: 1;
  projectKey: string;
  batchId: string;
  updatedAt: number;
  reason?: RunAllReason;
  journeys: RunAllJourneyState[];
}

export interface RunAllStorage {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
  removeItem: (key: string) => void;
}

export function runAllStorageKey(projectKey: string): string {
  return `graphhelm:studio:run-all:v1:${encodeURIComponent(projectKey)}`;
}

export function makeRunAllBatch(projectKey: string, flowIds: readonly string[], batchId: string, updatedAt = Date.now()): RunAllBatch {
  return {
    version: 1,
    projectKey,
    batchId,
    updatedAt,
    journeys: flowIds.map((flowId) => ({ flowId, phase: "not_started", updatedAt })),
  };
}

export function updateRunAllJourney(
  batch: RunAllBatch,
  index: number,
  phase: RunAllPhase,
  { runId, deadlineAt, reason, updatedAt = Date.now() }: { runId?: string; deadlineAt?: number; reason?: RunAllReason; updatedAt?: number } = {},
): RunAllBatch {
  if (!Number.isInteger(index) || index < 0 || index >= batch.journeys.length) throw new RangeError("Run all journey index is outside the batch.");
  const journeys = batch.journeys.map((journey, current) => {
    if (current !== index) return journey;
    const next: RunAllJourneyState = { flowId: journey.flowId, phase, updatedAt };
    if (runId !== undefined) next.runId = runId;
    const savedDeadline = deadlineAt ?? (phase === "terminal" ? undefined : journey.deadlineAt);
    if (savedDeadline !== undefined) next.deadlineAt = savedDeadline;
    if (reason !== undefined) next.reason = reason;
    return next;
  });
  return { ...batch, updatedAt, journeys };
}

export function reconcileRunAllJourney(
  batch: RunAllBatch,
  index: number,
  run: { state: string; runId?: string } | null,
  { readFailed = false, reason: readReason, updatedAt = Date.now() }: { readFailed?: boolean; reason?: RunAllReason; updatedAt?: number } = {},
): RunAllBatch {
  const matches = run !== null && isRunAllRunId(run.runId) && run.runId === batch.journeys[index]?.runId &&
    ["running", "ready", "failed"].includes(run.state);
  const reason: RunAllReason = matches ? "reloaded" : readReason ?? (readFailed ? "read_failed" : "run_identity_unknown");
  const phase: RunAllPhase = !matches ? "unknown" : run!.state === "running" ? "running" : "terminal";
  const updated = updateRunAllJourney(batch, index, phase, {
    ...(isRunAllRunId(batch.journeys[index]?.runId) ? { runId: batch.journeys[index].runId } : {}), reason, updatedAt,
  });
  return {
    ...updated,
    reason,
    journeys: updated.journeys.map((journey) => journey.phase === "not_started"
      ? { ...journey, reason, updatedAt }
      : journey),
  };
}

export function readRunAllBatch(storage: RunAllStorage, projectKey: string): RunAllBatch | null {
  const raw = storage.getItem(runAllStorageKey(projectKey));
  if (raw === null) return null;
  const value: unknown = JSON.parse(raw);
  if (typeof value !== "object" || value === null) throw new Error("Saved Run all status is not an object.");
  const batch = value as Partial<RunAllBatch>;
  if (batch.version !== 1 || batch.projectKey !== projectKey || !isRunAllRunId(batch.batchId) ||
      typeof batch.updatedAt !== "number" || (batch.reason !== undefined && !RUN_ALL_REASONS.includes(batch.reason)) ||
      !Array.isArray(batch.journeys) || batch.journeys.some((journey) =>
        typeof journey?.flowId !== "string" || !["not_started", "start_pending", "running", "terminal", "unknown"].includes(journey.phase) ||
        typeof journey.updatedAt !== "number" || (journey.deadlineAt !== undefined && (!Number.isFinite(journey.deadlineAt) || journey.deadlineAt <= 0)) ||
        (journey.runId !== undefined && !isRunAllRunId(journey.runId)) ||
        (journey.reason !== undefined && !RUN_ALL_REASONS.includes(journey.reason)) ||
        (journey.phase === "running" && !isRunAllRunId(journey.runId)))) {
    throw new Error("Saved Run all status is incomplete or from a different format.");
  }
  return batch as RunAllBatch;
}

/** Web Storage setItem is one atomic operation; on failure its prior value remains in place. */
export function writeRunAllBatch(storage: RunAllStorage, batch: RunAllBatch): void {
  storage.setItem(runAllStorageKey(batch.projectKey), JSON.stringify(batch));
}

export function clearRunAllBatch(storage: RunAllStorage, projectKey: string): void {
  storage.removeItem(runAllStorageKey(projectKey));
}

export function isRunAllRunId(value: unknown): value is string {
  return typeof value === "string" && RUN_ALL_RUN_ID.test(value);
}

export class RunAllReadFailure extends Error {
  readonly reason: RunAllReason;
  readonly failures: number;

  constructor(reason: RunAllReason, failures: number) {
    super(reason === "read_deadline"
      ? "The active run could not be confirmed before Run all's wait limit."
      : "The Runtime could not confirm the active run after repeated reads.");
    this.name = "RunAllReadFailure";
    this.reason = reason;
    this.failures = failures;
  }
}

export class RunAllIdentityFailure extends Error {
  readonly reason = "run_identity_unknown" as const;

  constructor() {
    super("The Runtime did not return the same run ID, so Run all stopped without starting another journey.");
    this.name = "RunAllIdentityFailure";
  }
}

/** Reads one identified run until its known state is terminal. Only GET failures are retried. */
export async function runAllUntil<T extends { state: string; runId?: string }>(
  expectedRunId: string,
  read: (timeoutMs: number) => Promise<T>,
  { timeoutMs = RUN_ALL_TIMEOUT_MS, pollMs = RUN_ALL_POLL_MS, retryMs = RUN_ALL_RETRY_MS,
    maxFailures = RUN_ALL_MAX_READ_FAILURES, clock = realClock, live = () => true }:
    { timeoutMs?: number; pollMs?: number; retryMs?: number; maxFailures?: number; clock?: Clock; live?: () => boolean } = {},
): Promise<{ state: "done"; run: T } | { state: "timeout" } | { state: "cancelled" }> {
  const deadline = clock.now() + timeoutMs;
  let failures = 0;
  for (;;) {
    if (!live()) return { state: "cancelled" };
    const remaining = deadline - clock.now();
    if (remaining <= 0) return { state: "timeout" };
    let run: T;
    try {
      run = await read(Math.max(1, Math.min(30_000, remaining)));
    } catch {
      if (!live()) return { state: "cancelled" };
      failures += 1;
      if (failures >= maxFailures) throw new RunAllReadFailure("read_failed", failures);
      const wait = Math.min(retryMs, Math.max(0, deadline - clock.now()));
      if (wait === 0) return { state: "timeout" };
      await clock.sleep(wait);
      continue;
    }
    if (!live()) return { state: "cancelled" };
    if (run.runId !== expectedRunId || !["running", "ready", "failed"].includes(run.state)) {
      throw new RunAllIdentityFailure();
    }
    failures = 0;
    if (run.state !== "running") return { state: "done", run };
    const wait = Math.min(pollMs, Math.max(0, deadline - clock.now()));
    if (wait === 0) return { state: "timeout" };
    await clock.sleep(wait);
  }
}

/** #1912: how long a failed read of a running journey waits before it is tried again, and how
 * many failures in a row end the follow (the Studio then says it lost the run). */
export const FOLLOW_RETRY_MS = 2_000;
export const FOLLOW_MAX_FAILURES = 30;

/** #1912: follows one run from the answer that started it to its recorded result, handing each
 * answer to `onRun`. A read that fails while the run is still running does not end the follow. */
export async function followRun<T extends { state: string }>(
  first: T,
  read: () => Promise<T>,
  { onRun, onError, live = () => true, pollMs = RUN_ALL_POLL_MS, retryMs = FOLLOW_RETRY_MS, maxFailures = FOLLOW_MAX_FAILURES, clock = realClock }: {
    onRun: (run: T) => void;
    onError: (cause: unknown, failures: number, final: boolean) => void;
    live?: () => boolean;
    pollMs?: number;
    retryMs?: number;
    maxFailures?: number;
    clock?: Clock;
  },
): Promise<T | null> {
  let run = first;
  onRun(run);
  let failures = 0;
  while (run.state === "running") {
    await clock.sleep(failures === 0 ? pollMs : retryMs);
    if (!live()) return null;
    try {
      run = await read();
    } catch (cause) {
      if (!live()) return null;
      failures += 1;
      onError(cause, failures, failures >= maxFailures);
      if (failures >= maxFailures) return null;
      continue;
    }
    if (!live()) return null;
    failures = 0;
    onRun(run);
  }
  return run;
}
