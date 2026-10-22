import { describe, expect, it } from "vitest";

import type { BoardTask, JourneyFlow, TasksBoard } from "../runtime/client";
import { cards, journeysWithWork, kanbanColumn, requestsFor, result, type OwnerRequest } from "./requests";

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
const request = (over: Partial<OwnerRequest>): OwnerRequest => ({
  requestId: "r1", ask: "do it", title: null, recordedBy: "owner", sequence: 1, status: "in_progress", issues: [], ...over,
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
    expect(journeysWithWork(b, flows)).toEqual([{ id: "j2", title: "j2", slices: 1 }, { id: "j1", title: "j1", slices: 1 }]);
    const both = [request({ requestId: "r1", issues: [{ issue: 1, slices: [] }] }), request({ requestId: "r2", issues: [{ issue: 2, slices: [] }] })];
    expect(requestsFor(both, b, "j1").map((row) => row.requestId)).toEqual(["r1"]);
    expect(requestsFor(both, b, null)).toHaveLength(2);
  });

  it("reads a request's result from its status, and Proven only when its merged slices' journeys passed", () => {
    const merged = board([task({ issue: 7, pr: 70, step: "merged", journeys: ["j"] })]);
    const delivered = request({ status: "delivered", issues: [{ issue: 7, slices: [{ pr: 70, step: "merged", merged: true }] }] });
    expect(result(request({ status: "open" }), merged, []).word).toBe("open");
    expect(result(request({ issues: [{ issue: 7, slices: [] }, { issue: 8, slices: [{ pr: 80, step: "merged", merged: true }] }] }), merged, []))
      .toEqual({ word: "in progress", detail: "1 of 2 issues merged" });
    expect(result(delivered, merged, [flow("j", "failed")]).word).toBe("merged");
    expect(result(delivered, merged, [flow("j", "passed")]).word).toBe("proven");
  });
});
