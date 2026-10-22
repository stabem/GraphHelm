import type { KanbanColumn } from "./requests";

/** #1802: a Kanban card opens the review only once there is a pull request to look at. */
export function reviewable(column: KanbanColumn, pr: number | null): boolean {
  return pr !== null && (column === "review" || column === "merged");
}

/** The frame routes the Runtime hands out (#1795): a journey's current or kept run frame. Only
 * these are fetched with the bearer, so a crafted URL cannot send it anywhere else. */
export function isFrameUrl(url: string): boolean {
  return /^\/v1\/journey-flows\/[A-Za-z0-9][A-Za-z0-9._:-]*\/(?:history\/\d{20}\/)?screens\/[A-Za-z0-9][A-Za-z0-9._:-]*\/frame$/.test(url);
}

export type ReviewMode = "frame" | "side" | "overlay" | "changes";

/** Which compare modes are on. Before is the main run's frame; Now is the PR head's, which no
 * route serves until #1839, so with no Now only Frame (Before) is shown, never a guess. */
export function modesOn(before: boolean, now: boolean): Record<ReviewMode, boolean> {
  const both = before && now;
  return { frame: before || now, side: both, overlay: both, changes: both };
}
