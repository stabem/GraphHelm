import { describe, expect, it } from "vitest";
import { foldTaskEvents, parseTaskEvent, type TaskEventRecord } from "./team-tasks";

/* #386 (spec §7, §9 row F): the Team tab's per-task graph is folded from `task.*` records alone.
 * These cells observe the fold the spec names: a PR that is blocked, gets a new head and is then
 * approved and merged. They catch a fold that clears the red edge on a new head before a verdict
 * on that head, one that keeps a stale block after the approval, and records attributed to an
 * actor other than the one that recorded them. Cost: pure functions, no I/O, milliseconds. */

function record(sequence: number, kind: string, actorId: string, document: Record<string, unknown>): TaskEventRecord {
  const parsed = parseTaskEvent(kind, actorId, JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "pr-384", revision: sequence, at: "2026-10-07T20:00:00Z", ...document }));
  if (parsed === null) throw new Error(`fixture ${kind} did not parse`);
  return { ...parsed, sequence };
}

describe("foldTaskEvents", () => {
  it("folds pr_opened, BLOCK, new head, APPROVE and merged into the spec's step sequence", () => {
    const records = [
      record(1, "task.pr_opened", "gh-claude-4", { pr: 384, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" }),
      record(2, "task.review_verdict", "gh-claude-1", { pr: 384, headSha: "aaaaaaaa", reviewer: "gh-claude-1", verdict: "BLOCK", commentUrl: "https://github.com/stabem/GraphHelm/pull/384#c1" }),
      record(3, "task.pr_opened", "gh-claude-4", { pr: 384, headSha: "bbbbbbbb", journeys: [], lane: "gh-claude-4" }),
      record(4, "task.review_verdict", "gh-claude-1", { pr: 384, headSha: "bbbbbbbb", reviewer: "gh-claude-1", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/384#c2" }),
      record(5, "task.merged", "gh-claude-1", { pr: 384, mergeSha: "cccccccc", closes: [382], merger: "gh-claude-1" }),
    ];
    const states = records.map((_, index) => foldTaskEvents(records.slice(0, index + 1))[0]);
    expect(states.map((state) => [state.step, state.blockedBy?.headSha ?? null])).toEqual([
      ["review", null],
      ["review", "aaaaaaaa"],
      ["review", "aaaaaaaa"],
      ["merge", null],
      ["merged", null],
    ]);
    expect(states[1].blockedBy?.reviewer).toBe("gh-claude-1");
    expect(states[2].headSha).toBe("bbbbbbbb");
    expect(states[4]).toMatchObject({ taskId: "pr-384", pr: 384, lane: "gh-claude-4", mergeSha: "cccccccc" });
  });

  it("refuses a record whose lane field names another lane than the actor that recorded it", () => {
    expect(parseTaskEvent("task.pr_opened", "agent-chat", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "pr-1", revision: 1, at: "2026-10-07T20:00:00Z", pr: 1, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" }))).toBeNull();
  });
});
