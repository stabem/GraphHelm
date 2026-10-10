import { describe, expect, it } from "vitest";
import { agentBoard, span } from "./agent-board";
import type { Bot } from "./team";
import { laneBars, type Lane, type TimedTaskEvent } from "./lane-bars";
import { STALE_CLAIM_MS } from "./agent-board";
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

  it("the open bar gives PR, title and link; the time is the lane's latest record", () => {
    const l = { ...lane("a", [bar("implement", now - 77 * M)]), lastRecord: { kind: "planned", issue: 7, at: now - 5 * M } };
    const [r] = agentBoard([bot("a")], [l], [task({})], now);
    expect(r).toMatchObject({ stage: "implement", pr: 7, title: "Add board", href: "https://github.com/o/r/pull/7", forMs: 5 * M, latest: "planned #7 · 5m ago", stageMs: 77 * M });
    expect(span(77 * M)).toBe("1h 17m");
  });

  it("a lane with no record has no time", () => {
    const rows = agentBoard([bot("d"), bot("e")], [lane("d", [bar("review", now - 60 * M, false, now - 23 * M)])], [], now);
    expect(rows.find((r) => r.name === "e")!.forMs).toBeNull();
    expect(rows.find((r) => r.name === "d")!.latest).toBeNull();
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
  describe("reviewing", () => {
    const pr = (over: Partial<MissionTask>) => task({ key: "t9", pr: 9, title: "Fix it", lane: "a", step: "review", headSha: "h2", ...over });
    it("a lane assigned as reviewer on an open PR is Reviewing, not Free", () => {
      const [r] = agentBoard([bot("rev")], [lane("rev", [])], [pr({ reviewers: ["rev"] })], now);
      expect(r).toMatchObject({ status: "waiting", label: "Reviewing", doing: "reviewing #9 Fix it", pr: 9, forMs: null });
    });
    it("time is since the reviewer's latest record", () => {
      const l = { ...lane("rev", [bar("review", now - 12 * M, true, now, "#9")]), lastRecord: { kind: "review_assigned", pr: 9, at: now - 12 * M } };
      const [r] = agentBoard([bot("rev")], [l], [pr({ reviewers: ["rev"] })], now);
      expect(r).toMatchObject({ status: "waiting", label: "Reviewing", forMs: 12 * M, latest: "review_assigned #9 · 12m ago" });
    });
    it("Working wins when the lane also implements", () => {
      const [r] = agentBoard([bot("a")], [lane("a", [bar("implement", now - 5 * M)])], [pr({ reviewers: ["a"] })], now);
      expect(r!.status).toBe("working");
      expect(r!.label ?? null).toBeNull();
    });
    it("a merged PR does not keep the reviewer busy", () => {
      const [r] = agentBoard([bot("rev")], [lane("rev", [])], [pr({ reviewers: ["rev"], step: "merged" })], now);
      expect(r!.status).toBe("free");
    });
    it("a verdict on the current head makes the lane Free again; a new head reopens it", () => {
      const round = { reviewer: "rev", headSha: "h2", fixHead: null };
      const [done] = agentBoard([bot("rev")], [lane("rev", [])], [pr({ reviewers: ["rev"], rounds: [round] })], now);
      expect(done!.status).toBe("free");
      const [blocked] = agentBoard([bot("rev")], [lane("rev", [])], [pr({ reviewers: ["rev"], blocked: true, blockedBy: { reviewer: "rev", headSha: "h2" } })], now);
      expect(blocked!.status).toBe("free");
      const [again] = agentBoard([bot("rev")], [lane("rev", [])], [pr({ reviewers: ["rev"], headSha: "h3", rounds: [round] })], now);
      expect(again!.status).toBe("waiting");
    });
  });

  it("never lists TBD or empty names", () => {
    const rows = agentBoard([bot("TBD"), bot("tbd"), bot(""), bot("ok")], [lane("Tbd", []), lane(" ", []), lane("real", [])],
      [task({ reviewers: ["TBD"], step: "review" })], now);
    expect(rows.map((r) => r.name).sort()).toEqual(["ok", "real"]);
  });
});

