import { describe, expect, it } from "vitest";
import { ACTIVE_MS, EXPECTED_FALLBACK_MS, LIVENESS_MS, laneLiveness, ownerLane, SLOW_FACTOR, SLOW_MIN_SAMPLES, activity, liveDuration, stageDuration, stageHealth, stageProgress, stageSince } from "./stage-health";
import { STALL_MS, type Lane } from "./lane-bars";
import type { TaskState } from "./team-tasks";

const H = 3_600_000, M = 60_000;
const NOW = Date.parse("2026-10-09T12:00:00Z");
const iso = (ms: number) => new Date(ms).toISOString();
const ts = (key: string, over: Partial<TaskState> = {}): TaskState => ({
  key, taskId: key, branch: null, issue: 1, pr: 1, lane: "gh-claude-1", headSha: null, journeys: [], step: "implement",
  blockedBy: null, reviewers: [], mergeSha: null, repoUrl: null, strayVerdicts: [], title: "T", summary: null, prTitle: null,
  prSummary: null, critic: null, recordedHeads: [], parent: null, rounds: [], clock: { since: iso(NOW - H), spent: {} }, lastSequence: 0, ...over,
});
const lane = (name: string, lastEventAt: number, taskId?: string, since?: number): Lane => ({
  lane: name, silent: false, lastEventAt,
  bars: taskId ? [{ kind: "implement", label: taskId, start: since ?? 0, end: NOW, open: true, taskId, since: since ?? 0 }] : [],
});
const done = (key: string, implementMs: number) => ts(key, { step: "merged", clock: { since: null, spent: { implement: implementMs } } });

describe("stageDuration", () => {
  it("formats seconds, minutes, hours and days", () => {
    expect(stageDuration(-5)).toBe("0s");
    expect(stageDuration(59_999)).toBe("59s");
    expect(stageDuration(M)).toBe("1m");
    expect(stageDuration(59 * M)).toBe("59m");
    expect(stageDuration(H)).toBe("1h");
    expect(stageDuration(H + 17 * M)).toBe("1h 17m");
    expect(stageDuration(24 * H)).toBe("1d");
    expect(stageDuration(52 * H)).toBe("2d 4h");
  });
});

describe("stageHealth", () => {
  it("merged work has no flag", () => expect(stageHealth(done("m", H), [], [], NOW)).toBeNull());
  it("times the stage from the step clock, else from the lane bar start", () => {
    expect(stageHealth(ts("a", { clock: { since: iso(NOW - H - 17 * M), spent: {} } }), [], [], NOW)?.elapsed).toBe("1h 17m");
    const t = ts("b", { clock: { since: null, spent: {} } });
    expect(stageSince(t, [lane("gh-claude-1", NOW, "b", NOW - 2 * H)])).toBe(NOW - 2 * H);
    expect(stageHealth(t, [], [], NOW)?.elapsed).toBeNull();
  });
  it("Needs you wins over everything", () => {
    const t = ts("a", { blockedBy: { reviewer: "r", headSha: "x", commentUrl: "" } });
    expect(stageHealth(t, [lane("gh-claude-1", NOW - 10 * H)], [], NOW, true)).toMatchObject({ flag: "needs_you", text: "Needs you", tone: "red" });
  });
  it("Stalled at exactly LIVENESS_MS without a record from the owner lane, not one ms before", () => {
    const t = ts("a");
    expect(stageHealth(t, [lane("gh-claude-1", NOW - 9 * H)], [], NOW)).toMatchObject({ flag: "stalled", text: "gh-claude-1 silent 9h", tone: "red" });
    expect(stageHealth(t, [lane("gh-claude-1", NOW - STALL_MS)], [], NOW)?.flag).toBe("stalled");
    expect(stageHealth(t, [lane("gh-claude-1", NOW - STALL_MS + 1)], [], NOW)?.flag).toBe("moving");
  });
  it("Blocked names the reviewer of the unanswered BLOCK", () => {
    const t = ts("a", { step: "review", blockedBy: { reviewer: "gh-claude-7", headSha: "x", commentUrl: "" } });
    expect(stageHealth(t, [], [], NOW)).toMatchObject({ flag: "blocked", text: "Blocked by gh-claude-7", tone: "orange" });
  });
  it(`Slow past ${SLOW_FACTOR}x the group's median for the stage, with at least ${SLOW_MIN_SAMPLES} samples`, () => {
    const others = [done("o1", 20 * M), done("o2", 40 * M)]; // median 30m, limit 60m
    const at = (ms: number) => ts("a", { clock: { since: iso(NOW - ms), spent: {} } });
    expect(stageHealth(at(H), [], [at(H), ...others], NOW)?.flag).toBe("moving");
    expect(stageHealth(at(H + 1000), [], [at(H + 1000), ...others], NOW)).toMatchObject({ flag: "slow", text: "Slow", tone: "amber" });
    expect(stageHealth(at(10 * H), [], [others[0]!], NOW)?.flag).toBe("moving");
  });
  it("Moving otherwise, in muted green", () => expect(stageHealth(ts("a"), [], [], NOW)).toMatchObject({ flag: "moving", text: "Moving", tone: "green" }));
});

