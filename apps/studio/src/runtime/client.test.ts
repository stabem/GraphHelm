import { afterEach, describe, expect, it, vi } from "vitest";

import { agents } from "../lanes/board";
import { needsAction } from "../lanes/panels";
import { cards } from "../graph/requests";
import { ViewBoundary } from "../views/ViewBoundary";
import { refusedToken, Runtime } from "./client";

// #1899: the owner's Runtime predates #1794, so its open tasks reply has no `waiting`. This is the
// key set that Runtime returned for a real run (read-only GET), with one open slice and one lane.
const OLD_REPLY = {
  ok: true,
  command: "execution.tasks",
  data: {
    backlog: [], durations: { since: null, slices: [], summary: {} }, endedTasks: 0, executionId: "gh-team",
    headSequence: 9, leases: [], opened: 1, rejected: 0, view: "open", now: "2026-10-22T20:56:36Z",
    liveness: { budgets: { freeAfterSeconds: 900, silentAfterSeconds: 1800, stallAfterSeconds: 7200 }, lanes: [{ free: false, lane: "claude-1", owes: null, silent: false }] },
    stalls: [],
    tasks: [{ key: "t#claim-0", issue: 7, pr: null, lane: "claude-1", currentReviewer: null, step: "implement", blockedBy: null, title: "t", prTitle: null, journeys: [], lastSequence: 3 }],
  },
  diagnostics: [],
};

afterEach(() => vi.unstubAllGlobals());

describe("a run read from an older Runtime (#1899)", () => {
  it("reads a tasks reply without waiting as a board the Lanes and Graph views can show", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify(OLD_REPLY), { status: 200 })));
    const board = await new Runtime("t").board("gh-team");
    expect(board.waiting).toEqual([]);
    // These threw `board.waiting is not iterable` and blanked the Studio before the fix.
    expect(agents(board).map((agent) => [agent.lane, agent.column])).toEqual([["claude-1", "implement"]]);
    expect(() => needsAction(board)).not.toThrow();
    expect(cards(board, [], null)).toHaveLength(1);
  });

  it("reads a reply missing every list as an empty board", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ ok: true, data: {} }), { status: 200 })));
    const board = await new Runtime("t").board("old");
    expect([board.tasks, board.stalls, board.waiting, board.liveness.lanes]).toEqual([[], [], [], []]);
    expect(agents(board)).toEqual([]);
  });

  it("shows a view's render error as an alert instead of unmounting the Studio", () => {
    const boundary = new ViewBoundary({ name: "lanes", children: null });
    boundary.state = ViewBoundary.getDerivedStateFromError(new TypeError("board.waiting is not iterable"));
    const shown = boundary.render() as { props: { children: { props: { role: string; children: unknown[] } } } };
    expect(shown.props.children.props.role).toBe("alert");
    expect(shown.props.children.props.children.join("")).toContain("board.waiting is not iterable");
  });
});

describe("a Runtime that refuses the token (#1897)", () => {
  it("tells a refusal (401, 403) from a Runtime that does not answer", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ ok: false, data: null, diagnostics: [{ message: "bad token" }] }), { status: 401 })));
    const refused = await new Runtime("old").runs().catch((error: unknown) => error);
    expect(refusedToken(refused)).toBe(true);
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("down"); }));
    const silent = await new Runtime("old").runs().catch((error: unknown) => error);
    expect(refusedToken(silent)).toBe(false);
  });
});
