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
