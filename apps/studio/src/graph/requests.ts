import type { BoardTask, JourneyFlow, TasksBoard } from "../runtime/client";
import { minutesInStep } from "../lanes/board";

/** #1801: one owner request as `GET /v1/executions/{id}/requests` serves it (#1792 R1). */
export interface OwnerRequest {
  requestId: string;
  ask: string;
  title: string | null;
  recordedBy: string;
  sequence: number;
  /** #1901: when the Runtime appended the request's first record; absent from an older Runtime. */
  recordedAt?: string;
  status: "open" | "in_progress" | "delivered";
  issues: { issue: number; slices: { pr: number | null; step: string; merged: boolean }[] }[];
}

/** The Kanban columns of the design, left to right. "Merged" is not done: a card is Proven only
 * when every journey its slice names passed its last run. */
export const KANBAN = ["ready", "implementing", "review", "merged", "proven"] as const;
export type KanbanColumn = (typeof KANBAN)[number];
export const KANBAN_NAME: Record<KanbanColumn, string> = {
  ready: "Ready / stalled",
  implementing: "Implementing",
  review: "In review",
  merged: "Merged, waits proof",
  proven: "Proven",
};

/** Whether every journey a slice names passed its last run. A slice naming none, or a journey with
 * no recorded run (an older Runtime, or never run), is not proven: absence is not a pass. */
export function proven(task: Pick<BoardTask, "journeys">, flows: readonly JourneyFlow[]): boolean {
  if (task.journeys.length === 0) return false;
  return task.journeys.every((id) => flows.find((flow) => flow.id === id)?.lastRun?.status === "passed");
}

/** The column a slice sits in: a stall it owes first, then its step; a merged slice is Proven only
 * when its journeys passed. */
export function kanbanColumn(task: BoardTask, stalled: ReadonlySet<string>, flows: readonly JourneyFlow[]): KanbanColumn {
  if (task.step === "merged") return proven(task, flows) ? "proven" : "merged";
  if (stalled.has(task.key)) return "ready";
  if (task.step === "review" || task.step === "merge") return "review";
  return "implementing";
}

/** One Kanban card. */
/** `stepMinutes`: #1855, minutes in the current step at the board's `now` (`null` when untimed). */
export interface Card { task: BoardTask; column: KanbanColumn; stepMinutes: number | null }

/** The board's slices as cards, narrowed to one journey when `journey` is set. A released slice is
 * nobody's work any more and is left out. */
export function cards(board: TasksBoard, flows: readonly JourneyFlow[], journey: string | null): Card[] {
  const stalled = new Set(board.stalls.map((stall) => stall.key));
  return board.tasks
    .filter((task) => task.releasedBy === undefined)
    .filter((task) => journey === null || task.journeys.includes(journey))
    .map((task) => ({ task, column: kanbanColumn(task, stalled, flows), stepMinutes: minutesInStep(task, board.now) }));
}

/** The journeys that have work in flight, for the filter: the ones some live slice names, each with
 * its count of slices, in the order the flows list them. */
export function journeysWithWork(board: TasksBoard, flows: readonly JourneyFlow[]): { id: string; title: string; slices: number }[] {
  const counts = new Map<string, number>();
  for (const task of board.tasks) {
    if (task.releasedBy !== undefined) continue;
    for (const id of task.journeys) counts.set(id, (counts.get(id) ?? 0) + 1);
  }
  const known = flows.filter((flow) => counts.has(flow.id)).map((flow) => ({ id: flow.id, title: flow.title ?? flow.name ?? flow.id, slices: counts.get(flow.id)! }));
  const unknown = [...counts.keys()].filter((id) => !flows.some((flow) => flow.id === id)).sort().map((id) => ({ id, title: id, slices: counts.get(id)! }));
  return [...known, ...unknown];
}

/** The requests to draw: all of them, or with a journey picked, only the ones with an issue whose
 * slice names that journey. */
export function requestsFor(requests: readonly OwnerRequest[], board: TasksBoard, journey: string | null): OwnerRequest[] {
  if (journey === null) return [...requests];
  const issues = new Set(board.tasks.filter((task) => task.journeys.includes(journey) && task.issue !== null).map((task) => task.issue));
  return requests.filter((request) => request.issues.some((row) => issues.has(row.issue)));
}

/** What a request's result column says. Delivered means every linked issue merged; it reads Proven
 * only when the merged slices' journeys passed as well. */
export function result(request: OwnerRequest, board: TasksBoard, flows: readonly JourneyFlow[]): { word: string; detail: string } {
  if (request.status === "open") return { word: "open", detail: "no issue opened yet" };
  const linked = new Set(request.issues.map((row) => row.issue));
  const merged = board.tasks.filter((task) => task.issue !== null && linked.has(task.issue) && task.step === "merged");
  const done = request.issues.filter((row) => row.slices.some((slice) => slice.merged)).length;
  if (request.status === "in_progress") return { word: "in progress", detail: `${done} of ${request.issues.length} issues merged` };
  if (merged.length > 0 && merged.every((task) => proven(task, flows))) return { word: "proven", detail: "every journey passed" };
  return { word: "merged", detail: "waits for the journey replay" };
}
