import { describe, expect, it } from "vitest";
import { STAGE_ALLOWANCE_MS, ownerRole, laneLiveness, ownerLane, SLOW_FACTOR, SLOW_MIN_SAMPLES, activity, liveDuration, stageDuration, stageHealth, stageProgress, stageSince } from "./stage-health";
import { LIVENESS_MS, type Lane } from "./lane-bars";
import type { TaskState } from "./team-tasks";
import { parseSlots } from "./slots";

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
    expect(stageHealth(t, [lane("gh-claude-1", NOW - LIVENESS_MS)], [], NOW)?.flag).toBe("stalled");
    expect(stageHealth(t, [lane("gh-claude-1", NOW - LIVENESS_MS + 1)], [], NOW)?.flag).toBe("moving");
  });
  it("Blocked names the reviewer of the unanswered BLOCK", () => {
    const t = ts("a", { step: "review", blockedBy: { reviewer: "gh-claude-7", headSha: "x", commentUrl: "" } });
    expect(stageHealth(t, [], [], NOW)).toMatchObject({ flag: "blocked", text: "Blocked by gh-claude-7", tone: "orange" });
  });
  it(`Slow past ${SLOW_FACTOR}x the expected time: max(stage allowance, median of ${SLOW_MIN_SAMPLES}+ samples)`, () => {
    expect(SLOW_MIN_SAMPLES).toBe(3);
    expect(STAGE_ALLOWANCE_MS).toEqual({ plan: 30 * M, implement: 3 * H, fix: 2 * H, review: H, merge: 15 * M });
    const others = [done("o1", 4 * H), done("o2", 5 * H), done("o3", 6 * H)]; // median 5h > 3h allowance
    const at = (ms: number) => ts("a", { clock: { since: iso(NOW - ms), spent: {} } });
    expect(stageHealth(at(10 * H), [], [at(10 * H), ...others], NOW)?.flag).toBe("moving");
    expect(stageHealth(at(10 * H + 1000), [], [at(10 * H + 1000), ...others], NOW)?.flag).toBe("slow");
    // allowance wins over a small median
    const quick = [done("q1", M), done("q2", M), done("q3", M)];
    expect(stageHealth(at(6 * H), [], [at(6 * H), ...quick], NOW)?.flag).toBe("moving");
    expect(stageHealth(at(6 * H + 1000), [], [at(6 * H + 1000), ...quick], NOW)?.flag).toBe("slow");
    expect(stageHealth(at(10 * H), [], [others[0]!, others[1]!], NOW)?.flag).toBe("moving");
  });
  it("Plan: 43s and 11m are not slow against a 1s median; 61m with no samples is not slow", () => {
    const planDone = (k: string) => ts(k, { step: "merged", clock: { since: null, spent: { plan: 1000 } } });
    const plan = (ms: number) => ts("p", { step: "plan", clock: { since: iso(NOW - ms), spent: {} } });
    const g = [planDone("a"), planDone("b"), planDone("c")];
    expect(stageHealth(plan(43_000), [], g, NOW)?.flag).toBe("moving");
    expect(stageHealth(plan(11 * M), [], g, NOW)?.flag).toBe("moving");
    expect(stageHealth(plan(61 * M), [], [], NOW)?.flag).toBe("moving");
    expect(stageHealth(plan(60 * M + 1000), [], g, NOW)?.flag).toBe("slow");
  });
  it("Slow with a live owner names who is active", () => {
    const others = [done("o1", H), done("o2", H), done("o3", H)];
    const t = ts("a", { clock: { since: iso(NOW - 7 * H), spent: {} } });
    expect(stageHealth(t, [lane("gh-claude-1", NOW - 12 * M)], [t, ...others], NOW)).toMatchObject({ flag: "slow", text: "Slow · author active 12m ago", tone: "amber" });
    const r = ts("r", { step: "review", reviewers: ["gh-claude-7"], clock: { since: iso(NOW - 3 * H), spent: {} } });
    const rs = ["r1", "r2", "r3"].map((k) => ts(k, { step: "merged", clock: { since: null, spent: { review: M } } }));
    expect(stageHealth(r, [lane("gh-claude-7", NOW - 4 * M)], [r, ...rs], NOW)).toMatchObject({ flag: "slow", text: "Slow · reviewer active 4m ago" });
    expect(ownerRole(r)).toBe("reviewer");
    expect(ownerRole(t)).toBe("author");
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
  it("uses the stage allowance with no samples, caps the fill, keeps the raw pace", () => {
    const fix = ts("f", { step: "review", blockedBy: { reviewer: "r", headSha: "a", commentUrl: "" } });
    const p = stageProgress(fix, [fix, done("x", H)], NOW)!;
    expect(p).toEqual({ elapsedMs: H, expectedMs: STAGE_ALLOWANCE_MS.fix, ratio: 0.5, pace: 0.5, tone: "green" });
    expect(stageProgress(ts("m", { step: "merge" }), [], NOW)).toMatchObject({ ratio: 1, pace: 4, tone: "amber" });
    expect(stageProgress(done("d", H), [], NOW)).toBeNull();
  });
  it("green up to 1x, amber past it, never red", () => {
    const g = (since: number) => ts("a", { clock: { since: iso(NOW - since), spent: {} } });
    const group = [done("x", 4 * H), done("y", 4 * H), done("z", 4 * H)];
    expect(stageProgress(g(4 * H), group, NOW)).toMatchObject({ expectedMs: 4 * H, ratio: 1, tone: "green" });
    expect(stageProgress(g(4 * H + 1), group, NOW)!.tone).toBe("amber");
    for (const ms of [0, H, 4 * H, 40 * H, 400 * H]) expect(stageProgress(g(ms), group, NOW)!.tone).not.toBe("red");
  });
});

