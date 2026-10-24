import type { OwnerRequest } from "../graph/requests";
import type { BoardTask, Capacity, LaneAction, TasksBoard } from "../runtime/client";
import { OWNER_ADDRESS, finishedLanes } from "./board";

/** What a Needs action item's button does. Answer opens the lane's reply box; Assign and Merge
 * message the lane that should act (the Studio never calls GitHub); Archive, Reassign and Wake are
 * the lane controls of #1847. */
export type NeedVerb = "Answer" | "Assign" | "Merge" | "Archive" | "Reassign" | "Wake" | "Hand off";
/** The design's dot colours: red waits on the owner, orange on another lane, blue a review, green a merge. */
export type NeedTone = "red" | "orange" | "blue" | "green";

export interface Need {
  /** Stable across reads, so a resend reuses its draft. */
  id: string;
  verb: NeedVerb;
  tone: NeedTone;
  what: string;
  /** When the item started waiting; `null` when no record carried a time. */
  since: string | null;
  /** The lane the action is sent to (Answer, Archive, Reassign, Wake) or about (Assign, Merge). */
  lane: string;
  /** Assign: the lane asked to review; Merge: the lane asked to merge; Reassign: the new holder.
   * `null` when no lane can take it, and the button is disabled. */
  to: string | null;
  slice: BoardTask | null;
  /** #1919 Hand off: the owner request no issue was opened for yet. */
  request?: OwnerRequest;
}

const open = (task: BoardTask) => task.step !== "merged" && task.releasedBy === undefined;

/** A live lane that can take work: free, not silent, not finished (#1986: a finished lane is off the
 * board and is nobody's target) and not one of `except`. */
function freeLane(board: TasksBoard, except: (string | null)[]): string | null {
  const done = finishedLanes(board);
  return board.liveness.lanes
    .filter((row) => row.free && !row.silent && !done.has(row.lane) && !except.includes(row.lane))
    .map((row) => row.lane)
    .sort()[0] ?? null;
}

/** Everything on the board that waits on the owner, oldest first (an item with no time last). */
export function needsAction(board: TasksBoard, requests: readonly OwnerRequest[] = []): Need[] {
  const needs: Need[] = [];
  // #1919: an owner request (a Proof "Send to fix" among them) that no issue was opened for yet
  // waits for the owner to hand it to a free lane.
  for (const request of requests) {
    if (request.status !== "open" || request.issues.length > 0) continue;
    const to = freeLane(board, []);
    needs.push({
      id: `handoff:${request.requestId}`, verb: "Hand off", tone: "red",
      what: `Your request waits for a lane: ${request.title ?? request.ask.split("\n")[0]}`,
      since: request.recordedAt ?? null, lane: to ?? "a free lane", to, slice: null, request,
    });
  }
  for (const row of board.waiting) {
    const owner = row.waitsOn === OWNER_ADDRESS;
    needs.push({
      id: `answer:${row.signalId}`, verb: "Answer", tone: owner ? "red" : "orange",
      what: `${row.lane} asks ${owner ? "you" : row.waitsOn}: ${row.question}`,
      since: row.since, lane: row.lane, to: null, slice: null,
    });
  }
  for (const row of board.liveness.lanes.filter((lane) => lane.silent)) {
    const held = board.tasks.filter((task) => open(task) && task.lane === row.lane).sort((a, b) => b.lastSequence - a.lastSequence)[0];
    needs.push(held === undefined
      ? { id: `archive:${row.lane}`, verb: "Archive", tone: "red", what: `${row.lane} silent, holds no task`, since: row.lastRecordAt ?? null, lane: row.lane, to: null, slice: null }
      : { id: `reassign:${held.key}`, verb: "Reassign", tone: "red", what: `${row.lane} silent on #${held.issue ?? held.key}`, since: row.lastRecordAt ?? null, lane: row.lane, to: freeLane(board, [row.lane]), slice: held });
  }
  for (const task of board.tasks.filter(open)) {
    if (task.pr === null || task.lane === null) continue;
    if (task.step === "review" && task.currentReviewer === null && task.blockedBy === null) {
      needs.push({ id: `assign:${task.key}`, verb: "Assign", tone: "blue", what: `PR #${task.pr} has no reviewer started`, since: task.clock?.since ?? null, lane: task.lane, to: freeLane(board, [task.lane]), slice: task });
    } else if (task.step === "merge") {
      needs.push({ id: `merge:${task.key}`, verb: "Merge", tone: "green", what: `PR #${task.pr} approved, ready to merge`, since: task.clock?.since ?? null, lane: task.lane, to: task.currentReviewer ?? task.lane, slice: task });
    }
  }
  for (const stall of board.stalls) {
    if (stall.reason !== "block_unanswered" || !stall.owner) continue;
    needs.push({ id: `wake:${stall.key}`, verb: "Wake", tone: "orange", what: `PR #${stall.pr ?? "?"}: a BLOCK waits on ${stall.owner}`, since: stall.idleSince ?? null, lane: stall.owner, to: null, slice: null });
  }
  const at = (need: Need) => (need.since === null ? NaN : Date.parse(need.since));
  return needs
    .map((need, index) => ({ need, index, time: at(need) }))
    .sort((a, b) => {
      const known = Number(Number.isFinite(b.time)) - Number(Number.isFinite(a.time));
      if (known !== 0) return known;
      return (Number.isFinite(a.time) ? a.time - b.time : 0) || a.index - b.index;
    })
    .map((row) => row.need);
}

