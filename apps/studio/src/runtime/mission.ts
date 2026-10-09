import type { JourneyRunView, JourneyView } from "./types";
import type { TaskState } from "./team-tasks";

export type StepStatus = "proven" | "failed" | "needs_you" | "preview_only" | "not_run";
export type TrustLevel = 0 | 1 | 2 | 3 | 4 | 5;
export const TRUST_LABELS = ["Written", "Reviewed", "Merged", "Proven", "Seen by you"] as const;

export interface MissionStep { stepId: string; index: number; title: string; status: StepStatus; reason: string | null }
export interface MissionTask { key: string; pr: number | null; issue: number | null; title: string; lane: string | null; reviewers: string[]; step: TaskState["step"]; blocked: boolean; trust: TrustLevel }
export interface MissionSummary { proven: number; total: number; inFlight: number; needYou: number }
export interface Mission { contractId: string; title: string; steps: MissionStep[]; tasks: MissionTask[]; summary: MissionSummary }

const STEP_ORDER: Record<TaskState["step"], number> = { implement: 0, review: 1, merge: 2, merged: 3 };

/** Run-edge keys are the flow's own edge ids (free-form; `graphhelm journey explore` writes
 * `<from>.<to>`, fixtures use `<from>-><to>`), not a fixed "from->to". An edge leads into a step
 * when the key is, or ends with `->` or `.` followed by, that step id. */
export function skippedEdgeInto(run: JourneyRunView | null, stepId: string) {
  return Object.entries(run?.edges ?? {}).find(([id, e]) =>
    e.result === "skipped" && (id === stepId || id.endsWith(`->${stepId}`) || id.endsWith(`.${stepId}`)));
}

function stepStatus(stepId: string, run: JourneyRunView | null): { status: StepStatus; reason: string | null } {
  if (!run || run.state === "none") return { status: "not_run", reason: null };
  const skipped = skippedEdgeInto(run, stepId);
  if (skipped) return { status: "needs_you", reason: skipped[1].reason ?? null };
  const screen = run.screens?.[stepId];
  if (!screen?.result) return { status: "not_run", reason: null };
  if (run.kind !== "replay") return { status: "preview_only", reason: screen.reason ?? null };
  return screen.result === "pass" ? { status: "proven", reason: null } : { status: "failed", reason: screen.reason ?? null };
}

export function toMissionTask(t: TaskState): MissionTask {
  const trust: TrustLevel = t.step === "merged" ? 3 : t.step === "merge" ? 2 : 1;
  return {
    key: t.key, pr: t.pr, issue: t.issue, title: t.prTitle ?? t.title ?? t.taskId, lane: t.lane,
    reviewers: t.reviewers, step: t.step, blocked: t.blockedBy !== null, trust,
  };
}

export function buildMission(journey: JourneyView, run: JourneyRunView | null, tasks: TaskState[]): Mission {
  const steps = journey.steps.map((s, index) => ({
    stepId: s.stepId, index, title: s.screen?.title ?? s.stepId, ...stepStatus(s.stepId, run),
  }));
  const linked = tasks
    .filter((t) => t.journeys.includes(journey.contractId))
    .sort((a, b) => STEP_ORDER[a.step] - STEP_ORDER[b.step])
    .map(toMissionTask);
  return {
    contractId: journey.contractId,
    title: journey.title,
    steps,
    tasks: linked,
    summary: {
      proven: steps.filter((s) => s.status === "proven").length,
      total: steps.length,
      inFlight: linked.filter((t) => t.step !== "merged").length,
      needYou: steps.filter((s) => s.status === "needs_you").length,
    },
  };
}

export function unlinkedTasks(journeys: JourneyView[], tasks: TaskState[]): MissionTask[] {
  const known = new Set(journeys.map((j) => j.contractId));
  return tasks.filter((t) => !t.journeys.some((id) => known.has(id))).map(toMissionTask);
}