describe("liveDuration", () => {
  it("ticks seconds, pads under an hour", () => {
    expect(liveDuration(12_000)).toBe("12s");
    expect(liveDuration(4 * M + 3000)).toBe("4m 03s");
    expect(liveDuration(5 * H + 28 * M + 12_000)).toBe("5h 28m 12s");
  });
});

describe("stageProgress", () => {
  it("falls back to the named stage constant under two samples, and caps the fill", () => {
    const fix = ts("f", { step: "review", blockedBy: { reviewer: "r", headSha: "a", commentUrl: "" } });
    const p = stageProgress(fix, [fix, done("x", H)], NOW)!;
    expect(p).toEqual({ elapsedMs: H, expectedMs: EXPECTED_FALLBACK_MS.fix, ratio: 0.5, tone: "green" });
    expect(stageProgress(ts("m", { step: "merge" }), [], NOW)).toMatchObject({ ratio: 1, tone: "red" });
    expect(stageProgress(done("d", H), [], NOW)).toBeNull();
  });
  it("uses the group median: green under 75%, amber to 100%, red at 100%", () => {
    const g = (since: number) => ts("a", { clock: { since: iso(NOW - since), spent: {} } });
    const group = [done("x", 4 * H), done("y", 4 * H)];
    expect(stageProgress(g(2.99 * H), group, NOW)!.tone).toBe("green");
    expect(stageProgress(g(3 * H), group, NOW)!.tone).toBe("amber");
    expect(stageProgress(g(4 * H - 1), group, NOW)!.tone).toBe("amber");
    expect(stageProgress(g(4 * H), group, NOW)).toMatchObject({ expectedMs: 4 * H, ratio: 1, tone: "red" });
  });
});

describe("activity", () => {
  it("green under LIVENESS_MS, red at it or with no lane (the Stalled rule)", () => {
    expect(activity("l", [lane("l", NOW - ACTIVE_MS + 1)], NOW)).toEqual({ sinceMs: ACTIVE_MS - 1, tone: "green" });
    expect(activity("l", [lane("l", NOW - ACTIVE_MS)], NOW).tone).toBe("red");
    expect(activity("z", [lane("l", NOW)], NOW)).toEqual({ sinceMs: null, tone: "red" });
  });
});

describe("laneLiveness (#591)", () => {
  it("29m59s since the lane's last record is live, 30m is stalled", () => {
    expect(laneLiveness("l", [lane("l", NOW - LIVENESS_MS + 1000)], [], NOW)).toEqual({ live: true, sinceMs: LIVENESS_MS - 1000 });
    expect(laneLiveness("l", [lane("l", NOW - LIVENESS_MS)], [], NOW)).toEqual({ live: false, sinceMs: LIVENESS_MS });
    expect(LIVENESS_MS).toBe(30 * M);
  });
  it("a record the lane left on another task keeps it alive", () => {
    const other = ts("other", { lane: "l", rounds: [{ reviewer: "r", headSha: "a", commentUrl: "", fixHead: "b", blockedAt: null, fixedAt: iso(NOW - 5 * M) }] });
    expect(laneLiveness("l", [lane("l", NOW - 2 * H)], [ts("mine", { lane: "l" }), other], NOW)).toEqual({ live: true, sinceMs: 5 * M });
  });
  it("an implementing lane silent 1h 10m reads Stalled, red, naming the lane", () => {
    const t = ts("a", { lane: "gh-claude-9" });
    expect(stageHealth(t, [lane("gh-claude-9", NOW - 70 * M)], [t], NOW)).toMatchObject({ flag: "stalled", text: "gh-claude-9 silent 1h 10m", tone: "red" });
    expect(activity("gh-claude-9", [lane("gh-claude-9", NOW - 70 * M)], NOW).tone).toBe("red");
  });
  it("a review waiting on a silent reviewer is stalled; the author's silence does not count there", () => {
    const t = ts("a", { step: "review", lane: "gh-claude-1", reviewers: ["gh-claude-7"] });
    const lanes = [lane("gh-claude-1", NOW - 5 * H), lane("gh-claude-7", NOW - 40 * M)];
    expect(stageHealth(t, lanes, [t], NOW)).toMatchObject({ flag: "stalled", text: "gh-claude-7 silent 40m" });
    expect(ownerLane(t)).toBe("gh-claude-7");
  });
  it("a pushed fix times its Re-review from the push, not from the BLOCK", () => {
    const t = ts("a", { step: "review", headSha: "b", blockedBy: { reviewer: "gh-claude-8", headSha: "a", commentUrl: "" }, clock: { since: iso(NOW - 5 * H), spent: {} },
      rounds: [{ reviewer: "gh-claude-8", headSha: "a", commentUrl: "", fixHead: "b", blockedAt: iso(NOW - 5 * H), fixedAt: iso(NOW - 3 * H) }] });
    expect(stageSince(t, [])).toBe(NOW - 3 * H);
    expect(stageProgress(t, [t], NOW)!.elapsedMs).toBe(3 * H);
    expect(stageHealth(t, [lane("gh-claude-8", NOW - M)], [t], NOW)?.flag).not.toBe("blocked");
    expect(ownerLane(t)).toBe("gh-claude-8");
  });
});