export interface FeedRow { lane: string; action: LaneAction }

/** The live feed: every lane's recent actions as one list, newest first, at most `limit` rows. */
export function feed(byLane: Record<string, { rows: LaneAction[] }>, limit: number): FeedRow[] {
  const time = (action: LaneAction) => (action.occurredAt ? Date.parse(action.occurredAt) : NaN);
  return Object.entries(byLane)
    .flatMap(([lane, { rows }]) => rows.map((action) => ({ lane, action })))
    .filter((row) => Number.isFinite(time(row.action)))
    .sort((a, b) => time(b.action) - time(a.action) || b.action.sequence - a.action.sequence)
    .slice(0, limit);
}

export interface CapacityRow {
  name: string;
  value: string;
  /** The bar's fill, 0..1; `null` when there is no number to draw. */
  fill: number | null;
  tone: "ok" | "slow" | "late" | "unknown";
  /** Why a value is unknown, as the Runtime said it. */
  reason: string | null;
}

const GIB = 1024 ** 3;

/** `GET /v1/capacity` as the panel's rows. A value the Runtime does not know stays unknown. */
export function capacityRows(capacity: Capacity): CapacityRow[] {
  const rows: CapacityRow[] = capacity.providers.list.map((provider) => ({
    name: `${provider.provider} quota`,
    value: provider.quotaLeft === null ? "unknown" : `${provider.quotaLeft} left`,
    fill: null, tone: "unknown", reason: provider.quotaLeft === null ? provider.quotaLeftReason ?? null : null,
  }));
  if (capacity.providers.list.length === 0 && capacity.providers.reason) {
    rows.push({ name: "Providers", value: "unknown", fill: null, tone: "unknown", reason: capacity.providers.reason });
  }
  const slots = capacity.buildSlots;
  if (slots.configured > 0) {
    const fill = Math.min(1, slots.busy / slots.configured);
    rows.push({
      name: "Build slots",
      value: `${slots.busy} of ${slots.configured} busy${slots.queued > 0 ? ` · ${slots.queued} queued` : ""}`,
      fill, tone: slots.queued > 0 || fill >= 1 ? "late" : fill > 0 ? "slow" : "ok", reason: slots.unreadable > 0 ? `${slots.unreadable} unreadable` : null,
    });
  } else {
    rows.push({ name: "Build slots", value: "none configured", fill: null, tone: "unknown", reason: null });
  }
  const memory = capacity.memory;
  if (memory.availableBytes !== null && memory.totalBytes !== null && memory.totalBytes > 0) {
    const left = memory.availableBytes / memory.totalBytes;
    rows.push({ name: "Free RAM", value: `${(memory.availableBytes / GIB).toFixed(1)} GiB`, fill: left, tone: left < 0.15 ? "late" : left < 0.3 ? "slow" : "ok", reason: null });
  } else {
    rows.push({ name: "Free RAM", value: "unknown", fill: null, tone: "unknown", reason: memory.reason });
  }
  return rows;
}
