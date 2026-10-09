import { describe, expect, it } from "vitest";
import { buildMission, custodyRows, unlinkedTasks, TRUST_LABELS } from "./mission";
import type { JourneyView, JourneyRunView } from "./types";
import { foldTaskEvents, parseTaskEvent, type TaskState } from "./team-tasks";

const journey: JourneyView = {
  contractId: "watch",
  title: "Watch plays inside the Studio",
  arrows: [],
  steps: [
    { stepId: "open", screen: { screenId: "open", title: "Open a journey", scopePaths: [] }, promises: [] },
    { stepId: "watch", screen: { screenId: "watch", title: "Watch the page live", scopePaths: [] }, promises: [] },
    { stepId: "mark", screen: { screenId: "mark", title: "Mark a skipped step safe", scopePaths: [] }, promises: [] },
  ],
};

function task(over: Partial<TaskState>): TaskState {
  return {
    key: "k", taskId: "t", branch: null, issue: 519, pr: 1, lane: "gh-claude-1", headSha: "a",
    journeys: ["watch"], step: "implement", blockedBy: null, reviewers: [], mergeSha: null, repoUrl: null,
    strayVerdicts: [], title: "t", summary: null, prTitle: "PR title", prSummary: null, critic: null,
    ...over,
  } as TaskState;
}

describe("buildMission", () => {
  it("no run: every step reads not_run, nothing proven", () => {
    const m = buildMission(journey, null, []);
    expect(m.steps.map((s) => s.status)).toEqual(["not_run", "not_run", "not_run"]);
    expect(m.summary).toEqual({ proven: 0, total: 3, inFlight: 0, needYou: 0, readyUnclaimed: 0 });
  });

  it("replay pass proves a step; fail does not", () => {
    const run: JourneyRunView = { state: "ready", kind: "replay", screens: { open: { frame: true, result: "fail", reason: "x" }, watch: { frame: true, result: "pass" } } };
    const m = buildMission(journey, run, []);
    expect(m.steps.map((s) => s.status)).toEqual(["failed", "proven", "not_run"]);
    expect(m.steps[0].reason).toBe("x");
    expect(m.summary.proven).toBe(1);
  });

  it("preview is not proof", () => {
    const run: JourneyRunView = { state: "ready", kind: "preview", screens: { watch: { frame: true, result: "pass" } } };
    expect(buildMission(journey, run, []).steps[1].status).toBe("preview_only");
  });

  it("skipped edge needs you", () => {
    const run: JourneyRunView = { state: "ready", kind: "replay", screens: {}, edges: { "watch->mark": { result: "skipped", reason: "data-changing" } } };
    const m = buildMission(journey, run, []);
    expect(m.steps[2].status).toBe("needs_you");
    expect(m.summary.needYou).toBe(1);
  });

  it("links tasks by journey and sets trust", () => {
    const m = buildMission(journey, null, [
      task({ key: "a", step: "implement" }),
      task({ key: "b", step: "review", reviewers: ["r"] }),
      task({ key: "c", step: "merged", mergeSha: "m" }),
      task({ key: "d", journeys: ["other"] }),
    ]);
    expect(m.tasks.map((t) => [t.key, t.trust])).toEqual([["a", 1], ["b", 1], ["c", 3]]);
    expect(m.summary.inFlight).toBe(2);
  });

  it("a task linked only by its claim appears in the mission (#577)", () => {
    const records = [{ ...parseTaskEvent("task.claimed", "gh-claude-4", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-577", revision: 1, at: "2026-10-09T00:00:00Z", issue: 577, lane: "gh-claude-4", branch: "issue-577-x", journeys: ["watch"] }))!, sequence: 1 }];
    const m = buildMission(journey, null, foldTaskEvents(records));
    expect(m.tasks.map((t) => t.key)).toEqual(foldTaskEvents(records).map((t) => t.key));
    expect(m.tasks).toHaveLength(1);
  });

  it("blocked task keeps its flag", () => {
    const m = buildMission(journey, null, [task({ blockedBy: { reviewer: "r", headSha: "h", commentUrl: "u" } })]);
    expect(m.tasks[0].blocked).toBe(true);
  });

  it("orphan tasks", () => {
    const orphans = unlinkedTasks([journey], [task({ key: "x", journeys: [] }), task({ key: "y", journeys: ["gone"] }), task({ key: "z" })]);
    expect(orphans.map((t) => t.key)).toEqual(["x", "y"]);
  });

  it("ladder labels are verbatim", () => {
    expect(TRUST_LABELS).toEqual(["Written", "Reviewed", "Merged", "Proven", "Seen by you"]);
  });

  it("empty prTitle falls through to title", () => {
    const m = buildMission(journey, null, [task({ prTitle: "", title: "Real title" })]);
    expect(m.tasks[0].title).toBe("Real title");
  });
});

describe("custodyRows", () => {
  it("reads who touched it from the review rounds, the approval and the merge", () => {
    const m = buildMission(journey, null, [task({
      key: "r", lane: "impl", step: "merged", reviewers: ["rev-a"], mergeSha: "4f1ead4c99", repoUrl: "https://github.com/o/r",
      rounds: [{ reviewer: "rev-a", headSha: "aaaaaaaa11", commentUrl: "", fixHead: "bbbbbbbb22", blockedAt: null, fixedAt: null }],
    })]);
    expect(custodyRows(m.tasks[0]!).map((r) => [r.stage, r.who, r.verdict])).toEqual([
      ["Implement", "impl", "done"], ["Review", "rev-a", "BLOCK"], ["Fix", "impl", "pushed bbbbbbbb"],
      ["Re-review", "rev-a", "APPROVE"], ["Merge", "4f1ead4c", "merged"],
    ]);
  });

  it("an unanswered BLOCK ends the rows at the BLOCK", () => {
    const m = buildMission(journey, null, [task({
      step: "review", reviewers: ["rev-b"], blockedBy: { reviewer: "rev-b", headSha: "cccccccc", commentUrl: "" },
      rounds: [{ reviewer: "rev-b", headSha: "cccccccc", commentUrl: "", fixHead: null, blockedAt: null, fixedAt: null }],
    })]);
    expect(custodyRows(m.tasks[0]!).map((r) => r.verdict)).toEqual(["done", "BLOCK"]);
    expect(m.tasks[0]!.blockedBy).toEqual({ reviewer: "rev-b", headSha: "cccccccc" });
  });

  it("a task with no records past its claim reads one Implement row", () => {
    const m = buildMission(journey, null, [task({ step: "implement" })]);
    expect(custodyRows(m.tasks[0]!)).toEqual([{ stage: "Implement", who: "gh-claude-1", verdict: "working", tone: "run" }]);
  });
});
