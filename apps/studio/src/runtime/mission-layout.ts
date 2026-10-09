import type { Mission, MissionTask } from "./mission";

export interface PlacedTask { task: MissionTask; col: number; row: number }
/** `done`: the source is merged, or the step it sits in is proven: drawn solid, else dashed. */
export interface LayoutEdge { from: string; to: string; done: boolean }
export interface MissionLayout { placed: PlacedTask[]; edges: LayoutEdge[]; rows: number }

const byPr = (a: MissionTask, b: MissionTask) =>
  (a.pr ?? Number.POSITIVE_INFINITY) - (b.pr ?? Number.POSITIVE_INFINITY) || a.key.localeCompare(b.key);

/**
 * Where each task sits on the mission graph.
 *
 * A task records which journeys it serves, never which step, so the column is derived, not read:
 * the frontier is the first step (by order) the replay has not proven. Work still open (implement,
 * review, merge) sits in the frontier's column; merged work sits one column past it (clamped to the
 * last step), because merging is not proving and the next step is what it unlocks. With every step
 * proven the frontier is the last step. Within a column, tasks stack in rows by PR number (no PR
 * last). Edges join consecutive tasks of the journey in PR order.
 */
export function layoutMission(mission: Mission): MissionLayout {
  const last = Math.max(0, mission.steps.length - 1);
  const open = mission.steps.findIndex((s) => s.status !== "proven");
  const frontier = open === -1 ? last : open;
  const ordered = [...mission.tasks].sort(byPr);
  const rowsInCol = new Map<number, number>();
  const placed = ordered.map((task) => {
    const col = task.step === "merged" ? Math.min(frontier + 1, last) : frontier;
    const row = rowsInCol.get(col) ?? 0;
    rowsInCol.set(col, row + 1);
    return { task, col, row };
  });
  const edges = placed.slice(1).map((b, i) => {
    const a = placed[i]!;
    return { from: a.task.key, to: b.task.key, done: a.task.step === "merged" || mission.steps[a.col]?.status === "proven" };
  });
  return { placed, edges, rows: Math.max(1, ...rowsInCol.values()) };
}
