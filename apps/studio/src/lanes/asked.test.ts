import { describe, expect, it } from "vitest";

import type { OwnerRequest } from "../graph/requests";
import type { BoardTask } from "../runtime/client";
import { arrangeRequests, requestChip, requestProgress, requestColor, REQUEST_COLORS } from "../graph/requests";
import { arrangeAsked, askedCards, askedWhen, requestTitles, unlinkedIssue } from "./asked";
import type { Agent } from "./board";

const request = (requestId: string, issues: number[], extra: Partial<OwnerRequest> = {}): OwnerRequest => ({
  requestId, ask: `ask ${requestId}`, title: null, recordedBy: "owner", sequence: 1, status: "in_progress",
  issues: issues.map((issue) => ({ issue, slices: [] })), ...extra,
});
const agent = (lane: string, issue: number | null, extra: Partial<Agent> = {}): Agent => ({
  lane, column: "implement", idleMinutes: 2, lastRecordAt: null, kind: null, model: null, stepMinutes: null, questions: [], moreQuestions: 0,
  task: issue === null ? null : ({ key: lane, issue, step: "implement" } as BoardTask), ...extra,
});

const slice = (lane: string, issue: number, extra: Partial<BoardTask> = {}) => ({ key: `${lane}-${issue}`, lane, issue, step: "implement", ...extra }) as BoardTask;

describe("what I asked", () => {
  it("lists the newest request first with its title, date and the agents on its issues", () => {
    const asking = { lane: "b", question: "?", waitsOn: "studio-operator", since: "2026-10-22T10:00:00Z", signalId: "s", sequence: 1, laneTotal: 1 };
    // "a" shows another slice on its card but still holds one on #7; "f" released its #9 slice.
    const tasks = [slice("a", 7), slice("a", 99), slice("b", 8), slice("c", 9), slice("d", 9), slice("f", 9, { releasedBy: "graphhelm" })];
    const cards = askedCards(
      [request("r1", [7], { title: "Old ask" }), request("r2", [8, 9], { recordedAt: "2026-10-21T18:30:00" })],
      [agent("a", 99), agent("b", 8, { questions: [asking] }), agent("c", 9, { column: "merge" }), agent("d", 9, { idleMinutes: 45 }), agent("e", null), agent("f", null)],
      tasks,
      [],
      new Set(),
      new Date(2026, 9, 23, 12, 0),
    );
    expect(cards.map((card) => card.requestId)).toEqual(["r2", "r1"]);
    expect(cards[0]).toMatchObject({ title: "ask r2", when: "21 Oct 18:30", summary: "3 agents · needs a reply" });
    expect(cards[0]!.agents).toEqual([{ lane: "b", tone: "asks" }, { lane: "c", tone: "merge" }, { lane: "d", tone: "quiet" }]);
    expect(cards[1]).toMatchObject({ title: "Old ask", when: null, summary: "1 agent" });
  });

  it("shows a request the Runtime handed to a lane as in progress with that agent (#1981)", () => {
    const handed = request("fix-1", [], { status: "open", dispatchedTo: "lane-a" });
    const cards = askedCards([handed, request("fix-2", [], { status: "open" })], [agent("lane-a", null), agent("lane-b", null)], []);
    expect(cards[1]).toMatchObject({ requestId: "fix-1", summary: "1 agent", chip: { key: "in_progress", label: "In progress · handed to lane-a" } });
    expect(cards[1]!.agents).toEqual([{ lane: "lane-a", tone: expect.any(String) }]);
    expect(cards[0]).toMatchObject({ requestId: "fix-2", summary: "no agent working", chip: { key: "not_started" } });
  });

  it("names on each card the newest request its lane serves (#1904)", () => {
    const tasks = [slice("a", 7), slice("b", 8), slice("b", 7)];
    const titles = requestTitles([request("r1", [7], { title: "Old ask" }), request("r2", [8])], [agent("a", 7), agent("b", 8), agent("c", null)], tasks);
    expect([...titles]).toEqual([["b", "ask r2"], ["a", "Old ask"]]);
  });

  it("leaves an absent or unreadable time undated", () => {
    expect(askedWhen(undefined)).toBeNull();
    expect(askedWhen("not a time")).toBeNull();
  });
  it("flags an issue that no recorded request links (#1921)", () => {
    const requests = [request("r1", [7])];
    expect(unlinkedIssue(requests, 7)).toBe(false);
    expect(unlinkedIssue(requests, 8)).toBe(true);
    expect(unlinkedIssue([], 8)).toBe(true);
    expect(unlinkedIssue(null, 8)).toBe(false);
    expect(unlinkedIssue(requests, null)).toBe(false);
  });
});


