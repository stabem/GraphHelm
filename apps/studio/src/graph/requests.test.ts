import { describe, expect, it } from "vitest";

import type { BoardTask, JourneyFlow, TasksBoard } from "../runtime/client";
import { cards, kanbanColumn } from "./requests";

const task = (over: Partial<BoardTask>): BoardTask => ({
  key: "t#claim-0", issue: 1, pr: null, lane: "lane-a", currentReviewer: null, step: "implement",
  blockedBy: null, title: "t", prTitle: null, journeys: [], lastSequence: 1, ...over,
});
const flow = (id: string, status?: "passed" | "failed"): JourneyFlow => ({
  id, title: id, status: "draft",
  ...(status === undefined ? {} : { lastRun: { status, ranAt: null, kind: "replay", reason: null, failingStep: null, flowChanged: null } }),
});
const board = (tasks: BoardTask[], stalls: string[] = []): TasksBoard => ({
  now: "2026-10-22T00:00:00Z", tasks, liveness: { lanes: [] }, stalls: stalls.map((key) => ({ key, owner: null })), waiting: [],
});

// #1801: the Graph's columns and results are the contract the studio2-graph-requests journey reads.
describe("Graph requests and Kanban", () => {
  it("puts a slice in its column, and a merged one in Proven only when every journey it names passed", () => {
    const flows = [flow("j-pass", "passed"), flow("j-fail", "failed"), flow("j-none")];
    const none = new Set<string>();
    expect(kanbanColumn(task({ step: "plan" }), none, flows)).toBe("implementing");
    expect(kanbanColumn(task({ step: "review" }), none, flows)).toBe("review");
    expect(kanbanColumn(task({ step: "merge" }), none, flows)).toBe("review");
    expect(kanbanColumn(task({ key: "s" }), new Set(["s"]), flows)).toBe("ready");
    expect(kanbanColumn(task({ step: "merged", journeys: ["j-pass"] }), none, flows)).toBe("proven");
    // Merged is not done: a failing, unrun or absent journey, or none named, waits for proof.
    for (const journeys of [["j-pass", "j-fail"], ["j-none"], ["j-unknown"], []]) {
      expect(kanbanColumn(task({ step: "merged", journeys }), none, flows)).toBe("merged");
    }
  });

  it("narrows cards, requests and the filter to one journey, and leaves released slices out", () => {
    const b = board([
      task({ key: "a", issue: 1, journeys: ["j1"] }),
      task({ key: "b", issue: 2, journeys: ["j2"] }),
      task({ key: "c", issue: 3, journeys: ["j1"], releasedBy: "owner" }),
    ]);
    const flows = [flow("j2"), flow("j1")];
    expect(cards(b, flows, "j1").map((card) => card.task.key)).toEqual(["a"]);
    expect(cards(b, flows, null).map((card) => card.task.key)).toEqual(["a", "b"]);
  });

  it("ages each card from the record that moved its slice into its step (#1855)", () => {
    const b = { ...board([
      task({ key: "old", clock: { since: "2026-10-21T22:00:00Z" } }),
      task({ key: "new", clock: { since: "2026-10-21T23:30:00Z" } }),
      task({ key: "untimed" }),
    ]) };
    expect(cards(b, [], null).map((card) => [card.task.key, card.stepMinutes])).toEqual([["old", 120], ["new", 30], ["untimed", null]]);
  });
});
