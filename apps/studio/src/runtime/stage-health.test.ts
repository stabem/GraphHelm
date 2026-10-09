import { describe, expect, it } from "vitest";
import { SLOW_FACTOR, SLOW_MIN_SAMPLES, stageDuration, stageHealth, stageSince } from "./stage-health";
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
  it("Stuck at exactly STALL_MS without a record, not one ms before", () => {
    const t = ts("a");
    expect(stageHealth(t, [lane("other", NOW - 9 * H, "a", NOW - H)], [], NOW)).toMatchObject({ flag: "stuck", text: "Stuck · no record 9h", tone: "red" });
    expect(stageHealth(t, [lane("gh-claude-1", NOW - STALL_MS)], [], NOW)?.flag).toBe("stuck");
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