describe("request status chips, order and delivered collapse (#1922)", () => {
  const NOW = Date.parse("2026-10-23T12:00:00Z");
  const hours = (n: number) => new Date(NOW - n * 3600_000).toISOString();
  const done = (issue: number) => ({ issue, slices: [{ pr: issue, step: "merged", merged: true }] });
  const merged = (issue: number, ago: number, journeys: string[] = []) => ({ key: `k${issue}`, lane: "x", issue, step: "merged", journeys, clock: { since: hours(ago) } }) as unknown as BoardTask;
  const flow = (id: string, status: "passed" | "failed") => ({ id, title: id, status: "draft", lastRun: { status, ranAt: null, kind: "replay", reason: null, failingStep: null, flowChanged: null } }) as never;

  it("maps a request to its chip", () => {
    expect(requestChip(request("a", [], { status: "open" }), [], []).label).toBe("Not started");
    expect(requestChip(request("a", [1, 2], { issues: [done(1), { issue: 2, slices: [] }] }), [], []).label).toBe("In progress · 1 of 2");
    expect(requestChip(request("a", [1], { status: "delivered", issues: [done(1)] }), [merged(1, 1)], []).key).toBe("delivered");
    expect(requestChip(request("a", [1], { status: "delivered", issues: [done(1)] }), [merged(1, 1, ["j"])], [flow("j", "passed")]).key).toBe("proven");
    expect(requestChip(request("a", [1], { status: "delivered", issues: [done(1)] }), [merged(1, 1, ["j"])], [flow("j", "failed")]).key).toBe("delivered");
  });

  it("puts working first (newest on top), then not started, then delivered; hides delivered older than 24 h", () => {
    const rows = [
      { id: "old-work", key: "in_progress" as const, at: NOW - 5 * 3600_000 },
      { id: "delivered-new", key: "delivered" as const, at: NOW - 2 * 3600_000 },
      { id: "idle", key: "not_started" as const, at: NOW - 1 * 3600_000 },
      { id: "new-work", key: "in_progress" as const, at: NOW - 1 * 60_000 },
      { id: "delivered-old", key: "proven" as const, at: NOW - 30 * 3600_000 },
    ];
    const arranged = arrangeRequests(rows, (row) => row, NOW);
    expect(arranged.open.map((row) => row.id)).toEqual(["new-work", "old-work", "idle"]);
    expect(arranged.delivered.map((row) => row.id)).toEqual(["delivered-new", "delivered-old"]);
    expect(arranged.recent.map((row) => row.id)).toEqual(["delivered-new"]);
  });

  it("says no agent working and arranges the Lanes cards", () => {
    const cards = askedCards(
      [request("d", [3], { status: "delivered", issues: [done(3)] }), request("n", [], { status: "open" })],
      [], [merged(3, 50)], [],
    );
    expect(cards.find((card) => card.requestId === "n")!.summary).toBe("no agent working");
    const arranged = arrangeAsked(cards, NOW);
    expect(arranged.open.map((card) => card.requestId)).toEqual(["n"]);
    expect(arranged.delivered.map((card) => card.requestId)).toEqual(["d"]);
    expect(arranged.recent).toEqual([]);
  });
});

describe("request colour (#1922)", () => {
  it("is stable per id, drawn from the palette, and spreads across it", () => {
    expect(requestColor("screens-match-design")).toBe(requestColor("screens-match-design"));
    const ids = Array.from({ length: 40 }, (_, at) => `request-${at}`);
    for (const id of ids) expect(REQUEST_COLORS).toContain(requestColor(id));
    expect(new Set(ids.map(requestColor)).size).toBeGreaterThan(4);
    expect(askedCards([request("r1", [7])], [], [], [])[0]!.color).toBe(requestColor("r1"));
  });
});

describe("request progress bar (#1922)", () => {
  const flow = (id: string, status: "passed" | "failed") => ({ id, title: id, status: "draft", lastRun: { status, ranAt: null, kind: "replay", reason: null, failingStep: null, flowChanged: null } }) as never;
  it("orders segments proven to stalled, counts a legend and the percent delivered", () => {
    const t = (issue: number, step: string, journeys: string[] = []) => ({ key: `k${issue}`, lane: "x", issue, step, journeys, pr: null }) as unknown as BoardTask;
    const req = request("p", [1, 2, 3, 4], { issues: [1, 2, 3, 4].map((issue) => ({ issue, slices: [] })) });
    const progress = requestProgress(req, [t(1, "merged", ["j"]), t(2, "merged"), t(3, "review"), t(4, "implement")], [flow("j", "passed")], new Set());
    expect(progress.segs.map((seg) => seg.state)).toEqual(["proven", "merged, waits proof", "in review", "implementing"]);
    expect(progress.segs.map((seg) => seg.color)).toEqual(["#3ddc97", "#d08cff", "#4ea1ff", "#ff8a3d"]);
    expect(progress.pct).toBe("50%");
    expect(progress.chip.label).toBe("In progress · 2 of 4");
    expect(progress.legend.map((g) => `${g.n} ${g.label}`)).toEqual(["1 proven", "1 merged, waits proof", "1 in review", "1 implementing"]);
    expect(requestProgress(req, [t(1, "implement")], [], new Set(["k1"])).chip.key).toBe("stalled");
  });
});