describe("activity", () => {
  it("green under LIVENESS_MS, red at it or with no lane (the Stalled rule)", () => {
    expect(activity("l", [lane("l", NOW - LIVENESS_MS + 1)], NOW)).toEqual({ sinceMs: LIVENESS_MS - 1, tone: "green" });
    expect(activity("l", [lane("l", NOW - LIVENESS_MS)], NOW).tone).toBe("red");
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
    const t = ts("a", { step: "review", headSha: "b", blockedBy: null /* #613: the fold cleared it */, clock: { since: iso(NOW - 5 * H), spent: {} },
      rounds: [{ reviewer: "gh-claude-8", headSha: "a", commentUrl: "", fixHead: "b", blockedAt: iso(NOW - 5 * H), fixedAt: iso(NOW - 3 * H) }] });
    expect(stageSince(t, [])).toBe(NOW - 3 * H);
    expect(stageProgress(t, [t], NOW)!.elapsedMs).toBe(3 * H);
    expect(stageHealth(t, [lane("gh-claude-8", NOW - M)], [t], NOW)?.flag).not.toBe("blocked");
    expect(ownerLane(t)).toBe("gh-claude-8");
  });
});

describe("#636 build-slot queue", () => {
  const slots = parseSlots({ slots: [{ root: "D:/gh", holder: { lane: "gh-claude-1", heldSeconds: 240, worktree: null }, waiting: [
    { lane: "x", waitedSeconds: 5 }, { lane: "gh-claude-2", waitedSeconds: 720, worktree: null }] }] });
  it("a building owner lane silent 9h reads building, never Stalled", () => {
    expect(stageHealth(ts("a"), [lane("gh-claude-1", NOW - 9 * H)], [], NOW, false, slots)).toMatchObject({ flag: "building", text: "building · 4m (D:/gh)", tone: "blue" });
  });
  it("a waiting owner lane reads waiting Nth, never Slow", () => {
    const t = ts("a", { lane: "gh-claude-2", clock: { since: iso(NOW - 100 * H), spent: {} } });
    const group = [done("p", M), done("q", M), done("r", M)];
    expect(stageHealth(t, [lane("gh-claude-2", NOW)], group, NOW)?.flag).toBe("slow");
    expect(stageHealth(t, [lane("gh-claude-2", NOW)], group, NOW, false, slots)).toMatchObject({ flag: "waiting_build", text: "waiting for build · 2nd · 12m" });
  });
  it("a queued lane counts as alive", () => {
    expect(laneLiveness("gh-claude-2", [lane("gh-claude-2", NOW - 9 * H)], [], NOW).live).toBe(false);
    expect(laneLiveness("gh-claude-2", [lane("gh-claude-2", NOW - 9 * H)], [], NOW, slots).live).toBe(true);
  });
});
