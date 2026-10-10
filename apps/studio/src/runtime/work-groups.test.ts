import { describe, expect, it } from "vitest";
import { buildWorkGroups, GRAPH_STAGES, graphCol, graphRow, prPath, workStage } from "./work-groups";
import type { TaskState } from "./team-tasks";
import type { JourneyRunView, JourneyView } from "./types";

const task = (key: string, over: Partial<TaskState> = {}): TaskState => ({
  key, taskId: key, branch: null, issue: null, pr: null, lane: "gh-claude-1", headSha: null, journeys: [], step: "implement",
  blockedBy: null, reviewers: [], mergeSha: null, repoUrl: null, strayVerdicts: [], title: null, summary: null, prTitle: null,
  prSummary: null, critic: null, recordedHeads: [], parent: null, rounds: [], clock: {} as TaskState["clock"], lastSequence: 0, ...over,
});
const round = (fixHead: string | null) => ({ reviewer: "r", headSha: "a", commentUrl: "", fixHead, blockedAt: null, fixedAt: null });

describe("workStage", () => {
  it("maps each step to its column", () => {
    expect(workStage(task("a"), false)).toBe("implement");
    expect(workStage(task("a", { step: "plan" }), false)).toBe("plan");
    expect(workStage(task("a", { step: "critic" }), false)).toBe("plan");
    expect(workStage(task("a", { step: "review" }), false)).toBe("review");
    expect(workStage(task("a", { step: "merge" }), false)).toBe("merge");
    expect(workStage(task("a", { step: "merged" }), false)).toBe("merged");
    expect(workStage(task("a", { step: "merged" }), true)).toBe("proven");
  });
  it("Fix: an unanswered BLOCK; once the fix is pushed the work waits in Review (#591)", () => {
    expect(workStage(task("a", { step: "review", blockedBy: { reviewer: "r", headSha: "a", commentUrl: "" }, rounds: [round(null)] }), false)).toBe("fix");
    expect(workStage(task("a", { step: "review", rounds: [round("b")] }), false)).toBe("review");
    expect(workStage(task("a", { step: "merge", rounds: [round("b")] }), false)).toBe("merge");
  });
});

describe("buildWorkGroups", () => {
  it("groups by issue with the issue title, PR order and the union of journeys", () => {
    const groups = buildWorkGroups([
      task("p2", { issue: 519, pr: 578, title: "Watch plays", journeys: ["watch"] }),
      task("p1", { issue: 519, pr: 548, title: "Watch plays", journeys: ["proof", "watch"] }),
      task("q", { issue: 600, pr: 601 }),
    ], []);
    expect(groups.map((g) => [g.key, g.label, g.tasks.map((t) => t.pr)])).toEqual([
      ["issue-519", "#519 Watch plays", [548, 578]], ["issue-600", "Issue #600", [601]],
    ]);
    expect(groups[0]!.journeyIds).toEqual(["proof", "watch"]);
  });
  it("tasks with no issue share one No issue group", () => {
    const groups = buildWorkGroups([task("a", { pr: 1 }), task("b", { pr: 2 })], []);
    expect(groups).toHaveLength(1);
    expect(groups[0]).toMatchObject({ key: "no-issue", issue: null, label: "No issue" });
  });
  it("open work first, then the most recent", () => {
    const groups = buildWorkGroups([
      task("done", { issue: 1, step: "merged", lastSequence: 99 }),
      task("old", { issue: 2, lastSequence: 5 }),
      task("new", { issue: 3, lastSequence: 50 }),
    ], []);
    expect(groups.map((g) => g.issue)).toEqual([3, 2, 1]);
  });
  it("Proven only when the linked journey's replay passed the step; else Merged", () => {
    const journey: JourneyView = { contractId: "watch", title: "Watch", arrows: [],
      steps: [{ stepId: "s1", screen: { screenId: "s1", title: "S1", scopePaths: [] }, promises: [] }] };
    const tasks = [task("m", { issue: 1, pr: 5, step: "merged", journeys: ["watch"] })];
    const pass: JourneyRunView = { state: "ready", kind: "replay", screens: { s1: { result: "pass" } }, edges: {} } as unknown as JourneyRunView;
    expect(buildWorkGroups(tasks, [journey], () => pass)[0]!.stages.m).toBe("proven");
    expect(buildWorkGroups(tasks, [journey], () => null)[0]!.stages.m).toBe("merged");
    expect(buildWorkGroups(tasks, [journey])[0]!.stages.m).toBe("merged");
  });
});

