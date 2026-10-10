import { describe, expect, it } from "vitest";
import { foldTaskEvents, type TaskEventRecord } from "./team-tasks";
import { handoffSummary } from "./handoff-summary";

// Contract: recorded debt, head changes and merge evidence. The tree tests only group assignments.
// Regression: treating BLOCK as an outstanding review, or a terminal step as merge evidence.
// Cost: pure fold, milliseconds, no I/O or test-only seams.
const records: TaskEventRecord[] = [
  { kind: "task.claimed", actorId: "lane-a", sequence: 1, taskId: "issue-901", issue: 901, lane: "lane-a", branch: "fixture-a", assignedBy: "lead-901" },
  { kind: "task.review_assigned", actorId: "lane-a", sequence: 2, taskId: "issue-901", pr: 903, reviewer: "lane-b", headSha: "aaaaaaaa" },
  { kind: "task.claimed", actorId: "lane-c", sequence: 3, taskId: "issue-902", issue: 902, lane: "lane-c", branch: "fixture-c" },
  { kind: "task.pr_opened", actorId: "lane-a", sequence: 4, taskId: "issue-901", pr: 903, lane: "lane-a", headSha: "aaaaaaaa", occurredAt: "2026-10-10T10:00:00Z" },
  { kind: "task.review_verdict", actorId: "lane-b", sequence: 5, taskId: "issue-901", pr: 903, reviewer: "lane-b", headSha: "aaaaaaaa", verdict: "BLOCK", commentUrl: "https://github.com/stabem/GraphHelm/pull/903#issuecomment-1", occurredAt: "2026-10-10T10:01:00Z" },
];
describe("handoffSummary", () => {
  it("reports BLOCK fix debt without inventing review debt or a last-record timestamp", () => {
    const result = handoffSummary(foldTaskEvents(records));
    expect(result.now).toEqual([{ key: "issue-901#pr-903", issue: 901, lane: "lane-a", step: "review", since: "2026-10-10T10:00:00Z" }]);
    expect(result.last).toMatchObject({ key: "issue-901#pr-903", step: "review", sequence: 5, at: null });
    expect(result.next.map((item) => item.text)).toEqual(["fix owed by lane-a on aaaaaaaa"]);
  });
  it("replaces answered BLOCK debt with the assigned review on the newer head", () => {
    const result = handoffSummary(foldTaskEvents([...records, { kind: "task.pr_opened", actorId: "lane-a", sequence: 6, taskId: "issue-901", pr: 903, lane: "lane-a", headSha: "bbbbbbbb" }]));
    expect(result.next.map((item) => item.text)).toEqual(["review owed by lane-b"]);
  });
  it("never reports merged without a recorded merge SHA", () => {
    const tasks = foldTaskEvents(records);
    const task = tasks.find((item) => item.issue === 901)!;
    task.step = "merged";
    task.blockedBy = null;
    expect(handoffSummary(tasks).last?.step).toBe("not recorded");
    task.mergeSha = "cccccccc";
    expect(handoffSummary(tasks).last?.step).toBe("merged");
  });
});
