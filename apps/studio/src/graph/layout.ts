import type { BoardTask, JourneyFlow, TasksBoard } from "../runtime/client";
import { minutesInStep } from "../lanes/board";
import { when } from "../time/when";
import { cards, kanbanColumn, KANBAN_NAME, type KanbanColumn, type OwnerRequest, arrangeRequests, requestActivity, requestProgress, type Arranged, type RequestProgress } from "./requests";

/** #1903: the Graph view of the approved design (docs/design/studio/Graph.dc.html), as pure data. */

/** The design's journey colours, in the order the journeys-with-work column lists them. */
const JOURNEY_COLORS = ["#ff8a3d", "#8b7cff", "#3ddc97", "#8ab4ff"] as const;

/** The step squares of an issue, left to right. */
export const STEPS = ["claimed", "planned", "implementing", "tests green", "PR open", "in review", "merged", "proven"] as const;

/** Colours of a column, as the design paints counts, bars and results. */
export const COLUMN_COLOR: Record<KanbanColumn, string> = {
  ready: "#ff6b6b",
  implementing: "#ff8a3d",
  review: "#4ea1ff",
  merged: "#d08cff",
  proven: "#3ddc97",
};

/** Index of the current step square: every square before it is done. A proven slice has all done. */
export function stage(task: BoardTask, column: KanbanColumn): number {
  if (column === "proven") return STEPS.length;
  if (task.step === "merged") return 6;
  if (task.step === "review" || task.step === "merge") return 5;
  if (task.step === "implement") return task.pr === null ? 2 : 4;
  return 1;
}

/** The squares as colours: done green, the current one orange (red when stalled), the rest grey. */
export function blocks(task: BoardTask, column: KanbanColumn): { step: string; color: string }[] {
  const at = stage(task, column);
  return STEPS.map((step, index) => ({
    step,
    color: index < at ? "#3ddc97" : index === at ? (column === "ready" ? "#ff6b6b" : "#ff8a3d") : "#2a2a30",
  }));
}

/** The header counts per column, narrowed to one journey when one is picked. */
export function counts(board: TasksBoard, flows: readonly JourneyFlow[], journey: string | null): Record<KanbanColumn, number> {
  const out: Record<KanbanColumn, number> = { ready: 0, implementing: 0, review: 0, merged: 0, proven: 0 };
  for (const card of cards(board, flows, journey)) out[card.column] += 1;
  return out;
}

export interface JourneyCard { id: string; title: string; color: string; issues: number; prs: number; lanes: string[]; stalled: boolean }

/** The JOURNEYS WITH WORK column: one card per journey a live slice names, in the flows' order. */
export function journeyCards(board: TasksBoard, flows: readonly JourneyFlow[]): JourneyCard[] {
  const live = board.tasks.filter((task) => task.releasedBy === undefined);
  const stalled = new Set(board.stalls.map((stall) => stall.key));
  const ids = [...new Set(live.flatMap((task) => task.journeys))];
  const order = (id: string) => { const at = flows.findIndex((flow) => flow.id === id); return at < 0 ? flows.length : at; };
  ids.sort((a, b) => order(a) - order(b) || a.localeCompare(b));
  return ids.map((id, index) => {
    const mine = live.filter((task) => task.journeys.includes(id));
    const flow = flows.find((row) => row.id === id);
    return {
      id,
      title: flow?.title ?? flow?.name ?? id,
      color: JOURNEY_COLORS[index % JOURNEY_COLORS.length],
      issues: new Set(mine.map((task) => task.issue ?? task.key)).size,
      prs: new Set(mine.filter((task) => task.pr !== null).map((task) => task.pr)).size,
      lanes: [...new Set(mine.map((task) => task.lane).filter((lane): lane is string => lane !== null))],
      stalled: mine.some((task) => stalled.has(task.key)),
    };
  });
}

/** "2 issues · 1 PR" or "1 issue · stalled", as the design writes a journey card's meta line. */
export function journeyMeta(card: JourneyCard): string {
  const issues = `${card.issues} issue${card.issues === 1 ? "" : "s"}`;
  if (card.stalled) return `${issues} · stalled`;
  return `${issues} · ${card.prs} PR${card.prs === 1 ? "" : "s"}`;
}

