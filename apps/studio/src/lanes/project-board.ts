import type { BoardLaneLiveness, BoardTask, OwnerNote, RunSummary, TasksBoard } from "../runtime/client";

/** #2228: one execution's board, as the Runtime served it. */
export interface RunBoard {
  executionId: string;
  board: TasksBoard;
}

/** #2228: every execution of a project read as one board. A lane is the address the Runtime keys its
 * records by, so one lane recording in two executions of the same project is one card (identity is
 * project + lane address, never a display name). `origins` names, per lane, the executions it
 * recorded in, newest record first; `runs` lists the executions that were read. */
export interface ProjectBoard extends TasksBoard {
  runs: string[];
  origins: Record<string, string[]>;
}

/** #2228: what a lane's records say about it now. Only record times count, never an execution's
 * status: a paused execution with a recent record is not a stopped agent. */
export type Presence = "working" | "idle" | "stale" | "unknown";

/** Past this many minutes without a record, a lane's last presence is history, not activity. */
export const STALE_MINUTES = 24 * 60;

export function presence(idleMinutes: number | null, holdsSlice: boolean): Presence {
  if (idleMinutes === null) return "unknown";
  if (idleMinutes >= STALE_MINUTES) return "stale";
  return idleMinutes < 30 && holdsSlice ? "working" : "idle";
}

const at = (value: string | null | undefined) => {
  const parsed = Date.parse(value ?? "");
  return Number.isNaN(parsed) ? -Infinity : parsed;
};

/** The executions Lanes reads: the one `?run=` names (a filtered view), else every execution the
 * project's Runtime lists. An id the list does not carry is still read: a deep link never empties. */
export function lanesRuns(runs: readonly RunSummary[] | null, filter: string | null): string[] {
  if (filter !== null) return [filter];
  return runs === null ? [] : runs.map((run) => run.executionId);
}

/** Combine the boards of one project. A slice keeps its execution in `key` (`<execution>/<key>`)
 * so two executions never collide or double count; each lane's liveness is the row of its newest
 * record, owing if any execution says it owes. */
export function combineBoards(parts: readonly RunBoard[]): ProjectBoard {
  const tasks: (BoardTask & { executionId: string })[] = [];
  const stalls: TasksBoard["stalls"] = [];
  const waiting: TasksBoard["waiting"] = [];
  const ownerNotes: OwnerNote[] = [];
  const newest = new Map<string, { row: BoardLaneLiveness; owes: boolean; free: boolean }>();
  const seen = new Map<string, Map<string, number>>();
  const note = (lane: string | null | undefined, executionId: string, when: number) => {
    if (!lane) return;
    const runs = seen.get(lane) ?? new Map<string, number>();
    runs.set(executionId, Math.max(runs.get(executionId) ?? -Infinity, when));
    seen.set(lane, runs);
  };
  let now = "";
  for (const { executionId, board } of parts) {
    if (at(board.now) > at(now)) now = board.now;
    const scoped = (key: string) => `${executionId}/${key}`;
    for (const task of board.tasks) {
      tasks.push({ ...task, key: scoped(task.key), executionId });
      note(task.lane, executionId, at(task.clock?.since));
      note(task.currentReviewer, executionId, at(task.clock?.since));
    }
    for (const stall of board.stalls) stalls.push({ ...stall, key: scoped(stall.key) });
    for (const row of board.waiting) { waiting.push(row); note(row.lane, executionId, -Infinity); }
    ownerNotes.push(...(board.ownerNotes ?? []));
    for (const row of board.liveness.lanes) {
      note(row.lane, executionId, at(row.lastRecordAt));
      const prior = newest.get(row.lane);
      if (prior === undefined) { newest.set(row.lane, { row, owes: row.owes, free: row.free }); continue; }
      const later = at(row.lastRecordAt) > at(prior.row.lastRecordAt) ? row : prior.row;
      newest.set(row.lane, { row: later, owes: prior.owes || row.owes, free: prior.free && row.free });
    }
  }
  const origins: Record<string, string[]> = {};
  for (const [lane, runs] of seen) origins[lane] = [...runs].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([id]) => id);
  return {
    now,
    tasks,
    liveness: { lanes: [...newest.values()].map(({ row, owes, free }) => ({ ...row, owes, free })) },
    stalls,
    waiting,
    ownerNotes,
    runs: parts.map((part) => part.executionId),
    origins,
  };
}

/** The execution a lane's panel reads and writes: the one with its newest record. */
export function homeRun(board: ProjectBoard, lane: string): string | null {
  return board.origins[lane]?.[0] ?? board.runs[0] ?? null;
}
