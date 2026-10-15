import type { JourneyRunView } from "./types";

/** Plain words for the closed list of failure codes a run can carry (#519, #586). */
const WHY: Record<string, string> = {
  "watch.launch_failed": "the app under test did not start",
  "watch.launcher_invalid": "the project's launcher file (.graphhelm/journey-fixture.json) is not valid",
  "watch.app_down": "the app under test is not running and the project has no launcher to start it",
  "driver.observer_missing": "no browser observer is installed",
  "replay.observer_missing": "no browser observer is installed",
  "preview.budget_exceeded": "the run took longer than its time limit",
  "preview.launcher_not_isolated": "the launcher does not start its own copy of the app, so the run will not touch your real one",
  "driver.secret_missing": "a secret the journey types is not set",
  "driver.secret_literal": "the journey types a secret written in plain text",
  "driver.unsupported_act": "the journey uses a step the browser cannot do",
  "replay.act_value_missing": "a step has no value to type",
  "replay.entry_missing": "the journey has no first screen",
  "preview.runner_lost": "the program running the journey stopped unexpectedly",
  internal: "the program running the journey stopped unexpectedly",
};

/** The last lines of a failed launcher's output, the part that names the real cause. */
function logTail(run: JourneyRunView, lines = 6): string | null {
  const tail = run.launchLog?.tail?.trim();
  if (!tail) return null;
  return tail.split(/\r?\n/).filter((l) => l.trim() !== "").slice(-lines).join("\n");
}

export interface RunFailure {
  /** One sentence: where the run stopped and why. */
  text: string;
  /** 0-based index of the identified failed step; null when no failed step is known. */
  step: number | null;
  /** The launcher's last output lines, when the app did not start. */
  log: string | null;
  /** Where the full launcher log lives on disk. */
  logPath: string | null;
}

/** #1296: a failed run says which step failed, or where the failure can be located. */
export function runFailure(run: JourneyRunView, stepIds: string[]): RunFailure {
  const code = run.detail?.code ?? run.reason ?? "internal";
  const why = WHY[code] ?? WHY[run.reason ?? ""] ?? `the Runtime refused the run (${code})`;
  const pointer = run.detail?.pointer ?? "";
  // The pointer names a step id when the play failed on it; the result of a screen also does.
  let step = stepIds.findIndex((id) => pointer === `/${id}` || pointer.startsWith(`/${id}/`) || pointer === id);
  if (step < 0) step = stepIds.findIndex((id) => run.screens?.[id]?.result && run.screens[id]!.result !== "pass");
  const log = logTail(run);
  const progressed = stepIds.some((id) => run.screens?.[id]?.result === "pass");
  const where = step >= 0 ? `Step ${step + 1} failed`
    : progressed ? "The run failed; the failing step is unknown" : "It stopped before step 1";
  return {
    text: `${where}: ${why}.`,
    step: step >= 0 ? step : null,
    log,
    logPath: run.launchLog?.path ?? null,
  };
}
