/* #668 (owner): the Lanes work kanban. Every open PR sits in one column, read only from the existing
 * rules: the stage (`workStage`), the owner lane's build slot (`slotStatus`, via `stageHealth`) and its
 * liveness (`laneLiveness`, via `stageHealth`'s Stalled flag). Pure. */
import type { StageHealth } from "./stage-health";
import type { WorkStage } from "./work-groups";

export type KanbanColumn = "implement" | "review" | "blocked" | "build" | "silent" | "merge";
export const KANBAN_COLUMNS: readonly { id: KanbanColumn; label: string }[] = [
  { id: "implement", label: "Implement" }, { id: "review", label: "Review" }, { id: "blocked", label: "Blocked" },
  { id: "build", label: "Waiting for build" }, { id: "silent", label: "Silent" }, { id: "merge", label: "Merge" },
];

/** The column one PR sits in, or `null` for merged work (not shown). Waiting for build (the owner
 * lane holds or waits for a build slot) and Silent (the owner lane silent ≥ LIVENESS_MS) take
 * precedence over the stage, so a stalled review shows under Silent. */
export function kanbanColumn(stage: WorkStage, health: StageHealth | null): KanbanColumn | null {
  if (stage === "merged" || stage === "proven") return null;
  if (health?.flag === "building" || health?.flag === "waiting_build") return "build";
  if (health?.flag === "stalled") return "silent";
  if (stage === "fix") return "blocked";
  if (stage === "review") return "review";
  if (stage === "merge") return "merge";
  return "implement";
}

/** Items bucketed by column, every column present (empty ones too), in KANBAN_COLUMNS order. */
export function bucketKanban<T>(items: T[], columnOf: (item: T) => KanbanColumn | null): Record<KanbanColumn, T[]> {
  const out = Object.fromEntries(KANBAN_COLUMNS.map((c) => [c.id, [] as T[]])) as unknown as Record<KanbanColumn, T[]>;
  for (const it of items) { const c = columnOf(it); if (c) out[c].push(it); }
  return out;
}
