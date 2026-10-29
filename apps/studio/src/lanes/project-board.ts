import type { BoardLaneLiveness, BoardTask, OwnerNote, RunSummary, TasksBoard } from "../runtime/client";
import type { OwnerRequest } from "../graph/requests";

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
  /** Lanes that declared presence (`agent_presence_declared`) in one of the runs. */
  declared?: string[];
  /** Per lane, per run, the time of its newest record there (epoch ms; -Infinity when none). */
  recordedAt: Record<string, Record<string, number>>;
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
  const recordedAt: Record<string, Record<string, number>> = {};
  for (const [lane, runs] of seen) { origins[lane] = newestFirst(runs); recordedAt[lane] = Object.fromEntries(runs); }
  return {
    now,
    tasks,
    liveness: { lanes: [...newest.values()].map(({ row, owes, free }) => ({ ...row, owes, free })) },
    stalls,
    waiting,
    ownerNotes,
    runs: parts.map((part) => part.executionId),
    origins,
    recordedAt,
  };
}

const newestFirst = (runs: Map<string, number>) => [...runs].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([id]) => id);

/** The execution a lane's panel reads and writes: the one with its newest record. */
export function homeRun(board: ProjectBoard, lane: string): string | null {
  return board.origins[lane]?.[0] ?? board.runs[0] ?? null;
}

/** #2228: one envelope of `GET /v1/executions/{id}/events`, only the fields the presence fold reads. */
export interface RunEvent {
  sequence: number;
  occurredAt?: string;
  actor?: { id?: string };
  scope?: { projectId?: string; executionId?: string };
  kind?: { type?: string; data?: { actorId?: string } };
}

/** #2228: who declared presence in the project's runs and when each one last recorded anything. */
export interface PresenceIndex {
  projectId: string | null;
  /** Per run, the last sequence folded, so the next read asks only for what is newer. */
  cursor: Record<string, number>;
  lanes: Record<string, { declared: boolean; lastAt: string | null; runs: Record<string, string | null> }>;
}

export const emptyPresence = (): PresenceIndex => ({ projectId: null, cursor: {}, lanes: {} });

/** Fold a page of one run's events. A lane joins the index only through `agent_presence_declared`;
 * its other records (signals among them) move its last-record time but never declare it, so an
 * orchestrator that only sends signals stays off the board. An envelope of another run or another
 * project is ignored, never mixed in. */
export function foldPresence(index: PresenceIndex, executionId: string, events: readonly RunEvent[]): PresenceIndex {
  const next: PresenceIndex = { projectId: index.projectId, cursor: { ...index.cursor }, lanes: { ...index.lanes } };
  for (const event of events) {
    next.cursor[executionId] = Math.max(next.cursor[executionId] ?? 0, event.sequence);
    if (event.scope?.executionId !== undefined && event.scope.executionId !== executionId) continue;
    const project = event.scope?.projectId ?? null;
    if (project !== null) {
      if (next.projectId === null) next.projectId = project;
      else if (project !== next.projectId) continue;
    }
    const declares = event.kind?.type === "agent_presence_declared";
    const lane = declares ? (event.kind?.data?.actorId ?? event.actor?.id) : event.actor?.id;
    if (!lane) continue;
    const prior = next.lanes[lane];
    if (prior === undefined && !declares) {
      // Kept undeclared: a later declaration in the same project brings the earlier records with it.
      next.lanes[lane] = { declared: false, lastAt: event.occurredAt ?? null, runs: { [executionId]: event.occurredAt ?? null } };
      continue;
    }
    const when = event.occurredAt ?? null;
    const later = (a: string | null, b: string | null) => (at(b) > at(a) ? b : a);
    next.lanes[lane] = {
      declared: (prior?.declared ?? false) || declares,
      lastAt: later(prior?.lastAt ?? null, when),
      runs: { ...(prior?.runs ?? {}), [executionId]: later(prior?.runs[executionId] ?? null, when) },
    };
  }
  return next;
}

/** Add the lanes that declared presence to a project board: a declared lane the task view does not
 * carry gets a liveness row from its newest record (it owes nothing and is judged neither silent
 * nor free), its runs join its origins, and `declared` keeps it on the board (board.ts
 * `finishedLanes`). */
