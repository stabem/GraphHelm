import { describe, expect, it } from "vitest";

import type { BoardTask, Capacity, LaneAction, TasksBoard } from "../runtime/client";
import { capacityRows, feed, needsAction } from "./panels";

const task = (key: string, lane: string, step: BoardTask["step"], extra: Partial<BoardTask> = {}): BoardTask => ({
  key, issue: 7, pr: null, lane, currentReviewer: null, step, blockedBy: null, title: "Work", prTitle: null, journeys: [], lastSequence: 1, ...extra,
});
const live = (lane: string, extra: Partial<TasksBoard["liveness"]["lanes"][number]> = {}) => ({ lane, owes: false, silent: false, free: false, ...extra });

describe("lanes panels", () => {
  it("#1919: an open request no issue was opened for waits to be handed to a free lane", () => {
    const board: TasksBoard = { now: "2026-10-23T12:00:00Z", tasks: [task("f#1", "lane-free", "implement", { releasedBy: "orch" })], liveness: { lanes: [live("lane-free", { free: true })] }, stalls: [], waiting: [] };
    const request = (requestId: string, status: "open" | "in_progress", issues: number[]) => ({
      requestId, ask: "Fix the journey buy", title: `Fix ${requestId}`, recordedBy: "studio-operator", sequence: 1,
      recordedAt: "2026-10-23T11:00:00Z", status, issues: issues.map((issue) => ({ issue, slices: [] })),
    });
    const needs = needsAction(board, [request("fix-a", "open", []), request("fix-b", "open", [9]), request("fix-c", "in_progress", [])]);
    expect(needs.map((need) => [need.verb, need.lane, need.to, need.what, need.request?.requestId])).toEqual([
      ["Hand off", "lane-free", "lane-free", "Your request waits for a lane: Fix fix-a", "fix-a"],
    ]);
  });

  it("lists what waits on the owner oldest first, each with the verb and the lane it acts on", () => {
    const board: TasksBoard = {
      now: "2026-10-22T12:00:00Z",
      tasks: [
        task("f#1", "lane-free", "implement", { releasedBy: "orch" }),
        task("s#1", "lane-s", "implement", { lastSequence: 4 }),
        task("p#1", "lane-p", "review", { pr: 11, clock: { since: "2026-10-22T11:48:00Z" } }),
        task("m#1", "lane-m", "merge", { pr: 12, currentReviewer: "lane-r", clock: { since: "2026-10-22T11:50:00Z" } }),
        task("b#1", "lane-b", "review", { pr: 13, blockedBy: { reviewer: "lane-r" } }),
      ],
      liveness: { lanes: [
        live("lane-gone", { silent: true, lastRecordAt: "2026-10-20T09:00:00Z" }),
        live("lane-s", { silent: true, lastRecordAt: "2026-10-22T09:48:00Z" }),
        live("lane-free", { free: true }),
        live("lane-asleep", { free: true, silent: true, lastRecordAt: "2026-10-22T11:59:00Z" }),
      ] },
      stalls: [{ key: "b#1", owner: "lane-b", pr: 13, reason: "block_unanswered", idleSince: "2026-10-22T10:00:00Z" }],
      waiting: [{ lane: "lane-q", question: "Before or after?", waitsOn: "studio-operator", since: "2026-10-22T11:22:00Z", signalId: "s1", sequence: 3, laneTotal: 1 }],
    };
    expect(needsAction(board).map((need) => [need.verb, need.lane, need.to, need.since])).toEqual([
      ["Archive", "lane-gone", null, "2026-10-20T09:00:00Z"],
      ["Reassign", "lane-s", "lane-free", "2026-10-22T09:48:00Z"],
      ["Wake", "lane-b", null, "2026-10-22T10:00:00Z"],
      ["Answer", "lane-q", null, "2026-10-22T11:22:00Z"],
      ["Assign", "lane-p", "lane-free", "2026-10-22T11:48:00Z"],
      ["Merge", "lane-m", "lane-r", "2026-10-22T11:50:00Z"],
      ["Archive", "lane-asleep", null, "2026-10-22T11:59:00Z"],
    ]);
  });

  it("leaves Assign and Reassign without a target when no free live lane can take them, and an item with no time last", () => {
    const board: TasksBoard = {
      now: "2026-10-22T12:00:00Z",
      tasks: [task("s#1", "lane-s", "implement"), task("p#1", "lane-p", "review", { pr: 11, clock: { since: null } })],
      liveness: { lanes: [live("lane-s", { silent: true, free: true, lastRecordAt: "2026-10-22T09:00:00Z" }), live("lane-p", { free: true })] },
      stalls: [],
      waiting: [],
    };
    expect(needsAction(board).map((need) => [need.verb, need.to])).toEqual([["Reassign", "lane-p"], ["Assign", null]]);
  });

  it("never names a finished lane as the target of a Reassign (#1986)", () => {
    const board: TasksBoard = {
      now: "2026-10-22T12:00:00Z",
      tasks: [task("s#1", "lane-s", "implement"), task("d#1", "lane-done", "merged", { lastSequence: 9 })],
      liveness: { lanes: [live("lane-s", { silent: true, lastRecordAt: "2026-10-22T09:00:00Z" }), live("lane-done", { free: true })] },
      stalls: [],
      waiting: [],
    };
    expect(needsAction(board).map((need) => [need.verb, need.to])).toEqual([["Reassign", null]]);
  });

  it("never picks a heartbeating zero-slice lane (the orchestrator) as a target (#1986)", () => {
    const board: TasksBoard = {
      now: "2026-10-22T12:00:00Z",
      tasks: [task("s#1", "lane-s", "implement")],
      liveness: { lanes: [live("lane-s", { silent: true, lastRecordAt: "2026-10-22T09:00:00Z" }), live("a-orch", { free: true, lastRecordAt: "2026-10-22T11:59:00Z" })] },
      stalls: [],
      waiting: [],
    };
    expect(needsAction(board).map((need) => [need.verb, need.to])).toEqual([["Reassign", null]]);
  });

  it("merges every lane's actions into one feed, newest first, bounded", () => {
    const act = (sequence: number, occurredAt: string | null): LaneAction => ({ kind: "task.heartbeat", sequence, occurredAt });
    const rows = feed({
      a: { rows: [act(5, "2026-10-22T10:05:00Z"), act(2, "2026-10-22T10:01:00Z")] },
      b: { rows: [act(4, "2026-10-22T10:03:00Z"), act(3, null)] },
    }, 2);
    expect(rows.map((row) => [row.lane, row.action.sequence])).toEqual([["a", 5], ["b", 4]]);
  });

  it("keeps an unknown quota unknown and reads slots and memory as served", () => {
    const capacity: Capacity = {
      providers: { configured: true, list: [{ provider: "claude", routes: 2, enabledRoutes: 1, quotaLeft: null, quotaLeftReason: "no provider publishes the quota left" }] },
      buildSlots: { configured: 2, unreadable: 0, busy: 2, queued: 1 },
      memory: { availableBytes: 7.5 * 1024 ** 3, totalBytes: 30 * 1024 ** 3, reason: null },
    };
    expect(capacityRows(capacity).map((row) => [row.name, row.value, row.fill, row.tone, row.reason])).toEqual([
      ["claude quota", "unknown", null, "unknown", "no provider publishes the quota left"],
      ["Build slots", "2 of 2 busy · 1 queued", 1, "late", null],
      ["Free RAM", "7.5 GiB", 0.25, "slow", null],
    ]);
    expect(capacityRows({ ...capacity, memory: { availableBytes: null, totalBytes: null, reason: "unsupported platform" } }).at(-1))
      .toMatchObject({ value: "unknown", fill: null, reason: "unsupported platform" });
  });

  it("#1991: a silent author whose PR waits for review is not flagged; one without a PR still is", () => {
    const board: TasksBoard = {
      now: "2026-10-22T12:00:00Z",
      tasks: [task("w#1", "lane-w", "review", { pr: 20, currentReviewer: "lane-r", lastSequence: 3 }), task("s#1", "lane-s", "implement", { lastSequence: 4 })],
      liveness: { lanes: [live("lane-w", { silent: true, lastRecordAt: "2026-10-22T11:14:00Z" }), live("lane-s", { silent: true, lastRecordAt: "2026-10-22T09:00:00Z" })] },
      stalls: [], waiting: [],
    };
    expect(needsAction(board).map((need) => need.id)).toEqual(["reassign:s#1"]);
  });
});
