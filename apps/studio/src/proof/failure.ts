import type { JourneyRun } from "../runtime/client";
import { isHeld, isWaitingReview, reasonSentence, type Step } from "./steps";

/** #1298: the log a failed run carries, its last lines and whose they are. */
export interface FailureLog { label: "Launcher" | "Driver"; lines: string[] }

/** #1298: what the Proof alert says about a failed run: one sentence naming the failing step and
 * why, or that the run stopped before step 1, or that the failing step is unknown; the error code
 * and step position; the launcher's (else the driver's) last lines; and the text Copy details copies. */
export interface FailureNotice { sentence: string; code: string | null; log: FailureLog | null; details: string }

/** #1298: the reason code a launch that never served the app records. */
const LAUNCH_FAILED = "watch.launch_failed";

const sentence = (text: string) => (/[.!?]$/.test(text.trim()) ? text.trim() : `${text.trim()}.`);
const lower = (text: string) => text.charAt(0).toLowerCase() + text.slice(1);

function why(code: string | null | undefined): string {
  if (!code) return "no reason was recorded.";
  if (code === LAUNCH_FAILED) return "the app did not start.";
  return sentence(lower(reasonSentence(code) ?? code));
}

function lastLines(tail: string | undefined): string[] {
  return (tail ?? "").split(/\r?\n/).map((line) => line.trimEnd()).filter((line) => line.trim() !== "");
}

/** #1298: the alert for a run that failed or could not finish; `null` for any other run. */
export function failureNotice(run: JourneyRun | null, steps: readonly Step[]): FailureNotice | null {
  if (run === null || isHeld(run) || isWaitingReview(run)) return null;
  if (!(run.state === "failed" || (run.state === "ready" && run.result === "fail"))) return null;
  const step = steps.find((each) => each.status === "failed") ?? null;
  const progressed = steps.some((each) => each.status === "passed" || each.status === "drift");
  let said: string;
  let code: string | null;
  if (step !== null) {
    code = step.reason ?? run.reason ?? null;
    said = `Failed at step ${step.n} (${step.label}): ${why(code)}`;
  } else if (!progressed) {
    code = run.reason ?? null;
    said = `Stopped before step 1: ${why(code)}`;
  } else {
    code = run.reason ?? null;
    said = "Failed; the failing step is unknown.";
  }
  const where = step === null ? null : `step ${step.n} of ${steps.length}`;
  const launcher = lastLines(run.launchLog?.tail);
  const driver = lastLines(run.driverLog?.tail);
  const log: FailureLog | null = launcher.length > 0 ? { label: "Launcher", lines: launcher } : driver.length > 0 ? { label: "Driver", lines: driver } : null;
  const head = [code, where].filter((part): part is string => part !== null).join(" · ");
  const details = [said, head, ...(log === null ? [] : [`${log.label}, last ${log.lines.length} lines:`, ...log.lines])].filter((line) => line !== "").join("\n");
  return { sentence: said, code: head === "" ? null : head, log, details };
}
