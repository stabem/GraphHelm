import type { JourneyRunView, JourneyView } from "./types";
import type { TaskState } from "./team-tasks";
import { buildMission, namedTask, openBlock, toMissionTask, type MissionTask } from "./mission";
import { layoutMission } from "./mission-layout";
import { duration, type TimedStep } from "./step-timing";

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
  /** #591: one row per PR, each with its own path along the stage columns; open rows first
   * (most urgent first, the `focusTask` rule), merged rows last. */
  rows: PrRow[];
}

/** #591: where a cell sits on its PR's own path. */
export type CellState = "done" | "block" | "current" | "ahead";
export interface PathCell {
  stage: WorkStage;
  col: number;
  state: CellState;
  /** Mono label: the stage, with a counter when it repeated (`Review ×2`). */
  label: string;
  /** Who acted there (lane, reviewer, merge sha), when recorded. */
  who: string | null;
  /** `✓`, `BLOCK`, `skipped`, `fix pushed`, or `null` for the current and ahead cells. */
  mark: string | null;
  /** Time spent there, when the Runtime's clock recorded it. */
  time: string | null;
  count: number;
  /** #591: the current card's heading when the stage alone does not say it (`Fixing`, `Re-review`). */
  title: string | null;
  /** #591: the current card's extra line (`after BLOCK by <reviewer>`). */
  sub: string | null;
}
/** An arrow inside one row: `col` to `col`, solid between done cells, dashed into the current one;
 * `loop` is the Fix back to the re-review in the Review column. */
export interface PathEdge { row: string; from: number; to: number; kind: "done" | "next" | "loop" }
export interface PrRow { key: string; task: MissionTask; open: boolean; cells: PathCell[]; edges: PathEdge[] }

/** #591: blocked work first, then work in flight, then the newest record; ties keep PR order. */
export function focusTask(rows: TaskState[]): string | null {
  const rank = (t: TaskState) => (openBlock(t) !== null ? 0 : t.step !== "merged" ? 1 : 2);
  const best = [...rows].sort(byPr).sort((a, b) => rank(a) - rank(b) || (b.lastSequence ?? 0) - (a.lastSequence ?? 0))[0];
  return best?.key ?? null;
}

/**
 * Which column a task sits in, read only from its own records. Fix: an unanswered BLOCK (the author
 * must fix). Once the fix is pushed the work is back in Review, waiting on the re-review. Proven: merged
 * and the journey replay passed the step the task serves (`proven`); merged alone is Merged.
 */
export function workStage(t: TaskState, proven: boolean): WorkStage {
  if (t.step === "merged") return proven ? "proven" : "merged";
  if (openBlock(t) !== null) return "fix";
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
      rows: orderRows(rows).map((t) => ({ ...prPath(t, proven.has(t.key)), task: toMissionTask(t) })),
    };
  });
  return groups.sort((a, b) => Number(b.open) - Number(a.open) || b.latest - a.latest);
}

const COL: Record<WorkStage, number> = Object.fromEntries(WORK_STAGES.map((s, i) => [s.id, i])) as Record<WorkStage, number>;
const spent = (t: TaskState, ...steps: TimedStep[]) => {
  const ms = steps.reduce((sum, s) => sum + (t.clock?.spent?.[s] ?? 0), 0);
  return ms > 0 ? duration(ms) : null;
};
const counted = (label: string, n: number) => (n > 1 ? `${label} ×${n}` : label);

/**
 * #591: the stages one PR has passed, read only from its own records. Plan: a recorded plan or
 * critic round, else `skipped` once the work moved past it. Review: one round per BLOCK, plus the
 * approving (or pending) review; Fix: one per fix pushed after a BLOCK. Merge: reached once approved;
 * Merged: `mergeSha`; Proven: the linked journey's replay passed the step (`proven`).
 */