describe("#591 board times the latest activity (record shapes)", () => {
  const H = 3_600_000;
  const T0 = Date.parse("2026-10-08T00:00:00Z");
  const iso = (ms: number) => new Date(T0 + ms).toISOString();
  let seq = 0;
  const ev = (e: Partial<TimedTaskEvent>): TimedTaskEvent => ({ actorId: "rt", sequence: seq++, taskId: "issue-600", ...e }) as TimedTaskEvent;
  const board = (events: TimedTaskEvent[], at: number, names: string[]) => {
    const lanes = laneBars(events, T0 + at, 48 * H);
    return Object.fromEntries(agentBoard(names.map(bot), lanes, [], T0 + at).map((r) => [r.name, r]));
  };

  it("a slice ends when its PR merges, even with the issue open (Refs #N): the lane is Free", () => {
    const events = [
      ev({ kind: "task.claimed", lane: "gh-claude-3", issue: 600, at: iso(0) }),
      ev({ kind: "task.pr_opened", lane: "gh-claude-3", issue: 600, pr: 615, headSha: "aaa", at: iso(1 * H) }),
      ev({ kind: "task.review_assigned", reviewer: "gh-claude-5", pr: 615, at: iso(2 * H) }),
      ev({ kind: "task.review_verdict", reviewer: "gh-claude-5", verdict: "APPROVE" as never, pr: 615, headSha: "aaa", at: iso(3 * H) }),
      // The merge record is keyed by the PR, not by the issue's task id.
      ev({ kind: "task.merged", taskId: "pr-615", pr: 615, mergeSha: "mmm", at: iso(4 * H) }),
    ];
    const rows = board(events, 25 * H, ["gh-claude-3", "gh-claude-5"]);
    expect(rows["gh-claude-3"]!.status).toBe("free");
    expect(rows["gh-claude-5"]!.status).toBe("free");
  });

  it("a second claim of the same lane on the issue ends when that lane's PR merges", () => {
    const events = [
      ev({ kind: "task.claimed", lane: "gh-claude-3", issue: 600, at: iso(0) }),
      ev({ kind: "task.claimed", lane: "gh-claude-3", issue: 600, at: iso(0.5 * H) }),
      ev({ kind: "task.pr_opened", lane: "gh-claude-3", issue: 600, pr: 615, headSha: "aaa", at: iso(1 * H) }),
      ev({ kind: "task.merged", pr: 615, at: iso(2 * H) }),
    ];
    expect(board(events, 3 * H, ["gh-claude-3"])["gh-claude-3"]!.status).toBe("free");
  });

  it.each(["closes", "issue", "pr"] as const)("another lane's merge only ends a claim through closes: %s, preserving lastDelivered", (match) => {
    const events = [
      ev({ kind: "task.claimed", taskId: "issue-549", lane: "codex-3", issue: 549, at: iso(0) }),
      ev({ kind: "task.claimed", taskId: "issue-700", lane: "other", issue: match === "pr" ? 549 : 700, at: iso(M) }),
      ev({ kind: "task.pr_opened", taskId: "issue-700", lane: "other", pr: 701, at: iso(2 * M) }),
      ev({ kind: "task.merged", taskId: "pr-701", actorId: "reviewer", pr: 701,
        closes: match === "closes" ? [549] : [], ...(match === "issue" ? { issue: 549 } : {}), at: iso(3 * M) }),
    ];
    const delivered = task({ lane: "codex-3", step: "merged", title: "Previous delivery" });
    for (const at of [3 * H, 4 * M]) {
      const lanes = laneBars(events, T0 + at, 48 * H);
      const row = agentBoard([bot("codex-3")], lanes, [delivered], T0 + at).find((r) => r.name === "codex-3");
      const closed = match === "closes";
      expect(row).toMatchObject({ status: closed ? "free" : at === 3 * H ? "stale" : "working",
        stage: closed || at === 3 * H ? null : "implement", lastDelivered: "#7 Previous delivery" });
      expect(lanes.find((l) => l.lane === "codex-3")!.bars[0]).toMatchObject({ open: !closed, end: T0 + (closed ? 3 * M : at) });
    }
    events.push(ev({ kind: "task.claimed", taskId: "issue-702", lane: "codex-3", issue: 702, at: iso(3 * H) }));
    expect(board(events, 3 * H + M, ["codex-3"])["codex-3"]!.status).toBe("working");
  });

  it.each(["issue-86-tree", "issue-86-summary"])("a Refs slice by codex-4 keeps codex-1's PR-less claim working: %s", (taskId) => {
    const events = [
      ev({ kind: "task.claimed", taskId, lane: "codex-1", issue: 86, at: iso(0) }),
      ev({ kind: "task.claimed", taskId: "issue-86-summary", lane: "codex-4", issue: 86, at: iso(M) }),
      ev({ kind: "task.pr_opened", taskId: "issue-86-summary", lane: "codex-4", issue: 86, pr: 658, at: iso(2 * M) }),
      ev({ kind: "task.merged", taskId: "issue-86-summary", pr: 658, issue: 86, closes: [], at: iso(3 * M) }),
    ];
    const rows = board(events, 4 * M, ["codex-1", "codex-4"]);
    expect(rows["codex-1"]).toMatchObject({ status: "working", stage: "implement" });
    expect(rows["codex-4"]!.status).toBe("free");
  });

  it("the time is since the lane's latest record, naming that record", () => {
    const events = [
      ev({ kind: "task.claimed", lane: "gh-claude-3", issue: 600, at: iso(0) }),
      ev({ kind: "task.pr_opened", lane: "gh-claude-3", issue: 600, pr: 613, headSha: "aaa", at: iso(10 * H) }),
      ev({ kind: "task.review_assigned", reviewer: "gh-claude-5", pr: 613, at: iso(10 * H + 2 * M) }),
      ev({ kind: "task.review_verdict", reviewer: "gh-claude-5", verdict: "BLOCK" as never, pr: 613, headSha: "aaa", at: iso(10 * H + 9 * M) }),
    ];
    const rows = board(events, 10 * H + 12 * M, ["gh-claude-3", "gh-claude-5"]);
    expect(rows["gh-claude-3"]).toMatchObject({ latest: "pr_opened #613 · 12m ago", forMs: 12 * M });
    expect(rows["gh-claude-5"]).toMatchObject({ latest: "review_verdict #613 · 3m ago", forMs: 3 * M });
  });

  it("a claim with no PR and no record for over 2h reads stale claim · #issue", () => {
    const events = [ev({ kind: "task.claimed", lane: "gh-claude-3", issue: 600, at: iso(0) })];
    expect(board(events, STALE_CLAIM_MS - M, ["gh-claude-3"])["gh-claude-3"]!.status).not.toBe("stale");
    const r = board(events, STALE_CLAIM_MS + M, ["gh-claude-3"])["gh-claude-3"]!;
    expect(r).toMatchObject({ status: "stale", label: "stale claim · #600" });
  });

  it("after pr_opened the author is awaiting review #N, not Free, until a verdict or the merge", () => {
    const open = [
      ev({ kind: "task.claimed", lane: "gh-claude-3", issue: 600, at: iso(0) }),
      ev({ kind: "task.pr_opened", lane: "gh-claude-3", issue: 600, pr: 613, headSha: "aaa", at: iso(1 * H) }),
    ];
    expect(board(open, 1 * H + M, ["gh-claude-3"])["gh-claude-3"]).toMatchObject({ status: "awaiting", label: "awaiting review #613", pr: 613 });
    const assigned = [...open, ev({ kind: "task.review_assigned", reviewer: "gh-claude-5", pr: 613, at: iso(1 * H + 2 * M) })];
    expect(board(assigned, 1 * H + 5 * M, ["gh-claude-3"])["gh-claude-3"]!.status).toBe("awaiting");
    const merged = [...assigned, ev({ kind: "task.merged", pr: 613, at: iso(2 * H) })];
    expect(board(merged, 2 * H + M, ["gh-claude-3"])["gh-claude-3"]!.status).toBe("free");
  });
});
