/* #1192 (optimization plan A1, slice 2): the Studio reads the task projection the Runtime folds
 * (`GET /v1/executions/{id}/tasks`, #1149) instead of opening every `task.*` envelope and folding
 * it in the browser. The reply carries the live slices, the verified records they came from, the
 * stream head and how many envelopes did not verify. Shapes mirror `core/execution/src/tasks.rs`
 * (camelCase on the wire); this file only checks the envelope's outer shape, since the Runtime is
 * the one source for the fold. */
import type { TaskEventRecord, TaskState } from "./team-tasks";

export interface TasksReply {
  executionId: string;
  headSequence: number;
  tasks: TaskState[];
  /** The verified `task.*` records, in sequence order, each with the Runtime's `occurredAt`. */
  records: TaskEventRecord[];
  /** Envelopes whose digest, kind or signer did not match their recorded event; never drawn. */
  rejected: number;
}

/** What the views need when the Runtime predates the route: say so, never an empty board. */
export type TasksRead =
  | { kind: "tasks"; reply: TasksReply }
  | { kind: "unsupported" };

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
