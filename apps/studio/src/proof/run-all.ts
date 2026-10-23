/** #1909 (review of #1908): how long Run all waits on one journey before it stops and says so. */
export const RUN_ALL_TIMEOUT_MS = 10 * 60_000;
export const RUN_ALL_POLL_MS = 1_000;

export interface Clock { now: () => number; sleep: (ms: number) => Promise<void> }

const realClock: Clock = { now: () => Date.now(), sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)) };

/** Reads one journey's run until it is no longer running. Resolves `"done"`, or `"timeout"` once
 * `timeoutMs` has passed with the run still running: Run all then stops instead of waiting forever. */
export async function runAllUntil(
  read: () => Promise<{ state: string }>,
  { timeoutMs = RUN_ALL_TIMEOUT_MS, pollMs = RUN_ALL_POLL_MS, clock = realClock }: { timeoutMs?: number; pollMs?: number; clock?: Clock } = {},
): Promise<"done" | "timeout"> {
  const deadline = clock.now() + timeoutMs;
  for (;;) {
    const run = await read();
    if (run.state !== "running") return "done";
    if (clock.now() >= deadline) return "timeout";
    await clock.sleep(pollMs);
  }
}

/** #1912: how long a failed read of a running journey waits before it is tried again, and how
 * many failures in a row end the follow (the Studio then says it lost the run). */
export const FOLLOW_RETRY_MS = 2_000;
export const FOLLOW_MAX_FAILURES = 30;

/** #1912: follows one run from the answer that started it to its recorded result, handing each
 * answer to `onRun`. A read that fails while the run is still running does not end the follow (one
 * dropped answer left Proof on "Running" for good while the Runtime recorded a pass): `onError`
 * says so and the run is read again, until `maxFailures` reads in a row have failed. Once `live()`
 * is false (the panel moved on) nothing more is read or handed on. */
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
