import { RuntimeError, type RuntimeClient } from "./client";
import type { TaskState } from "./team-tasks";

/** The review_assigned document from tools/task-record/task_record.py, including its default ordinal. */
export function reviewAssignmentDocument(task: Pick<TaskState, "taskId" | "pr" | "headSha">, lane: string, at: string) {
  if (task.pr === null || !task.headSha) throw new Error("The review needs a recorded PR and current head.");
  return { schema: "graphhelm-task-event-v1", taskId: task.taskId, revision: 3, at,
    pr: task.pr, headSha: task.headSha, reviewer: lane, ordinal: 1 };
}

export async function recordReviewAssigned(client: Pick<RuntimeClient, "signal">, run: string,
  task: Pick<TaskState, "taskId" | "pr" | "headSha">, lane: string) {
  const at = new Date().toISOString();
  const document = reviewAssignmentDocument(task, lane, at);
  const result = await client.signal(run, JSON.stringify(document), { kind: "task.review_assigned", emittedAt: at });
  if (result.result !== "succeeded") {
    throw new RuntimeError(result.diagnostics.map((d) => `${d.code}: ${d.message}`).join("; ") || "The Runtime did not confirm the review assignment.", 0, result.diagnostics);
  }
  return result;
}
