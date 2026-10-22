import type { LaneAction } from "../runtime/client";

export function clock(at: string | null | undefined): string {
  if (!at) return "--:--";
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? "--:--" : `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

/** One action as the card and the panel print it. */
export function actionLine(action: LaneAction): string {
  return `${clock(action.occurredAt)} ${actionText(action)}`;
}

/** What one action did, without its time. */
export function actionText(action: LaneAction): string {
  const pr = action.pr === undefined ? "" : ` PR #${action.pr}`;
  const issue = action.issue === undefined ? "" : ` #${action.issue}`;
  const what: Record<string, string> = {
    "task.claimed": `claimed${issue}`,
    "task.planned": `planned${issue}`,
    "task.released": `released${issue}`,
    "task.heartbeat": action.reason ?? "heartbeat",
    "task.pr_opened": `opened${pr}`,
    "task.review_assigned": `assigned${pr} to ${action.reviewer ?? "a reviewer"}`,
    "task.review_verdict": `${action.verdict ?? "verdict"} on${pr}`,
    "task.review_activity": action.activity ?? `reviewing${pr}`,
    "task.merged": `merged${pr}`,
    "task.closed": `closed${issue}`,
    "task.critic_verdict": `critic verdict${issue}`,
    "task.queued": "queued the wave",
    "task.lane_retired": "retired lanes",
  };
  return what[action.kind] ?? action.kind;
}
