import type { JourneyRunView, JourneyView } from "./types";
import type { TaskState } from "./team-tasks";
import { buildMission, toMissionTask, type MissionTask } from "./mission";
import { layoutMission, type LayoutEdge, type PlacedTask } from "./mission-layout";

/** #583 (+ #554 Plan): the columns of an issue's graph, in the order work moves through them. */
export type WorkStage = "plan" | "implement" | "review" | "fix" | "merge" | "merged" | "proven";
export const WORK_STAGES: readonly { id: WorkStage; label: string }[] = [
  { id: "plan", label: "Plan" }, { id: "implement", label: "Implement" }, { id: "review", label: "Review" }, { id: "fix", label: "Fix" },
  { id: "merge", label: "Merge" }, { id: "merged", label: "Merged" }, { id: "proven", label: "Proven" },
];

export interface WorkGroup {
  /** `issue-<n>`, or `no-issue` for the one group of tasks that name no issue. */
  key: string;
  issue: number | null;
  /** The issue's title as `task.claimed` recorded it, or `null` when none did. */
  title: string | null;
  /** What the rail and the graph heading print: `#<n> <title>`, `Issue #<n>`, or `No issue`. */
  label: string;
  /** In PR order (no PR last). */
  tasks: MissionTask[];
  stages: Record<string, WorkStage>;
  /** The union of the tasks' journeys, first-seen order. */
  journeyIds: string[];
  /** Some task is not merged yet. */
  open: boolean;
  /** The newest record any of its tasks carries. */
  latest: number;
  /** #591: the issue's one-line summary, from the first task that recorded one. */
  summary: string | null;
  /** #591: the task that most needs the owner (blocked, else in flight, else the newest). */
  focus: string | null;
}

/** #591: blocked work first, then work in flight, then the newest record; ties keep PR order. */
export function focusTask(rows: TaskState[]): string | null {
  const rank = (t: TaskState) => (t.blockedBy !== null ? 0 : t.step !== "merged" ? 1 : 2);
  const best = [...rows].sort(byPr).sort((a, b) => rank(a) - rank(b) || (b.lastSequence ?? 0) - (a.lastSequence ?? 0))[0];
  return best?.key ?? null;
}

/**
 * Which column a task sits in, read only from its own records. Fix: an unanswered BLOCK, or the
 * fix pushed in answer to the last BLOCK while the re-review has not approved yet. Proven: merged
 * and the journey replay passed the step the task serves (`proven`); merged alone is Merged.
 */
export function workStage(t: TaskState, proven: boolean): WorkStage {
  if (t.step === "merged") return proven ? "proven" : "merged";
  if (t.blockedBy !== null) return "fix";
  const last = t.rounds?.[t.rounds.length - 1];
  if (t.step === "review" && last?.fixHead) return "fix";
  // #554: planning, and a design plan waiting on its critic, sit in the Plan column.
  return t.step === "critic" ? "plan" : t.step;
}

const byPr = (a: { pr: number | null; key: string }, b: { pr: number | null; key: string }) =>
  (a.pr ?? Number.POSITIVE_INFINITY) - (b.pr ?? Number.POSITIVE_INFINITY) || a.key.localeCompare(b.key);

/** Merged work counts as proven when one of its journeys places it on a step the replay passed. */
function provenKeys(tasks: TaskState[], journeys: JourneyView[], runFor?: (contractId: string) => JourneyRunView | null): Set<string> {
  const keys = new Set<string>();
  if (!runFor) return keys;
  for (const j of journeys) {
    if (!tasks.some((t) => t.journeys.includes(j.contractId))) continue;
    const mission = buildMission(j, runFor(j.contractId), tasks);
    for (const p of layoutMission(mission).placed) {
      if (p.task.step === "merged" && mission.steps[p.col]?.status === "proven") keys.add(p.task.key);
    }
  }
  return keys;
}

/** #583: one group per issue, the grouping the Team view draws; groups with open work first, then newest. */
export function buildWorkGroups(tasks: TaskState[], journeys: JourneyView[], runFor?: (contractId: string) => JourneyRunView | null): WorkGroup[] {
  const proven = provenKeys(tasks, journeys, runFor);
  const byIssue = new Map<string, TaskState[]>();
  for (const t of tasks) {
    const key = t.issue !== null ? `issue-${t.issue}` : "no-issue";
    byIssue.set(key, [...(byIssue.get(key) ?? []), t]);
  }
  const groups = [...byIssue.entries()].map(([key, rows]): WorkGroup => {
    const issue = rows[0]!.issue;
    const sorted = [...rows].sort(byPr);
    const named = rows.find((r) => r.title);
    return {
      key, issue,
      title: named?.title ?? null,
      label: issue === null ? "No issue" : named?.title ? `#${issue} ${named.title}` : `Issue #${issue}`,
      tasks: sorted.map(toMissionTask),
      stages: Object.fromEntries(sorted.map((t) => [t.key, workStage(t, proven.has(t.key))])),
      journeyIds: [...new Set(sorted.flatMap((t) => t.journeys))],
      open: rows.some((r) => r.step !== "merged"),
      latest: Math.max(...rows.map((r) => r.lastSequence ?? 0)),
      summary: sorted.find((r) => r.summary)?.summary ?? null,
      focus: focusTask(rows),
    };
  });
  return groups.sort((a, b) => Number(b.open) - Number(a.open) || b.latest - a.latest);
}

export interface WorkLayout { placed: PlacedTask[]; edges: LayoutEdge[]; rows: number }

/** Each task in its stage's column, stacked in PR order; edges join consecutive tasks in PR order,
 * solid when the source is merged. */
export function layoutWorkGroup(group: WorkGroup): WorkLayout {
  const rowsInCol = new Map<number, number>();
  const placed = group.tasks.map((task) => {
    const col = WORK_STAGES.findIndex((s) => s.id === group.stages[task.key]);
    const row = rowsInCol.get(col) ?? 0;
    rowsInCol.set(col, row + 1);
    return { task, col, row };
  });
  const edges = placed.slice(1).map((b, i) => ({ from: placed[i]!.task.key, to: b.task.key, done: placed[i]!.task.step === "merged" }));
  return { placed, edges, rows: Math.max(1, ...rowsInCol.values()) };
}
