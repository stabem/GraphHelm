import { describe, expect, it } from "vitest";

import type { BoardLaneLiveness, BoardTask, RunSummary, TasksBoard } from "../runtime/client";
import { agents } from "./board";
import { combineBoards, emptyPresence, foldPresence, homeRun, lanesRuns, presence, withPresence, type RunEvent } from "./project-board";
import { teamRun } from "../sidebar/project-summary";

const task = (key: string, lane: string, extra: Partial<BoardTask> = {}): BoardTask => ({
  key, issue: 7, pr: null, lane, currentReviewer: null, step: "implement", blockedBy: null, title: "Work", prTitle: null, journeys: [], lastSequence: 1, ...extra,
});
const live = (lane: string, lastRecordAt: string, extra: Partial<BoardLaneLiveness> = {}): BoardLaneLiveness => ({ lane, lastRecordAt, owes: false, silent: false, free: false, ...extra });
const board = (now: string, tasks: BoardTask[], lanes: BoardLaneLiveness[]): TasksBoard => ({ now, tasks, liveness: { lanes }, stalls: [], waiting: [] });
const run = (executionId: string, status: string, lastEventAt: string): RunSummary => ({ executionId, status, startedAt: lastEventAt, lastEventAt });

const NOW = "2026-10-29T12:00:00Z";
// The observed project: an old marked run holds nothing; a newer paused run holds Leslie's work.
const owner = { executionId: "graphhelm-owner", board: board(NOW, [], []) };
const product = { executionId: "zuvemo-produto-experiencia-ops", board: board(NOW, [task("k1", "leslie")], [live("leslie", "2026-10-29T11:50:00Z")]) };
const dev = { executionId: "zuvemo-desenvolvimento-ops", board: board(NOW, [task("k1", "dev-a"), task("k2", "leslie", { clock: { since: "2026-10-27T09:00:00Z" } })], [live("dev-a", "2026-10-26T08:00:00Z"), live("leslie", "2026-10-27T09:00:00Z", { silent: true })]) };

describe("project lanes (#2228)", () => {
  it("the single team run Lanes read before shows none of the project's agents", () => {
    const runs = [run("graphhelm-owner", "running", "2026-09-01T00:00:00Z"), run("zuvemo-produto-experiencia-ops", "paused", "2026-10-29T11:50:00Z")];
    const only = teamRun(runs, "graphhelm-owner")!.executionId;
    expect(agents(owner.board)).toHaveLength(0);
    expect(only).toBe("graphhelm-owner");
    // Without ?run= every listed execution is read; ?run= keeps its filtered view.
    expect(lanesRuns(runs, null)).toEqual(["graphhelm-owner", "zuvemo-produto-experiencia-ops"]);
    expect(lanesRuns(runs, "zuvemo-desenvolvimento-ops")).toEqual(["zuvemo-desenvolvimento-ops"]);
    expect(lanesRuns(null, null)).toEqual([]);
  });

  it("aggregates every execution: one card per lane address, no double count, origin newest first", () => {
    const merged = combineBoards([owner, product, dev]);
    const cards = agents(merged);
    expect(cards.map((card) => card.lane)).toEqual(["dev-a", "leslie"]);
    expect(merged.tasks.map((slice) => slice.key)).toEqual(["zuvemo-produto-experiencia-ops/k1", "zuvemo-desenvolvimento-ops/k1", "zuvemo-desenvolvimento-ops/k2"]);
    expect(merged.origins.leslie).toEqual(["zuvemo-produto-experiencia-ops", "zuvemo-desenvolvimento-ops"]);
    expect(homeRun(merged, "leslie")).toBe("zuvemo-produto-experiencia-ops");
    // Her newest record decides her liveness: an older silent verdict elsewhere does not.
    const leslie = cards.find((card) => card.lane === "leslie")!;
    expect(leslie.lastRecordAt).toBe("2026-10-29T11:50:00Z");
    expect(leslie.column).not.toBe("silent");
  });

  it("never merges two lanes that share a display name; lanes are addresses", () => {
    const a = { executionId: "r1", board: board(NOW, [task("k", "leslie-1")], [{ ...live("leslie-1", NOW), agentKind: "claude" }]) };
    const b = { executionId: "r2", board: board(NOW, [task("k", "leslie-2")], [{ ...live("leslie-2", NOW), agentKind: "claude" }]) };
    expect(agents(combineBoards([a, b])).map((card) => card.lane)).toEqual(["leslie-1", "leslie-2"]);
  });

  it("reads presence from record times only: old => stale, none => unknown, a paused run's recent record => working", () => {
    const merged = combineBoards([product, dev]);
    const cards = Object.fromEntries(agents(merged).map((card) => [card.lane, card]));
    expect(presence(cards["dev-a"].idleMinutes, cards["dev-a"].task !== null)).toBe("stale");
    expect(presence(cards.leslie.idleMinutes, cards.leslie.task !== null)).toBe("working");
    expect(presence(null, true)).toBe("unknown");
    expect(presence(45, true)).toBe("idle");
  });

  it("shows a lane that only declared presence and signalled in a non-team run; a signal-only orchestrator and other projects stay out", () => {
    const ev = (sequence: number, type: string, actor: string, occurredAt: string, projectId = "zuvemo", executionId = "zuvemo-produto-experiencia-ops"): RunEvent =>
      ({ sequence, occurredAt, actor: { id: actor }, scope: { projectId, executionId }, kind: { type, data: type === "agent_presence_declared" ? { actorId: actor } : {} } });
    let index = foldPresence(emptyPresence(), "graphhelm-owner", [ev(1, "signal_recorded", "orchestrator", "2026-10-29T11:00:00Z", "zuvemo", "graphhelm-owner")]);
    index = foldPresence(index, "zuvemo-produto-experiencia-ops", [
      ev(1, "agent_presence_declared", "leslie", "2026-10-29T11:40:00Z"),
      ev(2, "signal_recorded", "leslie", "2026-10-29T11:55:00Z"),
      ev(3, "signal_recorded", "orchestrator", "2026-10-29T11:56:00Z"),
      ev(4, "agent_presence_declared", "intruder", "2026-10-29T11:57:00Z", "another-project"),
    ]);
    const empty = { executionId: "zuvemo-produto-experiencia-ops", board: board(NOW, [], []) };
    const merged = withPresence(combineBoards([owner, empty]), index);
    const cards = agents(merged);
    // Main's Lanes (the marked team run, the #1986 rule) shows nobody for this project.
    expect(agents(owner.board)).toEqual([]);
    expect(agents(combineBoards([owner, empty]))).toEqual([]);
    expect(cards.map((card) => card.lane)).toEqual(["leslie"]);
    expect(cards[0].lastRecordAt).toBe("2026-10-29T11:55:00Z");
    expect(presence(cards[0].idleMinutes, cards[0].task !== null)).toBe("idle");
    expect(merged.origins.leslie).toEqual(["zuvemo-produto-experiencia-ops"]);
    expect(index.cursor).toEqual({ "graphhelm-owner": 1, "zuvemo-produto-experiencia-ops": 4 });
    // A presence older than a day is history: stale, still shown, never "working".
    const old = foldPresence(emptyPresence(), "r", [ev(1, "agent_presence_declared", "leslie", "2026-10-20T10:00:00Z", "zuvemo", "r")]);
    const oldCard = agents(withPresence(combineBoards([{ executionId: "r", board: board(NOW, [], []) }, owner]), old))[0];
    expect(presence(oldCard.idleMinutes, oldCard.task !== null)).toBe("stale");
  });
});
