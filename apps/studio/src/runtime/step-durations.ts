/* #1516: the delivery step durations the Runtime derives (`durations` on the tasks route), read for the
 * Lanes board: the step that took longest on a card, and the median and p90 of each step. Pure. */
import { stageDuration } from "./stage-health";
import type { SliceDurations, StepSummary, TaskDurations } from "./tasks-route";

/** The steps a card can name, in delivery order. `total` is the sum a reader compares them against. */
export const DURATION_STEPS: readonly { id: string; label: string }[] = [
  { id: "implement", label: "Implement" }, { id: "review_wait", label: "Review wait" },
  { id: "fix", label: "Fix after BLOCK" }, { id: "merge_wait", label: "Merge wait" },
];

export interface SlowestStep { id: string; label: string; seconds: number }

/** The step that took longest on one slice, or `null` when no step has lasted a second yet. */
export function slowestStep(row: SliceDurations | undefined): SlowestStep | null {
  if (!row) return null;
  let best: SlowestStep | null = null;
  for (const step of DURATION_STEPS) {
    const seconds = row.steps[step.id];
    if (typeof seconds === "number" && seconds > 0 && (best === null || seconds > best.seconds)) best = { ...step, seconds };
  }
  return best;
}

/** "Slowest: Implement 1h 12m" for a card, or `null` when there is nothing to name. */
export function slowestText(durations: TaskDurations | undefined, key: string): string | null {
  const best = slowestStep(durations?.slices.find((row) => row.key === key));
  return best ? `Slowest: ${best.label} ${stageDuration(best.seconds * 1000)}` : null;
}

export interface SummaryCell { id: string; label: string; stat: StepSummary }

/** The median and p90 of every step the window has data for, in delivery order, `total` last. */
export function summaryCells(durations: TaskDurations | undefined): SummaryCell[] {
  const summary = durations?.summary ?? {};
  return [...DURATION_STEPS, { id: "total", label: "Claim to merge" }]
    .flatMap((step) => (summary[step.id] ? [{ ...step, stat: summary[step.id]! }] : []));
}
