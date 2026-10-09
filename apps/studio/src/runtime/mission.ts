import type { JourneyRunView, JourneyView } from "./types";
import type { TaskState } from "./team-tasks";

export type StepStatus = "proven" | "failed" | "needs_you" | "preview_only" | "not_run";
export type TrustLevel = 0 | 1 | 2 | 3 | 4 | 5;
export const TRUST_LABELS = ["Written", "Reviewed", "Merged", "Proven", "Seen by you"] as const;

export interface MissionStep { stepId: string; index: number; title: string; status: StepStatus; reason: string | null; promise: string | null }
/** One BLOCK round of the review loop: who blocked which head, and the head the author pushed back. */
export interface MissionRound { reviewer: string; headSha: string; fixHead: string | null }
export interface MissionTask {
  key: string; pr: number | null; issue: number | null; title: string; lane: string | null; reviewers: string[];
  step: TaskState["step"]; blocked: boolean; trust: TrustLevel;
  /** The unanswered BLOCK, if any: the inspector names who and on which head. */
  blockedBy: { reviewer: string; headSha: string } | null;
  rounds: MissionRound[];
  headSha: string | null;
  mergeSha: string | null;
  /** `https://github.com/<owner>/<repo>`; the PR link needs it. */
  repoUrl: string | null;
}
export interface MissionSummary { proven: number; total: number; inFlight: number; needYou: number; readyUnclaimed: number }
export interface Mission { contractId: string; title: string; steps: MissionStep[]; tasks: MissionTask[]; summary: MissionSummary }

const STEP_ORDER: Record<TaskState["step"], number> = { implement: 0, review: 1, merge: 2, merged: 3 };

/** Run-edge keys are the flow's own edge ids (free-form; `graphhelm journey explore` writes
 * `<from>.<to>`, fixtures use `<from>-><to>`). They cannot be resolved through JourneyView.arrows:
 * an arrow's `transitionSignalId` is a ledger signal id, unrelated to a flow edge id. So an edge
 * leads into a step only on an exact `<from>.<to>` or `<from>-><to>` match, where `from` is one of
 * the journey's other steps (or the key is the step id). Anything else is unmatched, never guessed. */
export function skippedEdgeInto(run: JourneyRunView | null, stepId: string, stepIds: string[]) {
  const keys = new Set([stepId, ...stepIds.filter((f) => f !== stepId).flatMap((f) => [`${f}.${stepId}`, `${f}->${stepId}`])]);
  return Object.entries(run?.edges ?? {}).find(([id, e]) => e.result === "skipped" && keys.has(id));
}

function stepStatus(stepId: string, run: JourneyRunView | null, stepIds: string[]): { status: StepStatus; reason: string | null } {
  if (!run || run.state === "none") return { status: "not_run", reason: null };
  const skipped = skippedEdgeInto(run, stepId, stepIds);
  if (skipped) return { status: "needs_you", reason: skipped[1].reason ?? null };
  const screen = run.screens?.[stepId];
  if (!screen?.result) return { status: "not_run", reason: null };
  if (run.kind !== "replay") return { status: "preview_only", reason: screen.reason ?? null };
  return screen.result === "pass" ? { status: "proven", reason: null } : { status: "failed", reason: screen.reason ?? null };
}

export function toMissionTask(t: TaskState): MissionTask {
  const trust: TrustLevel = t.step === "merged" ? 3 : t.step === "merge" ? 2 : 1;
  return {
    key: t.key, pr: t.pr, issue: t.issue, title: t.prTitle || t.title || t.taskId, lane: t.lane,
    reviewers: t.reviewers, step: t.step, blocked: t.blockedBy !== null, trust,
    blockedBy: t.blockedBy ? { reviewer: t.blockedBy.reviewer, headSha: t.blockedBy.headSha } : null,
    rounds: (t.rounds ?? []).map((r) => ({ reviewer: r.reviewer, headSha: r.headSha, fixHead: r.fixHead })),
    headSha: t.headSha ?? null, mergeSha: t.mergeSha ?? null, repoUrl: t.repoUrl ?? null,
  };
}

export function buildMission(journey: JourneyView, run: JourneyRunView | null, tasks: TaskState[]): Mission {
  const steps = journey.steps.map((s, index) => ({
    stepId: s.stepId, index, title: s.screen?.title ?? s.stepId, promise: s.promises?.[0] ?? null, ...stepStatus(s.stepId, run, journey.steps.map((x) => x.stepId)),
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
      // A linked task no lane has claimed: work that is ready and waits for an agent.
      readyUnclaimed: linked.filter((t) => t.lane === null).length,
    },
  };
}

export function unlinkedTasks(journeys: JourneyView[], tasks: TaskState[]): MissionTask[] {
  const known = new Set(journeys.map((j) => j.contractId));
  return tasks.filter((t) => !t.journeys.some((id) => known.has(id))).map(toMissionTask);
}

export const sha8 = (sha: string | null | undefined) => (sha ? sha.slice(0, 8) : null);

export type CustodyTone = "ok" | "block" | "run" | "dim";
export interface CustodyRow { stage: "Implement" | "Review" | "Fix" | "Re-review" | "Merge"; who: string; verdict: string; tone: CustodyTone }

/** "Who touched it", read only from the task's own records: the claim, each BLOCK round and the
 * fix pushed in answer, the approval that moved it to merge, and the merge commit. */
export function custodyRows(t: MissionTask): CustodyRow[] {
  const rows: CustodyRow[] = [{ stage: "Implement", who: t.lane ?? "—", verdict: t.step === "implement" ? "working" : "done", tone: t.step === "implement" ? "run" : "ok" }];
  t.rounds.forEach((r, i) => {
    rows.push({ stage: i === 0 ? "Review" : "Re-review", who: r.reviewer || "—", verdict: "BLOCK", tone: "block" });
    if (r.fixHead) rows.push({ stage: "Fix", who: t.lane ?? "—", verdict: `pushed ${sha8(r.fixHead)}`, tone: "ok" });
  });
  const stage = t.rounds.length > 0 ? "Re-review" : "Review";
  const who = t.reviewers.join(", ") || "—";
  if (t.step === "merge" || t.step === "merged") rows.push({ stage, who, verdict: "APPROVE", tone: "ok" });
  else if (t.step === "review" && !t.blocked) rows.push({ stage, who, verdict: "pending", tone: "run" });
  if (t.step === "merged") rows.push({ stage: "Merge", who: sha8(t.mergeSha) ?? "—", verdict: "merged", tone: "ok" });
  return rows;
}
