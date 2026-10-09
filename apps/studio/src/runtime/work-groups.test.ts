import { describe, expect, it } from "vitest";
import { buildWorkGroups, layoutWorkGroup, workStage } from "./work-groups";
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
  it("Fix: an unanswered BLOCK, or a fix pushed after a BLOCK awaiting re-review", () => {
    expect(workStage(task("a", { step: "review", blockedBy: { reviewer: "r", headSha: "a", commentUrl: "" }, rounds: [round(null)] }), false)).toBe("fix");
    expect(workStage(task("a", { step: "review", rounds: [round("b")] }), false)).toBe("fix");
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

describe("layoutWorkGroup", () => {
  it("stacks rows within a column and joins tasks in PR order", () => {
    const [g] = buildWorkGroups([
      task("c", { issue: 1, pr: 30, step: "review" }), task("a", { issue: 1, pr: 10, step: "merged" }), task("b", { issue: 1, pr: 20, step: "review" }),
    ], []);
    const l = layoutWorkGroup(g!);
    expect(Object.fromEntries(l.placed.map((p) => [p.task.key, [p.col, p.row]]))).toEqual({ a: [5, 0], b: [2, 0], c: [2, 1] });
    expect(l.edges).toEqual([{ from: "a", to: "b", done: true }, { from: "b", to: "c", done: false }]);
    expect(l.rows).toBe(2);
  });
});
