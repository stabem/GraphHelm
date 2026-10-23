import { describe, expect, it } from "vitest";

import type { BoardTask, JourneyFlow, TasksBoard } from "../runtime/client";
import type { OwnerRequest } from "./requests";
import { askHeight, ASK_MIN, counts, detail, graphRequests, journeyCards, journeyMeta, ROW, rowHeight, stage } from "./layout";

const task = (over: Partial<BoardTask>): BoardTask => ({
  key: "t#claim-0", issue: 1, pr: null, lane: "lane-a", currentReviewer: null, step: "implement",
  blockedBy: null, title: "t", prTitle: null, journeys: [], lastSequence: 1, ...over,
});
const flow = (id: string, status?: "passed"): JourneyFlow => ({
  id, title: `Title ${id}`, status: "draft",
  ...(status === undefined ? {} : { lastRun: { status, ranAt: null, kind: "replay", reason: null, failingStep: null, flowChanged: null } }),
});
const board = (tasks: BoardTask[], stalls: string[] = []): TasksBoard => ({
  now: "2026-10-22T01:00:00Z", tasks, liveness: { lanes: [] }, stalls: stalls.map((key) => ({ key, owner: null })), waiting: [],
});
const request = (over: Partial<OwnerRequest>): OwnerRequest => ({
  requestId: "r1", ask: "do it", title: null, recordedBy: "owner", sequence: 1, status: "in_progress", issues: [], ...over,
});

// #1903: the design's Graph view is computed from the Runtime board, requests and journeys.
describe("Graph view layout", () => {
  const tasks = [
    task({ key: "a", issue: 10, lane: "lane-a", journeys: ["j1"], step: "implement" }),
    task({ key: "b", issue: 11, lane: "lane-b", journeys: ["j1"], step: "review", pr: 21, currentReviewer: "rev" }),
    task({ key: "c", issue: 12, lane: "lane-c", journeys: ["j2"], step: "merged", pr: 22 }),
    task({ key: "d", issue: 13, lane: "lane-a", journeys: ["j2"], step: "plan" }),
  ];
  const b = board(tasks, ["d"]);
  const flows = [flow("j1"), flow("j2"), flow("j3")];

  it("lists one journey card per journey with work, with its issues, PRs and lanes", () => {
    const cards = journeyCards(b, flows);
    expect(cards.map((card) => card.id)).toEqual(["j1", "j2"]);
    expect(cards[0]).toMatchObject({ title: "Title j1", issues: 2, prs: 1, lanes: ["lane-a", "lane-b"], stalled: false });
    expect(journeyMeta(cards[0])).toBe("2 issues · 1 PR");
    expect(journeyMeta(cards[1])).toBe("2 issues · stalled");
  });

  it("counts the header per column and narrows it to a journey", () => {
    expect(counts(b, flows, null)).toMatchObject({ implementing: 1, review: 1, merged: 1, ready: 1 });
    expect(counts(b, flows, "j1")).toMatchObject({ implementing: 1, review: 1, merged: 0, ready: 0 });
  });

  it("draws each request with a row per issue, its PR, result and three lines per issue", () => {
    const req = request({ recordedAt: "2026-10-21T19:40:00Z", issues: [
      { issue: 11, slices: [{ pr: 21, step: "review", merged: false }] },
      { issue: 12, slices: [{ pr: 22, step: "merged", merged: true }] },
    ] });
    const [row] = graphRequests([req], b, flows, null);
    expect(row.items.map((item) => [item.issue, item.y, item.pr, item.result.word])).toEqual([
      [11, 0, 21, "Not merged"], [12, ROW, 22, "Merged · waits proof"],
    ]);
    expect(row.items[0].prLine).toBe("rev reviewing");
    expect(row.links).toHaveLength(6);
    expect(row.summary).toBe("2 issues · 2 PRs · 1 merged");
    expect(row.when).not.toBe("owner");
    expect(graphRequests([req], b, flows, "j2")[0].items.map((item) => item.issue)).toEqual([12]);
    expect(graphRequests([req], b, flows, "j3")).toEqual([]);
    const panel = detail(row.items[0], row, b);
    expect(panel.now).toContain("Author lane-b");
    expect(panel.history).toContain("PR #21 · review");
  });

  it("fills the step squares up to the slice's step, all of them only once proven", () => {
    expect(stage(task({ step: "plan" }), "implementing")).toBe(1);
    expect(stage(task({ step: "implement", pr: 3 }), "implementing")).toBe(4);
    expect(stage(task({ step: "merged" }), "merged")).toBe(6);
    expect(stage(task({ step: "merged" }), "proven")).toBe(8);
  });
});

// #1947: a request row is never shorter than its YOU ASKED card.
describe("request row height", () => {
  it("gives a request with no issue or one issue the card's height, and many issues their rows", () => {
    expect(rowHeight(0, 2)).toBe(askHeight(2) + 8);
    expect(rowHeight(1, 2)).toBe(askHeight(2) + 8);
    expect(askHeight(1)).toBeGreaterThanOrEqual(ASK_MIN);
    expect(askHeight(5)).toBeGreaterThan(askHeight(1));
    expect(rowHeight(4, 2)).toBe(4 * ROW + 8);
  });

  it("centres the card inside its row, so it never leaves the row", () => {
    const b = board([task({ key: "a", issue: 10, step: "implement" })]);
    const rows = graphRequests([request({ requestId: "none" }), request({ requestId: "one", issues: [{ issue: 10, slices: [] }] })], b, [], null);
    for (const row of rows) {
      const card = askHeight(row.progress.legend.length);
      expect(row.height).toBeGreaterThanOrEqual(card);
      expect(row.askY).toBeGreaterThanOrEqual(0);
      expect(row.askY + card).toBeLessThanOrEqual(row.height);
    }
    expect(rows.map((row) => row.items.length)).toEqual([0, 1]);
  });
});