export function prPath(raw: TaskState, proven: boolean): Omit<PrRow, "task"> {
  const t = namedTask(raw);
  const current = workStage(t, proven);
  const cur = COL[current];
  const rounds = t.rounds ?? [];
  const fixes = rounds.filter((r) => r.fixHead).length;
  const approved = t.step === "merge" || t.step === "merged";
  const stateAt = (col: number): CellState => (col < cur ? "done" : col === cur ? "current" : "ahead");
  const cells: PathCell[] = [];
  const add = (stage: WorkStage, c: Partial<PathCell>) => {
    const col = COL[stage], state = c.state ?? stateAt(col);
    cells.push({ stage, col, label: WORK_STAGES[col]!.label, who: null, mark: state === "done" ? "✓" : null, time: null, count: 1, title: null, sub: null, ...c, state });
  };
  const planned = Boolean(t.plan || t.critic || t.clock?.spent?.plan || t.clock?.spent?.critic);
  add("plan", { mark: cur > 0 ? (planned ? "✓" : "skipped") : null, time: spent(t, "plan", "critic"), who: t.critic ? `critic ${t.critic.score}/${t.critic.passScore}` : null });
  add("implement", { who: t.lane, time: spent(t, "implement") });
  const block = openBlock(t);
  const reviews = rounds.length + (block ? 0 : approved || t.step === "review" ? 1 : 0);
  const lastRound = rounds[rounds.length - 1];
  const lastReviewer = block?.reviewer || (approved ? t.reviewers[t.reviewers.length - 1] : null) || lastRound?.reviewer || null;
  // #591: the fix answering the last BLOCK is pushed and the re-review has not answered yet.
  const fixPushed = !block && t.step === "review" && Boolean(lastRound?.fixHead);
  const blocker = block ? block.reviewer || "a reviewer" : null;
  const reReviewer = lastRound?.reviewer || t.reviewers[t.reviewers.length - 1] || null;
  if (block) add("review", { state: "block", mark: "BLOCK", who: `by ${blocker}`, count: Math.max(1, reviews), label: counted("Review", reviews) });
  else if (fixPushed) add("review", { who: reReviewer, sub: reReviewer ? `waiting on ${reReviewer}` : null, count: reviews, label: counted("Re-review", rounds.length), title: counted("Re-review", rounds.length), time: spent(t, "review") });
  else add("review", { who: lastReviewer, count: Math.max(1, reviews), label: counted("Review", reviews), time: spent(t, "review") });
  if (block) {
    add("fix", { title: "Fixing", sub: `after BLOCK by ${blocker}`, count: fixes + 1, label: counted("Fix", fixes + 1), who: t.lane });
  } else if (fixes > 0) {
    add("fix", { state: "done", mark: fixPushed ? `fix pushed ${lastRound!.fixHead!.slice(0, 8)}` : "✓", count: fixes, label: counted("Fix", fixes), who: t.lane });
  } else if (cur > COL.fix) {
    // No BLOCK was ever recorded: the PR went straight past Fix; draw nothing there.
  } else add("fix", { state: "ahead" });
  add("merge", { time: spent(t, "merge") });
  add("merged", { who: t.mergeSha ? t.mergeSha.slice(0, 8) : null, mark: t.mergeSha ? "✓" : null });
  add("proven", { mark: proven ? "✓" : null });
  const lit = cells.filter((c) => c.state !== "ahead");
  const edges: PathEdge[] = lit.slice(1).map((b, i) => ({ row: t.key, from: lit[i]!.col, to: b.col, kind: b.state === "current" && !fixPushed ? "next" : "done" }));
  if (fixes > 0) edges.push({ row: t.key, from: COL.fix, to: COL.review, kind: fixPushed ? "next" : "done" });
  return { key: t.key, open: t.step !== "merged", cells, edges };
}

/** #591: open rows first by the `focusTask` rank, then merged rows; ties keep PR order. */
export function orderRows(rows: TaskState[]): TaskState[] {
  const rank = (t: TaskState) => (openBlock(t) !== null ? 0 : t.step !== "merged" ? 1 : 2);
  return [...rows].sort(byPr).sort((a, b) => rank(a) - rank(b) || (b.lastSequence ?? 0) - (a.lastSequence ?? 0));
}