/** Row height of one issue in the graph (the design's `ROW`). */
export const ROW = 96;

/** The YOU ASKED card at its tallest without a legend: padding, the time line, a title clamped to
 * three lines, the chip and percent, the bar and the "no issue" line (#1947). */
export const ASK_MIN = 190;
/** One legend row of the card, two states a row at the card's 240px (#1947). */
const LEGEND_ROW = 18;

/** The least height a request row may have so its YOU ASKED card never reaches the next row. */
export function askHeight(legendCount: number): number {
  return ASK_MIN + Math.ceil(legendCount / 2) * LEGEND_ROW;
}

/** A request row's height: one ROW per issue, never less than its YOU ASKED card (#1947). */
export function rowHeight(issues: number, legendCount: number): number {
  return Math.max(Math.max(issues, 1) * ROW + 8, askHeight(legendCount) + 8);
}

export interface GraphItem {
  issue: number;
  title: string;
  task: BoardTask | null;
  column: KanbanColumn | null;
  pr: number | null;
  y: number;
  blocks: { step: string; color: string }[];
  lane: string | null;
  last: string;
  journey: JourneyCard | null;
  prLine: string;
  result: { word: string; detail: string; color: string };
}

export interface GraphRequest {
  request: OwnerRequest;
  when: string;
  height: number;
  askY: number;
  items: GraphItem[];
  links: { d: string; color: string; dashed: boolean }[];
  bar: string[];
  summary: string;
  chip: RequestProgress["chip"];
  progress: RequestProgress;
  at: number;
}

/** The Graph order of #1922: working, not started, delivered apart. */
export function arrangeGraph(rows: readonly GraphRequest[], now: number): Arranged<GraphRequest> {
  return arrangeRequests(rows, (row) => ({ key: row.chip.key, at: row.at }), now);
}

/** A time in the one Studio format (#1934), or "" when it is unknown. */
const stamp = (at: string | null | undefined): string => (at ? when(at).text : "").replace("—", "");

/** The live slice that stands for an issue: the newest one not released. */
export function taskOf(board: TasksBoard, issue: number): BoardTask | null {
  const mine = board.tasks.filter((task) => task.issue === issue && task.releasedBy === undefined);
  return mine.sort((a, b) => b.lastSequence - a.lastSequence)[0] ?? null;
}

/** A title for an issue whose slices were all released: the newest one that recorded any. */
function titleOf(board: TasksBoard, issue: number): string | null {
  const titled = board.tasks.filter((task) => task.issue === issue && (task.prTitle ?? task.title) !== null);
  const last = titled.sort((a, b) => b.lastSequence - a.lastSequence)[0];
  return last === undefined ? null : last.prTitle ?? last.title;
}

function outcome(column: KanbanColumn | null, merged: boolean): GraphItem["result"] {
  if (column === "proven") return { word: "Proven", detail: "the journey replay passed", color: COLUMN_COLOR.proven };
  if (column === "merged" || merged) return { word: "Merged · waits proof", detail: "replay will prove it", color: COLUMN_COLOR.merged };
  if (column === "ready") return { word: "Stalled", detail: "owes a record", color: COLUMN_COLOR.ready };
  return { word: "Not merged", detail: "—", color: "#8a8a96" };
}

const done = (item: GraphItem) => item.result.word === "Proven" || item.result.word.startsWith("Merged");

/** Each request laid out as the design draws it: its card, one row per issue, and the lines. With a
 * journey picked, only the issues whose slice names it, and only requests that keep one. */
