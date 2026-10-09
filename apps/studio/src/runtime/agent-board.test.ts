import { describe, expect, it } from "vitest";
import { agentBoard, span } from "./agent-board";
import type { Bot } from "./team";
import type { Lane } from "./lane-bars";
import type { MissionTask } from "./mission";

const now = 10 * 3_600_000;
const M = 60_000;
const bot = (name: string): Bot => ({ key: name, actorId: name, name, hue: 0, role: null, doingNow: "", lastRecordAt: null, lastSequence: 0,
  state: "working", quietMinutes: null, shared: false, native: false, tasks: [] });
const lane = (name: string, bars: Lane["bars"], silent = false): Lane => ({ lane: name, bars, silent, lastEventAt: 0 });
const bar = (kind: "implement" | "review" | "merge", since: number, open = true, end = now, label = "#7") =>
  ({ kind, label, start: since, end, open, since, taskId: "t7" });
const task = (over: Partial<MissionTask>): MissionTask => ({ key: "t7", pr: 7, issue: null, title: "Add board", lane: "a", reviewers: [],
  step: "implement", blocked: false, trust: 1, blockedBy: null, rounds: [], headSha: null, mergeSha: null, repoUrl: "https://github.com/o/r", ...over });

describe("agentBoard", () => {
  it("status rules: implement working, review/merge waiting, stall silent, no open bar free", () => {
    const rows = agentBoard([bot("a"), bot("b"), bot("c"), bot("d"), bot("e")], [
      lane("a", [bar("implement", now - 10 * M)]), lane("b", [bar("review", now - 5 * M)]), lane("c", [bar("merge", now - 5 * M)], true),
      lane("d", [bar("implement", now - 60 * M, false, now - 20 * M)]),
    ], [], now);
    expect(Object.fromEntries(rows.map((r) => [r.name, r.status]))).toEqual({ a: "working", b: "waiting", c: "silent", d: "free", e: "free" });
  });

  it("duration is since the open bar started, with PR, title and link", () => {
    const [r] = agentBoard([bot("a")], [lane("a", [bar("implement", now - 77 * M)])], [task({})], now);
    expect(r).toMatchObject({ stage: "implement", pr: 7, title: "Add board", href: "https://github.com/o/r/pull/7", forMs: 77 * M });
    expect(span(r!.forMs!)).toBe("1h 17m");
  });

  it("free time is since the last bar ended; unknown without bars", () => {
    const rows = agentBoard([bot("d"), bot("e")], [lane("d", [bar("review", now - 60 * M, false, now - 23 * M)])], [], now);
    expect(rows.find((r) => r.name === "d")!.forMs).toBe(23 * M);
    expect(rows.find((r) => r.name === "e")!.forMs).toBeNull();
  });

  it("sorts silent, working longest first, waiting, free", () => {
    const rows = agentBoard([bot("f"), bot("w2"), bot("q"), bot("w1"), bot("s")], [
      lane("w1", [bar("implement", now - 10 * M)]), lane("w2", [bar("implement", now - 90 * M)]),
      lane("q", [bar("review", now - 5 * M)]), lane("s", [bar("review", now - 300 * M)], true),
    ], [], now);
    expect(rows.map((r) => r.name)).toEqual(["s", "w2", "w1", "q", "f"]);
  });

  it("last delivered is the newest merged task of the lane; lanes without a bot still get a row", () => {
    const rows = agentBoard([], [lane("a", [])], [task({ pr: 5, title: "Old", step: "merged" }), task({ pr: 6, title: "New", step: "merged" })], now);
    expect(rows).toHaveLength(1);
    expect(rows[0]!.lastDelivered).toBe("#6 New");
  });
});
