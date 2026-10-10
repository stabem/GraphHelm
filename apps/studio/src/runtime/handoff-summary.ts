import type { TaskState } from "./team-tasks";

/** Only recorded work and obligations, in input order; never recommendations or priorities. */
export function handoffSummary(tasks: TaskState[]) {
  const now = tasks.filter((task) => task.step === "implement" || task.step === "review")
    .map((task) => ({ key: task.key, issue: task.issue, lane: task.lane, step: task.step, since: task.clock.since }));
  const newest = tasks.reduce<TaskState | null>((last, task) => last === null || task.lastSequence > last.lastSequence ? task : last, null);
  // TaskState keeps step entry time, not last-event time. Do not relabel that clock as activity.
  const last = newest === null ? null : { key: newest.key, issue: newest.issue, lane: newest.lane,
    step: newest.step === "merged" && !newest.mergeSha ? "not recorded" : newest.step,
    sequence: newest.lastSequence, at: null };
  const next: { key: string; text: string }[] = [];
  for (const task of tasks) {
    if (task.blockedBy) next.push({ key: `${task.key}:fix`, text: `fix owed by ${task.lane ?? "not recorded"} on ${task.blockedBy.headSha.slice(0, 8) || "not recorded"}` });
    else if (task.step === "review" && task.headSha) {
      for (const reviewer of task.assignedReviewers ?? []) next.push({ key: `${task.key}:${reviewer}`, text: `review owed by ${reviewer}` });
    } else if (task.step === "merge") next.push({ key: `${task.key}:merge`, text: "merge owed" });
  }
  return { now, last, next };
}
