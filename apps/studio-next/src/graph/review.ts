import type { KanbanColumn } from "./requests";

/** #1802: a Kanban card opens the review only once there is a pull request to look at. */
export function reviewable(column: KanbanColumn, pr: number | null): boolean {
  return pr !== null && (column === "review" || column === "merged");
}

/** The frame routes the Runtime hands out (#1795): a journey's current or kept run frame, and
 * (#1867) a PR head's sealed frame on the open run, `/v1/executions/<that run>/evidence/
 * signal-<id>-image-<1..4>` (#1839), only when `executionId` names that same run. Only these are
 * fetched with the bearer, so a crafted URL cannot send it anywhere else. */
export function isFrameUrl(url: string, executionId?: string): boolean {
  if (/^\/v1\/journey-flows\/[A-Za-z0-9][A-Za-z0-9._:-]*\/(?:history\/\d{20}\/)?screens\/[A-Za-z0-9][A-Za-z0-9._:-]*\/frame(?:\?viewport=(?:web|mobile))?$/.test(url)) {
    return true;
  }
  if (executionId === undefined || !/^[A-Za-z0-9][A-Za-z0-9._:-]*$/.test(executionId)) return false;
  const prefix = `/v1/executions/${executionId}/evidence/`;
  return url.startsWith(prefix) && /^signal-[A-Za-z0-9][A-Za-z0-9._:-]*-image-[1-4]$/.test(url.slice(prefix.length));
}

export type ReviewMode = "frame" | "side" | "overlay" | "changes";

/** Which compare modes are on. Before is the main run's frame; Now is the PR head's (#1839). With
 * only one of them, only Frame is on: a compare against nothing is never drawn. */
export function modesOn(before: boolean, now: boolean): Record<ReviewMode, boolean> {
  const both = before && now;
  return { frame: before || now, side: both, overlay: both, changes: both };
}
