import userEventBase from "@testing-library/user-event";

/**
 * A `user-event` instance with the inter-event delay removed.
 *
 * WHY. `user-event` 14 splits one `click` into a sequence — pointerover, pointerenter, pointermove,
 * pointerdown, mousedown, focus, pointerup, mouseup, click — and its default `delay: 0` still yields
 * to the event loop through `setTimeout` BETWEEN each of them. `src/App.test.tsx` alone makes 303
 * such calls, and its mount helper spends two clicks before a test asserts anything.
 *
 * MEASURED on this host, `npx vitest run src/App.test.tsx`, 143 tests passing every time:
 *
 *     default delay   103.6 s, 98.3 s
 *     delay: null      82.8 s, 72.7 s
 *
 * That file is 86% of the studio suite's test time, and the suite is the gate's critical-path tail:
 * `apps/studio (npm)` starts only after the PowerShell suites and both PostgreSQL children finish
 * (#1102), so nothing overlaps it and every second here is a second on the gate.
 *
 * A FUNCTION, NOT A SHARED INSTANCE, and that is not a style choice. `vite.config.ts` sets
 * `isolate: false` so the whole run shares one module registry: a `const` instance exported from
 * here would be ONE pointer state shared by all six files that use it, and a stale instance after
 * `cleanup()` is a defect that presents as another test's flake. Each caller takes its own.
 *
 * WHAT THIS DOES NOT DO: it removes waiting, not assertions. `findBy*`/`waitFor` still wait for the
 * DOM to reach the asserted state. If removing the delay ever makes a cell pass without the state
 * it claims, that cell was asserting the delay rather than the behaviour.
 */
export function fastUserEvent() {
  return userEventBase.setup({ delay: null });
}
