import { describe, expect, it } from "vitest";
import { agentBoard } from "./agent-board";
import { laneBars, packBars, placeholderLane, STALL_MS, type TimedTaskEvent } from "./lane-bars";

const T0 = Date.parse("2026-10-09T00:00:00Z");
const at = (ms: number) => new Date(T0 + ms).toISOString();
let seq = 0;
const ev = (e: Partial<TimedTaskEvent>): TimedTaskEvent => ({ actorId: "a", sequence: seq++, taskId: "t1", ...e }) as TimedTaskEvent;

describe("laneBars", () => {
  it("implement then review then merge", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", pr: 9, at: at(0) }),
      ev({ kind: "task.review_assigned", reviewer: "rev", pr: 9, at: at(100) }),
      ev({ kind: "task.review_verdict", reviewer: "rev", verdict: "APPROVE" as never, pr: 9, at: at(200) }),
      ev({ kind: "task.merged", pr: 9, at: at(300) }),
    ], T0 + 400, 1000);
    const dev = lanes.find((l) => l.lane === "dev")!;
    const rev = lanes.find((l) => l.lane === "rev")!;
    expect(dev.bars).toEqual([{ kind: "implement", label: "#9", start: T0, end: T0 + 100, open: false, taskId: "t1", since: T0 }]);
    expect(rev.bars.map((b) => [b.kind, b.start - T0, b.end - T0, b.open])).toEqual([["review", 100, 200, false], ["merge", 200, 300, false]]);
  });

  it("open bar runs to now", () => {
    const [lane] = laneBars([ev({ kind: "task.claimed", lane: "dev", at: at(0) })], T0 + 50, 1000);
    expect(lane.bars[0]).toMatchObject({ end: T0 + 50, open: true });
  });

  it("silence threshold", () => {
    const events = [ev({ kind: "task.review_assigned", reviewer: "rev", at: at(0) })];
    expect(laneBars(events, T0 + STALL_MS - 1, STALL_MS * 2)[0].silent).toBe(false);
    expect(laneBars(events, T0 + STALL_MS, STALL_MS * 2)[0].silent).toBe(true);
  });

  it("an open implement bar is never silent", () => {
    const [lane] = laneBars([ev({ kind: "task.claimed", lane: "dev", at: at(0) })], T0 + STALL_MS * 3, STALL_MS * 4);
    expect(lane.silent).toBe(false);
  });

  it("window clamps and drops", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "old", taskId: "o", at: at(0) }),
      ev({ kind: "task.review_assigned", reviewer: "x", taskId: "o", at: at(10) }),
      ev({ kind: "task.claimed", lane: "dev", taskId: "n", at: at(500) }),
    ], T0 + 1000, 600);
    expect(lanes.find((l) => l.lane === "old")!.bars).toEqual([]);
    expect(lanes.find((l) => l.lane === "x")!.bars[0].start).toBe(T0 + 400);
  });

  it("APPROVE-WITH-RISK opens a merge bar, BLOCK does not", () => {
    for (const [v, n] of [["APPROVE-WITH-RISK", 1], ["BLOCK", 0]] as const) {
      const lanes = laneBars([
        ev({ kind: "task.review_assigned", reviewer: "rev", at: at(0) }),
        ev({ kind: "task.review_verdict", reviewer: "rev", verdict: v as never, at: at(10) }),
      ], T0 + 20, 1000);
      expect(lanes.find((l) => l.lane === "rev")!.bars.filter((b) => b.kind === "merge")).toHaveLength(n);
    }
  });

  it("a repeated assign closes the first bar", () => {
    const lanes = laneBars([
      ev({ kind: "task.review_assigned", reviewer: "rev", at: at(0) }),
      ev({ kind: "task.review_assigned", reviewer: "rev", at: at(10) }),
      ev({ kind: "task.review_verdict", reviewer: "rev", verdict: "BLOCK" as never, at: at(20) }),
    ], T0 + STALL_MS * 2, STALL_MS * 4);
    expect(lanes[0].bars.some((b) => b.open)).toBe(false);
    expect(lanes[0].silent).toBe(false);
  });

  it("merged closes every open bar of the task", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", at: at(0) }),
      ev({ kind: "task.merged", at: at(50) }),
    ], T0 + 100, 1000);
    expect(lanes[0].bars[0]).toMatchObject({ end: T0 + 50, open: false });
  });

  it("skips events with an invalid timestamp", () => {
    const lanes = laneBars([ev({ kind: "task.claimed", lane: "dev", at: "garbage" })], T0, 1000);
    expect(lanes).toEqual([]);
  });

  it("#549: bars are kept per PR, so another slice's merge or claim never closes #581's review", () => {
    const t = "issue-549";
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "author", taskId: t, branch: "s1", at: at(0) } as never),
      ev({ kind: "task.pr_opened", lane: "author", taskId: t, pr: 571, headSha: "aaaa1111", at: at(10) } as never),
      ev({ kind: "task.claimed", lane: "author", taskId: t, branch: "s2", at: at(20) } as never),
      ev({ kind: "task.pr_opened", lane: "author", taskId: t, pr: 581, headSha: "66613c95", at: at(30) } as never),
      ev({ kind: "task.review_assigned", reviewer: "gh-claude-8", taskId: t, pr: 581, headSha: "66613c95", at: at(40) } as never),
      ev({ kind: "task.merged", taskId: t, pr: 571, at: at(50) }),
      ev({ kind: "task.claimed", lane: "author", taskId: t, branch: "s3", at: at(60) } as never),
    ], T0 + 100, 1000);
    const rev = lanes.find((l) => l.lane === "gh-claude-8")!;
    expect(rev.bars).toEqual([expect.objectContaining({ kind: "review", label: "#581", open: true })]);
    const author = lanes.find((l) => l.lane === "author")!;
    expect(author.bars.filter((b) => b.open).map((b) => b.label)).toEqual([t]);
    const row = agentBoard([{ name: "gh-claude-8" } as never], lanes, [], T0 + 100).find((r) => r.name === "gh-claude-8")!;
    expect(row).toMatchObject({ stage: "review", pr: 581 });
    expect(row.status).not.toBe("free");
  });
});

describe("packBars (#591)", () => {
  const bars = [
    { start: 0, end: 100 }, { start: 10, end: 50 }, { start: 20, end: 30 }, { start: 50, end: 90 }, { start: 100, end: 120 }, { start: 30, end: 60 },
  ];
  it("no two bars in one sub-row overlap", () => {
    const { row } = packBars(bars);
    for (let i = 0; i < bars.length; i++) for (let j = i + 1; j < bars.length; j++) {
      if (row[i] === row[j]) expect(bars[i]!.end <= bars[j]!.start || bars[j]!.end <= bars[i]!.start).toBe(true);
    }
  });
  it("uses the minimum number of sub-rows (the most bars open at one instant)", () => {
    // At t=25 three bars are open (0-100, 10-50, 20-30); at t=55, three (0-100, 50-90, 30-60).
    expect(packBars(bars).rows).toBe(3);
    expect(packBars([{ start: 0, end: 10 }, { start: 10, end: 20 }]).rows).toBe(1);
    expect(packBars([]).rows).toBe(1);
  });
  it("a lane named TBD or nothing is a placeholder", () => {
    expect(["TBD", "tbd ", "", "  "].map(placeholderLane)).toEqual([true, true, true, true]);
    expect(placeholderLane("gh-claude-5")).toBe(false);
  });
});
