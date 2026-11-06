import { describe, expect, it } from "vitest";

import type { OwnerRequest } from "../graph/requests";
import type { BoardTask } from "../runtime/client";
import { requestLanes } from "./asked";
import type { Agent } from "./board";
import { applyAskFilter, readAskParam, withAskParam } from "./request-filter";

const request = (requestId: string, issues: number[]): OwnerRequest => ({
  requestId, ask: `ask ${requestId}`, title: null, recordedBy: "owner", sequence: 1, status: "in_progress",
  issues: issues.map((issue) => ({ issue, slices: [] })),
} as OwnerRequest);
const agent = (lane: string, column: Agent["column"], issue: number | null): Agent => ({
  lane, column, idleMinutes: 2, lastRecordAt: null, kind: null, model: null, stepMinutes: null, questions: [], moreQuestions: 0,
  task: issue === null ? null : ({ key: lane, issue, step: "implement" } as BoardTask),
} as Agent);
const slice = (lane: string, issue: number) => ({ key: `${lane}-${issue}`, lane, issue, step: "implement" }) as BoardTask;

// #2530: r1 is an old ask on #7; r2 is the newer ask on #8 and #9. "d" holds no issue of any request.
const requests = [request("r1", [7]), request("r2", [8, 9])];
const all = [agent("a", "implement", 7), agent("b", "implement", 8), agent("c", "review", 9), agent("e", "blocked", 9), agent("d", "backlog", null)];
const tasks = [slice("a", 7), slice("b", 8), slice("c", 9), slice("e", 9)];

describe("the Lanes board narrowed to one request (#2530)", () => {
  it("selecting a request leaves only its agents on the board, and clearing it brings every agent back", () => {
    const lanes = requestLanes(requests, all, tasks);
    const selected = lanes.get("c") ?? null;
    const forR2 = new Set([...lanes].filter(([, key]) => key === "r2").map(([lane]) => lane));
    expect(selected).toBe("r2");

    const shown = applyAskFilter(all, forR2);
    expect(shown.map((row) => row.lane).sort()).toEqual(["b", "c", "e"]);
    expect(shown.filter((row) => row.column === "implement").map((row) => row.lane)).toEqual(["b"]);
    expect(shown.filter((row) => row.column === "review").map((row) => row.lane)).toEqual(["c"]);
    expect(shown.filter((row) => row.column === "blocked").map((row) => row.lane)).toEqual(["e"]);
    expect(shown.some((row) => row.lane === "a" || row.lane === "d")).toBe(false);

    expect(applyAskFilter(all, null)).toHaveLength(all.length);
  });

  it("an empty selection shows no agent when the request has none on the board", () => {
    expect(applyAskFilter(all, new Set<string>())).toEqual([]);
  });

  it("the selection is read from and written to ?ask= without touching other parameters", () => {
    expect(readAskParam("?ask=execA/r2")).toBe("execA/r2");
    expect(readAskParam("?view=lanes")).toBeNull();
    expect(readAskParam("?ask=../etc")).toBeNull();
    expect(readAskParam("?ask=" + "x".repeat(201))).toBeNull();

    expect(withAskParam("?view=lanes&journey=j1", "execA/r2")).toBe("?view=lanes&journey=j1&ask=execA%2Fr2");
    expect(withAskParam("?view=lanes&ask=r2", null)).toBe("?view=lanes");
    expect(withAskParam("?ask=r2", null)).toBe("");
  });
});
