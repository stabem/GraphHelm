import { describe, expect, it } from "vitest";

import type { BoardTask, LaneWaiting, TasksBoard } from "../runtime/client";
import { agents, finishedLanes, blocks, column, idleText, idleTone, minutesInStep, stepAged } from "./board";

const task = (key: string, lane: string, step: BoardTask["step"], extra: Partial<BoardTask> = {}): BoardTask => ({
  key, issue: 7, pr: null, lane, currentReviewer: null, step, blockedBy: null, title: "Work", prTitle: null, journeys: [], lastSequence: 1, ...extra,
});
const ask = (lane: string, signalId: string, laneTotal: number): LaneWaiting => ({
  lane, question: "Merge or wait?", waitsOn: "studio-operator", since: "2026-10-22T10:00:00Z", signalId, sequence: 1, laneTotal,
});

describe("lanes board", () => {
  it("places each lane by silence first, then a question or block, then its slice's step", () => {
    expect(column({ silent: true, asks: true, blocked: true, step: "review" })).toBe("silent");
    expect(column({ silent: false, asks: true, blocked: false, step: "implement" })).toBe("blocked");
    expect(column({ silent: false, asks: false, blocked: true, step: "review" })).toBe("blocked");
    expect(["plan", "critic", "implement", "review", "merge", "merged", null].map((step) =>
      column({ silent: false, asks: false, blocked: false, step: step as BoardTask["step"] | null }))).toEqual(
      ["implement", "implement", "implement", "review", "merge", "backlog", "backlog"]);
  });

  it("colours idle time grey under 10 minutes, orange from 10 and red from 30", () => {
    expect([0, 9, 10, 29, 30, null].map(idleTone)).toEqual(["ok", "ok", "slow", "slow", "late", "late"]);
    expect([4, 59, 132, 2880].map(idleText)).toEqual(["4m", "59m", "2h 12m", "2d"]);
  });

  it("marks the steps before the slice's done and its own stuck when blocked", () => {
    expect(blocks("review", false)).toEqual(["done", "done", "done", "now", "todo"]);
    expect(blocks("implement", true)).toEqual(["done", "done", "stuck", "todo", "todo"]);
    expect(blocks(null, false)).toEqual(["todo", "todo", "todo", "todo", "todo"]);
  });

  it("builds one card per lane from the open board, with idle at the Runtime's now and the questions beyond the rows", () => {
    const board: TasksBoard = {
      now: "2026-10-22T10:45:00Z",
      tasks: [
        task("a#1", "lane-a", "implement", { lastSequence: 3 }),
        task("a#0", "lane-a", "merged", { lastSequence: 9 }),
        task("b#1", "lane-b", "review", { pr: 9, currentReviewer: "lane-r" }),
        task("c#1", "lane-c", "implement", { releasedBy: "lane-c" }),
      ],
      liveness: { lanes: [
        { lane: "lane-a", lastRecordAt: "2026-10-22T10:31:00Z", owes: true, silent: false, free: false },
        { lane: "lane-b", lastRecordAt: "2026-10-22T09:00:00Z", owes: true, silent: true, free: false },
      ] },
      stalls: [],
      waiting: [ask("lane-q", "s1", 7), ask("lane-q", "s2", 7)],
    };
    expect(agents(board).map((agent) => [agent.lane, agent.column, agent.task?.key ?? null, agent.idleMinutes, agent.moreQuestions])).toEqual([
      ["lane-a", "implement", "a#1", 14, 0],
      ["lane-b", "review", "b#1", 105, 0], // #1991: open PR under review is waiting, not silent,
      ["lane-q", "blocked", null, null, 5],
      ["lane-r", "review", "b#1", null, 0],
    ]);
  });

  it("turns a card red exactly at 60 minutes in its step, not a second before (#1886)", () => {
    // A fixed Runtime `now` against a fixed `clock.since`; no timers.
    const since = "2026-10-22T11:00:00Z";
    const slice = { key: "a#1", issue: 1, pr: null, lane: "lane-a", currentReviewer: null, step: "review", blockedBy: null,
      title: "t", prTitle: null, journeys: [], lastSequence: 1, clock: { since } } as BoardTask;
    const at = (now: string) => { const minutes = minutesInStep(slice, now); return [minutes, stepAged(minutes)]; };
    expect(at("2026-10-22T11:59:59Z")).toEqual([59, false]);
    expect(at("2026-10-22T12:00:00Z")).toEqual([60, true]);
    expect(at("2026-10-22T12:00:01Z")).toEqual([60, true]);
  });

  it("ages each card from the record that moved its slice into its step, red from one hour", () => {
    const board: TasksBoard = {
      now: "2026-10-22T12:00:00Z",
      tasks: [
        task("a#1", "lane-a", "review", { clock: { since: "2026-10-22T10:00:00Z" } }),
        task("b#1", "lane-b", "implement", { clock: { since: "2026-10-22T11:01:00Z" } }),
        task("c#1", "lane-c", "implement", { clock: { since: null } }),
      ],
      liveness: { lanes: [] },
      stalls: [],
      waiting: [],
    };
    expect(agents(board).map((agent) => [agent.lane, agent.stepMinutes, stepAged(agent.stepMinutes)])).toEqual([
      ["lane-a", 120, true],
      ["lane-b", 59, false],
      ["lane-c", null, false],
    ]);
  });

  it("carries each lane's kind and model from its liveness row, and none when it never said (#1851)", () => {
    const board: TasksBoard = {
      now: "2026-10-22T12:00:00Z",
      tasks: [task("a#1", "lane-a", "implement"), task("b#1", "lane-b", "implement")],
      liveness: { lanes: [
        { lane: "lane-a", agentKind: "claude", agentModel: "opus", owes: false, silent: false, free: true },
        { lane: "lane-b", owes: false, silent: false, free: true },
      ] },
      stalls: [],
      waiting: [],
    };
    expect(agents(board).map((agent) => [agent.lane, agent.kind, agent.model])).toEqual([["lane-a", "claude", "opus"], ["lane-b", null, null]]);
  });

  it("drops a lane whose newest slice merged and holds nothing open; a new claim brings it back (#1979)", () => {
    const board: TasksBoard = {
      now: "2026-10-22T12:00:00Z",
      tasks: [
        task("a#1", "lane-done", "merged", { lastSequence: 5 }),
        task("b#1", "lane-again", "merged", { lastSequence: 4 }),
        task("b#2", "lane-again", "implement", { lastSequence: 8 }),
      ],
      liveness: { lanes: [
        { lane: "lane-done", lastRecordAt: "2026-10-22T09:00:00Z", owes: false, silent: true, free: true },
        { lane: "lane-again", lastRecordAt: "2026-10-22T11:59:00Z", owes: true, silent: false, free: false },
      ] },
      stalls: [],
      waiting: [],
    };
    board.liveness.lanes.push({ lane: "orch", lastRecordAt: "2026-10-22T09:00:00Z", owes: false, silent: false, free: true });
    expect([...finishedLanes(board)].sort()).toEqual(["lane-done", "orch"]);
    expect(agents(board).map((agent) => [agent.lane, agent.column])).toEqual([["lane-again", "implement"]]);
  });

  it("keeps a request-only lane off the board even when it reported a kind and model; its request records still count (#1986)", () => {
    const board: TasksBoard = {
      now: "2026-10-22T12:00:00Z",
      tasks: [task("b#1", "lane-b", "implement")],
      liveness: { lanes: [
        { lane: "orch", lastRecordAt: "2026-10-22T11:00:00Z", agentKind: "claude", agentModel: "opus", owes: false, silent: false, free: true },
        { lane: "lane-b", lastRecordAt: "2026-10-22T11:59:00Z", owes: true, silent: false, free: false },
      ] },
      stalls: [],
      waiting: [],
    };
    expect([...finishedLanes(board)]).toEqual(["orch"]);
    expect(agents(board).map((agent) => agent.lane)).toEqual(["lane-b"]);
  });

  it("moves a silent lane to implement once a heartbeat makes it live again (#1986)", () => {
    const at = (silent: boolean): TasksBoard => ({
      now: "2026-10-22T12:00:00Z",
      tasks: [task("a#1", "lane-a", "implement")],
      liveness: { lanes: [{ lane: "lane-a", lastRecordAt: silent ? "2026-10-22T09:00:00Z" : "2026-10-22T11:59:00Z", owes: true, silent, free: false }] },
      stalls: [],
      waiting: [],
    });
    expect(agents(at(true)).map((agent) => agent.column)).toEqual(["silent"]);
    expect(agents(at(false)).map((agent) => agent.column)).toEqual(["implement"]);
  });

  it("#1991: a lane whose open PR waits for review sits in Review, not Silent", () => {
    const board: TasksBoard = {
      now: "2026-10-22T12:00:00Z",
      tasks: [task("a#1", "lane-a", "review", { pr: 20 }), task("b#1", "lane-b", "implement")],
      liveness: { lanes: [
        { lane: "lane-a", lastRecordAt: "2026-10-22T11:14:00Z", owes: true, silent: true, free: false },
        { lane: "lane-b", lastRecordAt: "2026-10-22T09:00:00Z", owes: true, silent: true, free: false },
      ] },
      stalls: [], waiting: [],
    };
    expect(agents(board).map((agent) => [agent.lane, agent.column])).toEqual([["lane-a", "review"], ["lane-b", "silent"]]);
  });
});