export function graphRequests(
  requests: readonly OwnerRequest[],
  board: TasksBoard,
  flows: readonly JourneyFlow[],
  journey: string | null,
): GraphRequest[] {
  const stalled = new Set(board.stalls.map((stall) => stall.key));
  const journeyList = journeyCards(board, flows);
  return requests.map((request) => {
    const rows = request.issues.filter((row) => journey === null || (taskOf(board, row.issue)?.journeys.includes(journey) ?? false));
    const items: GraphItem[] = rows.map((row, index) => {
      const task = taskOf(board, row.issue);
      const column = task === null ? null : kanbanColumn(task, stalled, flows);
      const pr = task?.pr ?? row.slices.find((slice) => slice.pr !== null)?.pr ?? null;
      const merged = column === null ? row.slices.some((slice) => slice.merged) : column === "merged" || column === "proven";
      const reviewer = task?.currentReviewer ?? null;
      const prLine = pr === null ? "author still working"
        : column === "merged" || column === "proven" || merged ? "merged"
        : reviewer !== null ? `${reviewer} reviewing` : "waits for a reviewer";
      return {
        issue: row.issue,
        title: task?.prTitle ?? task?.title ?? titleOf(board, row.issue) ?? "title not recorded",
        task,
        column,
        pr,
        y: index * ROW,
        blocks: task === null || column === null
          ? STEPS.map((step, at) => ({ step, color: merged && at < 7 ? "#3ddc97" : "#2a2a30" }))
          : blocks(task, column),
        lane: task?.lane ?? null,
        last: task === null ? (merged ? "merged" : "no slice recorded") : `${stamp(task.clock?.since)}  ${column === null ? task.step : KANBAN_NAME[column].toLowerCase()}`.trim(),
        journey: journeyList.find((card) => task?.journeys.includes(card.id)) ?? null,
        prLine,
        result: outcome(column, merged),
      };
    });
    const progress = requestProgress(request, board.tasks, flows, stalled);
    const height = rowHeight(items.length, progress.legend.length);
    const askY = Math.max(0, (height - askHeight(progress.legend.length)) / 2);
    const cy = askY + 60;
    const links: GraphRequest["links"] = [];
    for (const item of items) {
      const iy = item.y + 42;
      links.push({ d: `M240 ${cy} C 270 ${cy}, 270 ${iy}, 300 ${iy}`, color: "#ff8a3d", dashed: false });
      links.push({ d: `M550 ${iy} L 600 ${iy}`, color: item.pr !== null ? "#4ea1ff" : "#45454e", dashed: item.pr === null });
      links.push({ d: `M780 ${iy} L 840 ${iy}`, color: done(item) ? "#d08cff" : "#45454e", dashed: !done(item) });
    }
    const prCount = items.filter((item) => item.pr !== null).length;
    return {
      request,
      chip: progress.chip,
      progress,
      at: requestActivity(request, board.tasks),
      when: request.recordedAt && stamp(request.recordedAt) !== "" ? stamp(request.recordedAt) : request.recordedBy,
      height,
      askY,
      items,
      links,
      bar: items.map((item) => item.result.color === "#8a8a96" ? COLUMN_COLOR.implementing : item.result.color),
      summary: `${items.length} issue${items.length === 1 ? "" : "s"} · ${prCount} PR${prCount === 1 ? "" : "s"} · ${items.filter(done).length} merged`,
    };
  }).filter((row) => journey === null || row.items.length > 0);
}

/** The NOW and HISTORY lines of the selected issue. */
export function detail(item: GraphItem, request: GraphRequest, board: TasksBoard): { now: string[]; history: string[] } {
  const minutes = item.task === null ? null : minutesInStep(item.task, board.now);
  const age = minutes === null ? "no clock" : minutes < 60 ? `${minutes}m` : `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
  const now = [
    item.column === null ? item.result.word : `${KANBAN_NAME[item.column]} · ${age} in this column`,
    `Author ${item.lane ?? "not recorded"}`,
    item.pr === null ? "No PR yet" : `PR #${item.pr} · ${item.prLine}`,
  ];
  const history = [`asked ${request.when}`];
  const row = request.request.issues.find((entry) => entry.issue === item.issue);
  for (const slice of row?.slices ?? []) history.push(`${slice.pr === null ? "slice" : `PR #${slice.pr}`} · ${slice.merged ? "merged" : slice.step}`);
  if (item.task?.clock?.since) history.push(`${item.task.step} since ${stamp(item.task.clock.since)}`);
  return { now, history };
}