describe("prPath (#591)", () => {
  const blocked = { reviewer: "gh-claude-7", headSha: "a", commentUrl: "" };
  const r1 = { reviewer: "gh-claude-7", headSha: "a", commentUrl: "", fixHead: "b", blockedAt: null, fixedAt: null };
  const cells = (t: TaskState, proven = false) => prPath(t, proven).cells.map((c) => [c.stage, c.state, c.label, c.mark]);
  it("BLOCK -> fix -> re-review -> approve -> merged, done stages included", () => {
    const base = { pr: 7, plan: { summary: "p" } as TaskState["plan"], reviewers: ["gh-claude-7"] };
    expect(cells(task("a", { ...base, step: "review", blockedBy: blocked, rounds: [{ ...r1, fixHead: null }] }))).toEqual([
      ["plan", "done", "Plan", "✓"], ["implement", "done", "Implement", "✓"], ["review", "block", "Review", "BLOCK"], ["fix", "current", "Fix", null],
      ["merge", "ahead", "Merge", null], ["merged", "ahead", "Merged", null], ["proven", "ahead", "Proven", null],
    ]);
    const open = prPath(task("a", { ...base, lane: "gh-claude-1", step: "review", blockedBy: blocked, rounds: [{ ...r1, fixHead: null }] }), false);
    expect(open.cells[2]).toMatchObject({ who: "by gh-claude-7", mark: "BLOCK" });
    expect(open.cells[3]).toMatchObject({ state: "current", title: "Fixing", who: "gh-claude-1", sub: "after BLOCK by gh-claude-7" });
    expect(open.edges).toEqual([{ row: "a", from: 0, to: 1, kind: "done" }, { row: "a", from: 1, to: 2, kind: "done" }, { row: "a", from: 2, to: 3, kind: "next" }]);
    const pushed = prPath(task("a", { ...base, step: "review", rounds: [r1] }), false);
    expect(pushed.cells.slice(2, 4).map((c) => [c.stage, c.state, c.label, c.mark, c.title])).toEqual([
      ["review", "current", "Re-review", null, "Re-review"], ["fix", "done", "Fix", "fix pushed b", null]]);
    expect(pushed.edges).toContainEqual({ row: "a", from: 3, to: 2, kind: "next" });
    expect(pushed.edges.filter((e) => e.kind === "next")).toHaveLength(1);
    const done = task("a", { ...base, step: "merged", mergeSha: "9a9a9a9a11", rounds: [r1] });
    expect(cells(done)).toEqual([
      ["plan", "done", "Plan", "✓"], ["implement", "done", "Implement", "✓"], ["review", "done", "Review ×2", "✓"], ["fix", "done", "Fix", "✓"],
      ["merge", "done", "Merge", "✓"], ["merged", "current", "Merged", "✓"], ["proven", "ahead", "Proven", null],
    ]);
    expect(prPath(done, false).edges).toContainEqual({ row: "a", from: 3, to: 2, kind: "done" });
    expect(cells(done, true).slice(-1)).toEqual([["proven", "current", "Proven", "✓"]]);
  });
  it("an unrecorded plan the work moved past reads skipped; no BLOCK draws no Fix cell", () => {
    const c = cells(task("a", { step: "merge", reviewers: ["r"] }));
    expect(c[0]).toEqual(["plan", "done", "Plan", "skipped"]);
    expect(c.map((x) => x[0])).not.toContain("fix");
  });
  it("arrows stay inside their row, left to right except the Fix loop back to Review", () => {
    const [g] = buildWorkGroups([
      task("x", { issue: 1, pr: 1, step: "merged", rounds: [r1], mergeSha: "m" }), task("y", { issue: 1, pr: 2, step: "review", blockedBy: blocked, rounds: [r1] }),
      task("z", { issue: 1, pr: 3, step: "implement" }),
    ], []);
    for (const row of g!.rows) {
      const cols = new Set(row.cells.map((c) => c.col));
      for (const e of row.edges) {
        expect(e.row).toBe(row.key);
        expect(cols.has(e.from) && cols.has(e.to)).toBe(true);
        if (e.to < e.from) expect([e.from, e.to]).toEqual([3, 2]);
      }
    }
    expect(g!.rows.flatMap((r) => r.edges).some((e) => !g!.rows.find((r) => r.key === e.row)!.edges.includes(e))).toBe(false);
  });
  it("rows: open work first, most urgent first, merged last", () => {
    const [g] = buildWorkGroups([
      task("m", { issue: 1, pr: 1, step: "merged", lastSequence: 99 }), task("w", { issue: 1, pr: 2, step: "implement", lastSequence: 5 }),
      task("n", { issue: 1, pr: 3, step: "review", lastSequence: 9 }), task("b", { issue: 1, pr: 4, step: "review", blockedBy: blocked, lastSequence: 1 }),
    ], []);
    expect(g!.rows.map((r) => [r.key, r.open])).toEqual([["b", true], ["n", true], ["w", true], ["m", false]]);
  });
});

