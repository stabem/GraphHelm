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

/** #1922 (Graph.dc.html renderVals): the state of one issue of a request, stalled to proven. */
export const ISSUE_STATES = [
  { label: "stalled", color: "#ff6b6b" },
  { label: "implementing", color: "#ff8a3d" },
  { label: "in review", color: "#4ea1ff" },
  { label: "merged, waits proof", color: "#d08cff" },
  { label: "proven", color: "#3ddc97" },
] as const;
const NOT_STARTED = { label: "not started", color: "#45454e" };

export type RequestChipKey = "not_started" | "in_progress" | "stalled" | "delivered" | "proven";
export interface RequestChip { key: RequestChipKey; label: string; color: string }
export interface RequestSeg { issue: number; state: string; color: string }
export interface RequestProgress {
  chip: RequestChip;
  /** "33%": share of issues merged. */
  pct: string;
  done: number;
  total: number;
  /** One segment per issue, proven first. */
  segs: RequestSeg[];
  legend: { n: number; label: string; color: string }[];
}

/** The column of each issue of a request (0 stalled, 1 implementing, 2 review, 3 merged, 4 proven);
 * -1 when no slice exists and none merged. Released slices are nobody's work. */
function issueColumns(request: OwnerRequest, tasks: readonly BoardTask[], flows: readonly JourneyFlow[], stalled: ReadonlySet<string>): { issue: number; col: number }[] {
  return request.issues.map((row) => {
    const own = tasks.filter((task) => task.releasedBy === undefined && task.issue === row.issue);
    const cols = own.map((task) => KANBAN.indexOf(kanbanColumn(task, stalled, flows)));
    // KANBAN order is ready, implementing, review, merged, proven: the same 0..4 scale.
    if (cols.length === 0) return { issue: row.issue, col: row.slices.some((slice) => slice.merged) ? 3 : -1 };
    return { issue: row.issue, col: Math.max(...cols) };
  });
}

export function requestProgress(request: OwnerRequest, tasks: readonly BoardTask[], flows: readonly JourneyFlow[], stalled: ReadonlySet<string> = new Set()): RequestProgress {
  const its = issueColumns(request, tasks, flows, stalled);
  const total = its.length;
  const done = its.filter((it) => it.col >= 3).length;
  const segs = [...its].sort((a, b) => b.col - a.col).map((it) => {
    const state = it.col < 0 ? NOT_STARTED : ISSUE_STATES[it.col]!;
    return { issue: it.issue, state: state.label, color: state.color};
  });
  const legend = [4, 3, 2, 1, 0, -1]
    .map((col) => ({ n: its.filter((it) => it.col === col).length, ...(col < 0 ? NOT_STARTED : ISSUE_STATES[col]!) }))
    .filter((g) => g.n > 0);
  let chip: RequestChip;
  if (total === 0) chip = { key: "not_started", label: "Not started", color: "#a2a2ad" };
  else if (done === total) {
    chip = its.every((it) => it.col >= 4) ? { key: "proven", label: "\u2713 Proven", color: "#3ddc97" } : { key: "delivered", label: "Delivered", color: "#4ea1ff" };
  } else if (its.some((it) => it.col === 1 || it.col === 2)) chip = { key: "in_progress", label: `In progress \u00b7 ${done} of ${total}`, color: "#ff8a3d" };
  else if (its.some((it) => it.col === 0)) chip = { key: "stalled", label: "Stalled", color: "#ff6b6b" };
  else chip = done > 0 ? { key: "in_progress", label: `In progress \u00b7 ${done} of ${total}`, color: "#ff8a3d" } : { key: "not_started", label: "Not started", color: "#a2a2ad" };
  return { chip, pct: `${total === 0 ? 0 : Math.round((done / total) * 100)}%`, done, total, segs, legend };
}

export function requestChip(request: OwnerRequest, tasks: readonly BoardTask[], flows: readonly JourneyFlow[], stalled: ReadonlySet<string> = new Set()): RequestChip {
  return requestProgress(request, tasks, flows, stalled).chip;
}

/** The newest sign of life of a request, in ms: the newest step clock on a slice of its issues,
 * else when it was recorded; 0 when neither is served. */
export function requestActivity(request: OwnerRequest, tasks: readonly BoardTask[]): number {
  const linked = new Set(request.issues.map((row) => row.issue));
  const times = tasks
    .filter((task) => task.issue !== null && linked.has(task.issue))
    .map((task) => Date.parse(task.clock?.since ?? ""))
    .concat(Date.parse(request.recordedAt ?? ""))
    .filter((time) => !Number.isNaN(time));
  return times.length === 0 ? 0 : Math.max(...times);
}

export const DAY_MS = 24 * 60 * 60 * 1000;

export interface Arranged<T> {
  /** Active work first (newest activity on top), then not started (newest first). */
  open: T[];
  /** Every delivered or proven request, newest first. */
  delivered: T[];
  /** The delivered ones whose last activity is within 24 h of `now`. */
  recent: T[];
}

/** #1922: the order of the Lanes column and the Graph. `info` gives each row's chip key and activity. */
export function arrangeRequests<T>(rows: readonly T[], info: (row: T) => { key: RequestChipKey; at: number }, now: number): Arranged<T> {
  const byNewest = (a: T, b: T) => info(b).at - info(a).at;
  const group = (keys: RequestChipKey[]) => rows.filter((row) => keys.includes(info(row).key)).sort(byNewest);
  const delivered = group(["delivered", "proven"]);
  return {
    open: [...group(["in_progress", "stalled"]), ...group(["not_started"])],
    delivered,
    recent: delivered.filter((row) => now - info(row).at <= DAY_MS),
  };
}

/** #1922: eight hues that read on the dark card, picked for contrast and for telling apart. */
export const REQUEST_COLORS = ["#4cc9f0", "#f72585", "#ffd166", "#06d6a0", "#b388ff", "#ff9f1c", "#80ed99", "#ef6f6c"] as const;

/** A request's own colour, stable per id (FNV-1a over the id), used only as a discreet stripe. */
export function requestColor(requestId: string): string {
  let hash = 0x811c9dc5;
  for (let at = 0; at < requestId.length; at += 1) hash = Math.imul(hash ^ requestId.charCodeAt(at), 0x01000193) >>> 0;
  return REQUEST_COLORS[hash % REQUEST_COLORS.length]!;
}