export function withPresence(board: ProjectBoard, index: PresenceIndex): ProjectBoard {
  const declared = Object.entries(index.lanes).filter(([, row]) => row.declared);
  if (declared.length === 0) return board;
  const lanes = [...board.liveness.lanes];
  const origins = { ...board.origins };
  const recordedAt = { ...board.recordedAt };
  for (const [lane, row] of declared) {
    const known = lanes.findIndex((live) => live.lane === lane);
    if (known === -1) lanes.push({ lane, ...(row.lastAt === null ? {} : { lastRecordAt: row.lastAt }), owes: false, silent: false, free: false });
    else if (at(row.lastAt) > at(lanes[known].lastRecordAt)) lanes[known] = { ...lanes[known], lastRecordAt: row.lastAt ?? undefined, silent: false };
    // One unit: record time per run, from the task view and the events alike.
    const runs = new Map<string, number>(Object.entries(recordedAt[lane] ?? {}));
    for (const [id, when] of Object.entries(row.runs)) runs.set(id, Math.max(runs.get(id) ?? -Infinity, at(when)));
    origins[lane] = newestFirst(runs);
    recordedAt[lane] = Object.fromEntries(runs);
  }
  return { ...board, liveness: { lanes }, origins, recordedAt, declared: declared.map(([lane]) => lane) };
}

/** #2230: one execution's owner requests, as `GET /v1/executions/{id}/requests` served them. */
export interface RunRequests {
  executionId: string;
  requests: readonly OwnerRequest[];
}

/** #2230: an owner request read across the project's executions, with the executions it was read in. */
export type ProjectRequest = OwnerRequest & { runs: string[] };

const STATUS_RANK: Record<OwnerRequest["status"], number> = { open: 0, in_progress: 1, delivered: 2 };

/** #2230: the requests of every execution of one project as one list, oldest first. The Runtime keys
 * a request by its id and only admits a link in the execution that recorded the request, so the same
 * id read in two executions is one ask: it is listed once, its issues are the union (each issue
 * once, its slices once per PR and step), it is dated by its earliest record and `runs` names every
 * execution it was read in. Nothing is invented: a request no execution served is not listed. */
export function combineRequests(parts: readonly RunRequests[]): ProjectRequest[] {
  const byId = new Map<string, ProjectRequest>();
  for (const { executionId, requests } of parts) {
    for (const request of requests) {
      const prior = byId.get(request.requestId);
      if (prior === undefined) {
        byId.set(request.requestId, { ...request, issues: request.issues.map((row) => ({ ...row, slices: [...row.slices] })), runs: [executionId] });
        continue;
      }
      if (!prior.runs.includes(executionId)) prior.runs.push(executionId);
      if (at(request.recordedAt) < at(prior.recordedAt)) { prior.recordedAt = request.recordedAt; prior.ask = request.ask; prior.title = request.title; }
      for (const row of request.issues) {
        const same = prior.issues.find((issue) => issue.issue === row.issue);
        if (same === undefined) { prior.issues.push({ ...row, slices: [...row.slices] }); continue; }
        for (const slice of row.slices) {
          if (!same.slices.some((own) => own.pr === slice.pr && own.step === slice.step && own.merged === slice.merged)) same.slices.push(slice);
        }
      }
      // Status follows the furthest execution only when every issue is known there; otherwise the
      // union is in progress once any execution moved it.
      if (STATUS_RANK[request.status] > STATUS_RANK[prior.status]) prior.status = request.status === "delivered" && prior.issues.length > request.issues.length ? "in_progress" : request.status;
      prior.dispatchedTo ??= request.dispatchedTo;
      if (request.dispatchLapsed) prior.dispatchLapsed = [...new Set([...(prior.dispatchLapsed ?? []), ...request.dispatchLapsed])];
    }
  }
  return [...byId.values()].sort((a, b) => at(a.recordedAt) - at(b.recordedAt) || a.sequence - b.sequence || a.requestId.localeCompare(b.requestId));
}

/** #2230: the execution a slice of a combined board belongs to, else the one execution read. */
export function taskRun(task: BoardTask & { executionId?: string }, runs: readonly string[]): string | null {
  return task.executionId ?? runs[0] ?? null;
}
