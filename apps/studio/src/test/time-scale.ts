/**
 * One knob for the Studio tests' waits (#549), the same `GRAPHHELM_TEST_TIME_SCALE` the CLI tests
 * read (`apps/cli/tests/support/time_scale.rs`).
 *
 * **What went wrong without it.** `App.test.tsx` failed on a busy machine with nothing wrong in
 * the Studio: five cells timed out on `main` while another lane's battery ran, and 42 of #596's
 * first 44 failures were the 5 s default test timeout. On this host, idle, several App cells
 * already take 3-5 s, so the default left no headroom at all.
 *
 * **What this scales.** Hang catchers only: the test timeout, Testing Library's wait for a UI
 * condition (`findBy*`, `waitFor`), and the explicit ceilings of waits that poll for a condition.
 * Each still waits for its condition and returns the moment it holds; the knob only moves the point
 * at which "it never happened" is declared.
 *
 * **What this must never scale.** A product budget, or an assertion that something took LESS than
 * a bound. The Studio never reads this variable outside its tests.
 *
 * Unset is 1. A value that does not parse throws instead of meaning 1: a lane that mistyped the
 * knob would otherwise read the same false reds and blame the machine.
 */
export const TIME_SCALE_ENV = "GRAPHHELM_TEST_TIME_SCALE";
const MAX_FACTOR = 20;

/** The factor a raw value of the variable means. Pure, so the refusal is testable. */
export function timeScaleFactor(raw: string | undefined): number {
  const value = (raw ?? "").trim();
  if (value === "") return 1;
  const factor = /^\d+$/.test(value) ? Number(value) : Number.NaN;
  if (!Number.isInteger(factor) || factor < 1 || factor > MAX_FACTOR) {
    throw new Error(`${TIME_SCALE_ENV}=${JSON.stringify(raw)} is not a whole number from 1 to ${MAX_FACTOR}`);
  }
  return factor;
}

function environment(): string | undefined {
  const process = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process;
  return process?.env?.[TIME_SCALE_ENV];
}

/** `baseMs` times this run's factor. */
export function scaled(baseMs: number): number {
  return baseMs * timeScaleFactor(environment());
}

/** The base test timeout: a hang catcher, set above the slowest App cell measured on an idle host
 * (about 5 s), then scaled. */
export const TEST_TIMEOUT_BASE_MS = 10_000;
/** Testing Library's default wait for a UI condition (its own default), then scaled. */
export const UI_WAIT_BASE_MS = 1_000;