describe("graphRow (#706): five display columns over prPath's seven stages", () => {
  const blocked = { reviewer: "gh-claude-7", headSha: "a", commentUrl: "" };
  const r1 = { reviewer: "gh-claude-7", headSha: "a", commentUrl: "", fixHead: "b", blockedAt: null, fixedAt: null };
  const plan = { plan: { summary: "p" } as TaskState["plan"], reviewers: ["gh-claude-7"] };
  const view = (t: TaskState, proven = false) => graphRow(prPath(t, proven));
  it("draws Implement · Review · Fix · Merge · Merged; Plan and Proven map onto them", () => {
    expect(GRAPH_STAGES.map((s) => s.label)).toEqual(["Implement", "Review", "Fix", "Merge", "Merged"]);
    expect((["plan", "implement", "review", "fix", "merge", "merged", "proven"] as const).map(graphCol)).toEqual([0, 0, 1, 2, 3, 4, 4]);
  });
  it("BLOCK -> Fixing keeps its cells and arrows, shifted one column; a recorded plan is the title tag", () => {
    const g = view(task("a", { ...plan, pr: 7, step: "review", blockedBy: blocked, rounds: [{ ...r1, fixHead: null }] }));
    expect(g.planTag).toBe("plan ✓");
    expect(g.cells.map((c) => [c.stage, c.col, c.state])).toEqual([
      ["implement", 0, "done"], ["review", 1, "block"], ["fix", 2, "current"], ["merge", 3, "ahead"], ["merged", 4, "ahead"]]);
    expect(g.edges).toEqual([{ row: "a", from: 0, to: 1, kind: "done" }, { row: "a", from: 1, to: 2, kind: "next" }]);
  });
  it("a pushed fix loops Fix back to the Re-review, inside the row", () => {
    const g = view(task("a", { ...plan, step: "review", rounds: [r1] }));
    expect(g.edges).toContainEqual({ row: "a", from: 2, to: 1, kind: "next" });
    const cols = new Set(g.cells.map((c) => c.col));
    for (const e of g.edges) expect(cols.has(e.from) && cols.has(e.to)).toBe(true);
  });
  it("a current Plan is the Planning card in Implement; a skipped plan has no tag", () => {
    const g = view(task("a", { step: "plan" }));
    expect(g.planTag).toBe("planning");
    expect(g.cells.filter((c) => c.col === 0).map((c) => [c.stage, c.state, c.label])).toEqual([["plan", "current", "Planning"]]);
    expect(g.edges).toEqual([]);
    expect(view(task("b", { step: "merge", reviewers: ["r"] })).planTag).toBeNull();
  });
  it("Proven is a badge on the Merged cell, never its own column", () => {
    const done = task("a", { ...plan, step: "merged", mergeSha: "9a9a9a9a11" });
    const g = view(done, true);
    expect(g.proven).toBe(true);
    expect(g.cells.map((c) => c.col)).toEqual([0, 1, 3, 4]);
    expect(g.cells.find((c) => c.stage === "merged")).toMatchObject({ col: 4, badge: "proven ✓", state: "done" });
    expect(g.edges.every((e) => e.to <= 4)).toBe(true);
    expect(view(done, false).cells.find((c) => c.stage === "merged")!.badge).toBeUndefined();
  });
});

describe("a pushed fix after a BLOCK (#591, PR #581's shape)", () => {
  // The newer pr_opened head (66613c95) answers the BLOCK on 1a1a1a1a: since #613 the fold clears
  // `blockedBy`, and the BLOCK lives on in `rounds` with its fixHead.
  const t581 = task("pr-581", {
    issue: 549, pr: 581, lane: "gh-claude-11", step: "review", reviewers: ["gh-claude-8"], headSha: "66613c95aa",
    blockedBy: null, recordedHeads: ["1a1a1a1a00", "66613c95aa"],
    rounds: [{ reviewer: "gh-claude-8", headSha: "1a1a1a1a00", commentUrl: "", fixHead: "66613c95aa", blockedAt: "2026-10-09T06:30:00Z", fixedAt: "2026-10-09T09:00:00Z" }],
  });
  it("sits in Review, not Fix", () => expect(workStage(t581, false)).toBe("review"));
  it("draws BLOCK by the reviewer, a done Fix with the pushed sha, and a current Re-review waiting on the reviewer", () => {
    const p = prPath(t581, false);
    const review = p.cells.find((c) => c.stage === "review")!, fix = p.cells.find((c) => c.stage === "fix")!;
    expect(review).toMatchObject({ state: "current", title: "Re-review", who: "gh-claude-8", sub: "waiting on gh-claude-8", mark: null });
    expect(fix).toMatchObject({ state: "done", mark: "fix pushed 66613c95" });
    expect(p.cells.some((c) => c.title === "Fixing")).toBe(false);
  });
  it("an unchanged head keeps the Fixing card", () => {
    expect(workStage({ ...t581, headSha: "1a1a1a1a00", blockedBy: { reviewer: "gh-claude-8", headSha: "1a1a1a1a00", commentUrl: "" }, rounds: [{ ...t581.rounds[0]!, fixHead: null }] }, false)).toBe("fix");
  });
});
