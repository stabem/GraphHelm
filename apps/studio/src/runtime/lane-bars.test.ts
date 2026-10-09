import { describe, expect, it } from "vitest";
import { laneBars, STALL_MS, type TimedTaskEvent } from "./lane-bars";

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
});
