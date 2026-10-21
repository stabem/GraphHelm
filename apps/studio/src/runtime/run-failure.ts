import type { JourneyRunView } from "./types";

/** Plain words for the closed list of failure codes a run can carry (#519, #586). */
const WHY: Record<string, string> = {
  "watch.launch_failed": "the app under test did not start",
  "watch.launcher_invalid": "the project's launcher file (.graphhelm/journey-fixture.json) is not valid",
  "watch.app_down": "the app under test is not running and the project has no launcher to start it",
  "driver.observer_missing": "the browser observer is missing; Run again to install it",
  "replay.observer_missing": "the browser observer is missing; Run again to install it",
  "replay.observer_stale": "the browser observer is out of date; Run again to refresh it",
  "observer.setup_failed": "browser observer setup failed",
  "observer.setup_timeout": "browser observer setup timed out",
  "preview.budget_exceeded": "the run took longer than its time limit",
  "preview.launcher_not_isolated": "the launcher does not start its own copy of the app, so the run will not touch your real one",
  "driver.secret_missing": "a secret the journey types is not set",
  "driver.secret_literal": "the journey types a secret written in plain text",
  "driver.unsupported_act": "the journey uses a step the browser cannot do",
  "replay.act_value_missing": "a step has no value to type",
  "replay.entry_missing": "the journey has no first screen",
  "preview.runner_lost": "the program running the journey stopped unexpectedly",
  "replay.driver_frame_invalid": "the browser driver stopped before it answered",
  internal: "the program running the journey stopped unexpectedly",
};

/** The last non-empty lines of a kept output tail, the part that names the real cause. */
function lastLines(tail: string | undefined, lines = 6): string | null {
  const text = tail?.trim();
  if (!text) return null;
  return text.split(/\r?\n/).filter((l) => l.trim() !== "").slice(-lines).join("\n");
}

export interface RunFailure {
  /** One sentence: where the run stopped and why. */
  text: string;
  /** 0-based index of the identified failed step; null when no failed step is known. */
  step: number | null;
  /** The last output lines that name the cause: the launcher's, else the browser driver's. */
  log: string | null;
  /** Whose output `log` is (#1448). */
  logFrom: "launcher" | "driver" | null;
  /** Where the full launcher log lives on disk. */
  logPath: string | null;
}

/** #1296: a failed run says which step failed, or where the failure can be located. */
export function runFailure(run: JourneyRunView, stepIds: string[]): RunFailure {
  const code = run.detail?.code ?? run.reason ?? "internal";
  const baseWhy = WHY[code] ?? WHY[run.reason ?? ""] ?? `the Runtime refused the run (${code})`;
  const why = run.observerSetup?.error && (code === "observer.setup_failed" || code === "observer.setup_timeout")
    ? `${baseWhy}: ${run.observerSetup.error}`
    : baseWhy;
  const pointer = run.detail?.pointer ?? "";
  // The pointer names a step id when the play failed on it; the result of a screen also does.
  let step = stepIds.findIndex((id) => pointer === `/${id}` || pointer.startsWith(`/${id}/`) || pointer === id);
  if (step < 0) step = stepIds.findIndex((id) => run.screens?.[id]?.result && run.screens[id]!.result !== "pass");
  const launcher = lastLines(run.launchLog?.tail);
  const log = launcher ?? lastLines(run.driverLog?.tail);
  const logFrom = launcher ? "launcher" : log ? "driver" : null;
  const progressed = stepIds.some((id) => run.screens?.[id]?.result === "pass");
  const where = step >= 0 ? `Step ${step + 1} failed`
    : progressed ? "The run failed; the failing step is unknown" : "It stopped before step 1";
  return {
    text: `${where}: ${why}.`,
    step: step >= 0 ? step : null,
    log,
    logFrom,
    logPath: run.launchLog?.path ?? null,
  };
}
