import type { BoardTask, LaneWaiting, TaskStepName, TasksBoard } from "../runtime/client";

/** The board's columns, in the design's order (docs/design/studio/Lanes.dc.html). */
export const COLUMNS = ["backlog", "implement", "review", "blocked", "silent", "merge"] as const;
export type Column = (typeof COLUMNS)[number];

/** The owner's address on a run (apps/cli `OWNER_ADDRESS`): a question to it waits on "you". */
export const OWNER_ADDRESS = "studio-operator";

/** The design's idle colours: grey under 10 minutes, orange from 10, red from 30. */
type IdleTone = "ok" | "slow" | "late";

/** The progress blocks: one per timed step of a slice. */
export const STEPS: readonly TaskStepName[] = ["plan", "critic", "implement", "review", "merge"];
type Block = "done" | "now" | "stuck" | "todo";

export interface Agent {
  lane: string;
  column: Column;
  /** The slice the card shows: the lane's own newest open one, else one it reviews. */
  task: BoardTask | null;
  /** Minutes since the lane's last record, at the Runtime's `now`; `null` when it recorded none. */
  idleMinutes: number | null;
  lastRecordAt: string | null;
  /** #1851: the kind and model the lane reported (e.g. `claude`, `opus`); never guessed from its name. */
  kind: string | null;
  model: string | null;
  /** #1855: minutes the card's slice has spent in its current step, at the Runtime's `now`;
   * `null` when no record carried a time. */
  stepMinutes: number | null;
  questions: LaneWaiting[];
  /** Open questions beyond the rows served (`laneTotal` minus this lane's rows): the "+K more". */
  moreQuestions: number;
}

const open = (task: BoardTask) => task.step !== "merged" && task.releasedBy === undefined;

/** #1979: lanes off the board: finished lanes, i.e. the newest slice they own was recorded `merged`, they hold
 * or review no open slice and wait on no question. Derived from records (no `lane_retired`); a
 * new claim opens a slice and the lane is no longer finished. */
export function finishedLanes(board: TasksBoard): Set<string> {
  const finished = new Set<string>();
  const owners = new Set(board.tasks.map((task) => task.lane).filter((lane): lane is string => !!lane));
  for (const lane of owners) {
    const newest = board.tasks.filter((task) => task.lane === lane).sort((a, b) => b.lastSequence - a.lastSequence)[0];
    const busy = board.tasks.some((task) => open(task) && (task.lane === lane || task.currentReviewer === lane));
    if (newest?.step === "merged" && !busy && !board.waiting.some((row) => row.lane === lane)) finished.add(lane);
  }
  // A lane whose records are only requests/notes (the orchestrator): it never claimed or reviewed a
  // slice, reported no kind or model and asks nothing, so it is no card of the board.
  for (const row of board.liveness.lanes) {
    const touched = board.tasks.some((task) => task.lane === row.lane || task.currentReviewer === row.lane);
    if (!touched && row.agentKind === undefined && row.agentModel === undefined && !board.waiting.some((ask) => ask.lane === row.lane)) finished.add(row.lane);
  }
  return finished;
}

/** The board read as one card per lane, in lane order. A lane is any lane the liveness verdicts,
 * the open slices or the open questions name. */
export function agents(board: TasksBoard): Agent[] {
  const live = new Map(board.liveness.lanes.map((row) => [row.lane, row]));
  const names = new Set<string>(live.keys());
  for (const task of board.tasks) {
    if (!open(task)) continue;
    if (task.lane) names.add(task.lane);
    if (task.currentReviewer) names.add(task.currentReviewer);
  }
  for (const row of board.waiting) names.add(row.lane);
  const now = Date.parse(board.now);
  const done = finishedLanes(board);
  return [...names].filter((lane) => !done.has(lane)).sort().map((lane) => {
    const owned = board.tasks.filter((task) => open(task) && task.lane === lane).sort((a, b) => b.lastSequence - a.lastSequence);
    const reviewing = board.tasks.filter((task) => open(task) && task.currentReviewer === lane);
    const task = owned[0] ?? reviewing[0] ?? null;
    const questions = board.waiting.filter((row) => row.lane === lane);
    const total = questions.reduce((most, row) => Math.max(most, row.laneTotal), 0);
    const liveness = live.get(lane);
    const lastRecordAt = liveness?.lastRecordAt ?? null;
    const since = lastRecordAt === null ? NaN : Date.parse(lastRecordAt);
    return {
      lane,
      column: column({
        silent: liveness?.silent ?? false,
        asks: questions.length > 0,
        blocked: owned.some((slice) => slice.blockedBy !== null) || board.stalls.some((stall) => stall.owner === lane),
        step: owned[0]?.step ?? (reviewing.length > 0 ? "review" : null),
      }),
      task,
      idleMinutes: Number.isFinite(since) && Number.isFinite(now) ? Math.max(0, Math.floor((now - since) / 60_000)) : null,
      lastRecordAt,
      kind: liveness?.agentKind ?? null,
      model: liveness?.agentModel ?? null,
      stepMinutes: minutesInStep(task, board.now),
      questions,
      moreQuestions: Math.max(0, total - questions.length),
    };
  });
}

/** Which column a lane sits in. A silent lane is Silent whatever it holds; a lane with an open
 * question, a BLOCK on its PR or a stalled PR it owes is Blocked; otherwise its slice's step
 * places it, and a lane holding no slice is in Backlog. */
export function column(lane: { silent: boolean; asks: boolean; blocked: boolean; step: TaskStepName | null }): Column {
  if (lane.silent) return "silent";
  if (lane.asks || lane.blocked) return "blocked";
  switch (lane.step) {
    case "plan": case "critic": case "implement": return "implement";
    case "review": return "review";
    case "merge": return "merge";
    default: return "backlog";
  }
}

export function idleTone(minutes: number | null): IdleTone {
  if (minutes === null || minutes >= 30) return "late";
  return minutes >= 10 ? "slow" : "ok";
}

/** #1855: whole minutes `task` has spent in its current step at the Runtime's `now`, from the
 * record that moved it there (`clock.since`); `null` when no record carried a time. */
export function minutesInStep(task: BoardTask | null, now: string): number | null {
  const entered = task?.clock?.since ? Date.parse(task.clock.since) : NaN;
  const at = Date.parse(now);
  return Number.isFinite(entered) && Number.isFinite(at) ? Math.max(0, Math.floor((at - entered) / 60_000)) : null;
}

/** #1855: a card turns red once its slice has sat an hour or more in its current step. */
export const AGED_MINUTES = 60;
export function stepAged(minutes: number | null): boolean {
  return minutes !== null && minutes >= AGED_MINUTES;
}

export function idleText(minutes: number | null): string {
  if (minutes === null) return "no record";
  if (minutes < 60) return `${minutes}m`;
  if (minutes < 24 * 60) return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
  return `${Math.floor(minutes / (24 * 60))}d`;
}

/** One block per step: the steps before the slice's are done, its own is now (stuck when the
 * lane is Blocked), the rest are to do. A lane with no slice has every block to do. */
export function blocks(step: TaskStepName | null, stuck: boolean): Block[] {
  const at = step === null ? -1 : step === "merged" ? STEPS.length : STEPS.indexOf(step);
  return STEPS.map((_, index) => (at < 0 || index > at ? "todo" : index < at ? "done" : stuck ? "stuck" : "now"));
}
