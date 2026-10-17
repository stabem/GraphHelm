/* #1192 (optimization plan A1, slice 2): the Studio reads the task projection the Runtime folds
 * (`GET /v1/executions/{id}/tasks`, #1149) instead of opening every `task.*` envelope and folding
 * it in the browser. The reply carries the live slices, the verified records they came from, the
 * stream head and how many envelopes did not verify. Shapes mirror `core/execution/src/tasks.rs`
 * (camelCase on the wire); this file only checks the envelope's outer shape, since the Runtime is
 * the one source for the fold. */
import type { TaskEventRecord, TaskState } from "./team-tasks";

/** One slice the Runtime's fold created, live or not: `ended` is a `task.closed`, `absorbed` a
 * PR-less claim folded into its PR's slice. Live slices are the ones `tasks` lists. */
export interface FoldSlice { id: number; ended: boolean; absorbed: boolean; state: TaskState }
/** Where one record landed: the id of the slice the fold routed it to. */
export interface Route { sequence: number; slice: number }
/** What a lane timeline needs beside the records: every slice and every record's slice. */
/** #1234 (plan A7): the Runtime judges lane liveness once and serves the budgets it judged by;
 * the views read these and keep no threshold of their own. Absent on a Runtime that predates it. */
export interface LivenessBudgets { silentAfterSeconds: number; freeAfterSeconds: number; stallAfterSeconds: number }
export interface LaneLiveness { lane: string; lastRecordAt?: string; owes: boolean; owesSince?: string; silent: boolean; free: boolean }
export interface Liveness { budgets: LivenessBudgets; lanes: LaneLiveness[] }
/** #1193: a live pre-PR claim's lease; `overdue` is the stale-claim verdict. */
export interface ClaimLease { key: string; taskId: string; lane: string; claimSequence: number; leaseSeconds: number; expiresAt: string; overdue: boolean }
/** #1469: one open wave issue nobody has picked up: the newest `task.queued` record lists it and no
 * live slice holds it. `openedAt` is when the issue was opened, as the recorder read it. */
export interface BacklogIssue { issue: number; title: string; openedAt: string }
/** #1516: how long each delivery step lasted, derived by the Runtime from the task records. `steps` is in
 * seconds per step name (implement, review_wait, fix, merge_wait, total); `summary` is the median and p90 of
 * each step over the slices merged in the window. Absent on a Runtime that predates it. */
export interface SliceDurations { key: string; pr: number; merged: boolean; steps: Record<string, number> }
export interface StepSummary { count: number; medianSeconds: number; p90Seconds: number }
export interface TaskDurations { slices: SliceDurations[]; summary: Record<string, StepSummary> }
export interface LaneFold { slices: FoldSlice[]; routes: Route[]; liveness?: Liveness; leases?: ClaimLease[]; backlog?: BacklogIssue[]; durations?: TaskDurations }

export interface TasksReply extends LaneFold {
  executionId: string;
  headSequence: number;
  tasks: TaskState[];
  /** The verified `task.*` records, in sequence order, each with the Runtime's `occurredAt`. */
  records: TaskEventRecord[];
  /** Envelopes whose digest, kind or signer did not match their recorded event; never drawn. */
  rejected: number;
}

/** The one sentence the Team, Graph and Lanes views show in place of the board when the Runtime
 * answered the tasks route with 404 or 401 (an older binary). */
export const RUNTIME_PREDATES_TASKS_ROUTE = "Runtime predates the tasks route · restart it on the current binary";

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

/** The route's data, or `null` when it is not the reply shape this Studio knows. */
export function tasksReply(value: unknown): TasksReply | null {
  const data = record(value);
  if (data === null || typeof data.executionId !== "string" || typeof data.headSequence !== "number"
    || !Array.isArray(data.tasks) || !Array.isArray(data.records) || typeof data.rejected !== "number") return null;
  return data as unknown as TasksReply;
}
