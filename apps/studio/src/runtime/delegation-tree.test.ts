import { describe, expect, it } from "vitest";
import { buildDelegationTree } from "./delegation-tree";
import { foldTaskEvents, parseTaskEvent, type TaskEventRecord } from "./team-tasks";

// Contract: recorded handoffs, including unknown assigners, never inferred from verdicts.
// Gap: task graphs show steps, not parentage. Cost: pure folds, milliseconds, no I/O.
function event(sequence: number, kind: string, actor: string, fields: Record<string, unknown>): TaskEventRecord {
  const parsed = parseTaskEvent(kind, actor, JSON.stringify({ schema: "graphhelm-task-event-v1",
    taskId: "issue-86", revision: 1, at: "2026-10-09T12:00:00Z", ...fields }));
  if (!parsed) throw new Error("Invalid fixture");
  return { ...parsed, sequence };
}
const claim = (assignedBy?: string) => event(1, "task.claimed", "lane", {
  issue: 86, lane: "lane", branch: "issue-86-tree", ...(assignedBy ? { assignedBy } : {}),
});
const opened = event(2, "task.pr_opened", "lane", { pr: 99, headSha: "aaaaaaaa", lane: "lane", journeys: [] });
const review = event(3, "task.review_assigned", "lane", { pr: 99, headSha: "aaaaaaaa", reviewer: "rev" });

describe("recorded delegation tree", () => {
  it("keeps coord to lane to issue to reviewer with each edge's source", () => {
    const roots = buildDelegationTree(foldTaskEvents([claim("coord"), opened, review]));
    expect(roots).toMatchObject([{ assigner: "coord", lanes: [{ lane: "lane",
      source: "claimed.assignedBy (reported by lane)", tasks: [{ task: { issue: 86 },
        source: "claimed.assignedBy (reported by lane)", reviewers: [{ reviewer: "rev", source: "review_assigned" }] }],
    }] }]);
  });
  it("keeps old claims under assigner unrecorded without inventing evidence", () => {
    const roots = buildDelegationTree(foldTaskEvents([claim()]));
    expect(roots).toMatchObject([{ assigner: null, lanes: [{ lane: "lane", source: null,
      tasks: [{ task: { issue: 86 }, source: null, reviewers: [] }] }] }]);
  });
  it("does not present a verdict alone as a recorded review assignment", () => {
    const verdict = event(4, "task.review_verdict", "unsolicited", { pr: 99, headSha: "aaaaaaaa",
      reviewer: "unsolicited", verdict: "BLOCK", commentUrl: "https://github.com/stabem/GraphHelm/pull/99#c1" });
    const roots = buildDelegationTree(foldTaskEvents([claim("coord"), opened, review, verdict]));
    expect(roots[0].lanes[0].tasks[0].reviewers).toEqual([{ reviewer: "rev", source: "review_assigned" }]);
    expect(roots[0].lanes[0].tasks[0].task.blockedBy?.reviewer).toBe("unsolicited");
  });
});
