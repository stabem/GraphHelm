import { describe, expect, it } from "vitest";
import { digestOf } from "./customs";
import { foldTaskEvents, parseTaskEvent, readTaskEvents, type TaskEventRecord } from "./team-tasks";
import type { RuntimeEvent } from "./types";

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

  it("links the task to the journeys its claim names, and an empty pr_opened list keeps them (#577)", () => {
    const claimed = record(1, "task.claimed", "gh-claude-4", { issue: 577, lane: "gh-claude-4", branch: "issue-577-x", journeys: ["studio-graph-tab"] });
    expect(foldTaskEvents([claimed])[0].journeys).toEqual(["studio-graph-tab"]);
    const opened = record(2, "task.pr_opened", "gh-claude-4", { pr: 600, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" });
    expect(foldTaskEvents([claimed, opened])[0].journeys).toEqual(["studio-graph-tab"]);
    const renamed = record(3, "task.pr_opened", "gh-claude-4", { pr: 600, headSha: "bbbbbbbb", journeys: ["studio-connect"], lane: "gh-claude-4" });
    expect(foldTaskEvents([claimed, opened, renamed])[0].journeys).toEqual(["studio-connect"]);
  });

  it("keeps a claim whose journeys field is malformed and drops only the field (#577)", () => {
    const claimed = record(1, "task.claimed", "gh-claude-4", { issue: 577, lane: "gh-claude-4", branch: "issue-577-x", journeys: ["ok", "bad id!"] });
    expect(claimed).not.toHaveProperty("journeys");
    expect(foldTaskEvents([claimed])[0]).toMatchObject({ issue: 577, journeys: [] });
  });

  it("refuses a record whose lane field names another lane than the actor that recorded it", () => {
    expect(parseTaskEvent("task.pr_opened", "agent-chat", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "pr-1", revision: 1, at: "2026-10-07T20:00:00Z", pr: 1, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" }))).toBeNull();
  });
});

describe("readTaskEvents", () => {
  async function sealed(id: string, kind: string, signer: string, document: Record<string, unknown>) {
    const content = JSON.stringify({ id: `signal-${id}`, source: { type: "user", id: signer }, type: kind, description: JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-9", revision: 1, at: "2026-10-07T20:00:00Z", ...document }) });
    const hash = (await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle)).slice("sha256:".length);
    return { evidenceId: id, mediaType: "application/json", sensitivity: "internal" as const, contentSha256: hash, content };
  }
  function event(sequence: number, actorId: string, kind: string, evidence: { evidenceId: string; contentSha256: string }): RuntimeEvent {
    return { sequence, kind: "signal_recorded", payload: { kind, sourceId: actorId, sourceKind: "user", signalId: `signal-${evidence.evidenceId}`, envelopeSha256: evidence.contentSha256 }, occurredAt: null, actorId, actorType: "agent", idempotencyKey: null, eventId: `event-${sequence}`, evidenceRefs: [evidence.evidenceId] };
  }

  it("folds sealed task records and skips one whose envelope was signed by another actor", async () => {
    const claimed = await sealed("e1", "task.claimed", "gh-claude-4", { issue: 9, lane: "gh-claude-4", branch: "issue-9-x" });
    const forged = await sealed("e2", "task.pr_opened", "gh-claude-4", { pr: 10, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" });
    const store = new Map([[claimed.evidenceId, claimed], [forged.evidenceId, forged]]);
    const tasks = await readTaskEvents({
      executionId: "gh-team",
      events: [event(1, "gh-claude-4", "task.claimed", claimed), event(2, "agent-chat", "task.pr_opened", forged)],
      readEvidence: async (_, id) => store.get(id)!,
    });
    expect(tasks).toHaveLength(1);
    expect(tasks[0]).toMatchObject({ taskId: "issue-9", issue: 9, lane: "gh-claude-4", step: "implement", pr: null });
  });

  /* #185: on a gh-team-sized run (~1,250 task records) the graph took about four minutes to draw,
   * because each envelope was read only after the previous one arrived: one round trip at a time.
   * The reads now overlap (bounded), and the fold still sees the records in sequence order. */
  it("reads the envelopes concurrently, bounded, and folds them in sequence order", async () => {
    const claimed = await sealed("c1", "task.claimed", "gh-claude-4", { issue: 9, lane: "gh-claude-4", branch: "issue-9-x" });
    const heads = await Promise.all(Array.from({ length: 30 }, (_, n) =>
      sealed(`p${n}`, "task.pr_opened", "gh-claude-4", { pr: 10, headSha: `${n}`.padStart(8, "a"), journeys: [], lane: "gh-claude-4" })));
    const store = new Map([claimed, ...heads].map((evidence) => [evidence.evidenceId, evidence]));
    let inFlight = 0;
    let most = 0;
    const tasks = await readTaskEvents({
      executionId: "gh-team",
      events: [event(1, "gh-claude-4", "task.claimed", claimed), ...heads.map((evidence, n) => event(n + 2, "gh-claude-4", "task.pr_opened", evidence))].reverse(),
      readEvidence: async (_, id) => {
        inFlight += 1;
        most = Math.max(most, inFlight);
        // The later records answer first: the fold must not depend on arrival order.
        await new Promise((resolve) => setTimeout(resolve, 40 - Number(id.slice(1) || 0)));
        inFlight -= 1;
        return store.get(id)!;
      },
    });
    expect(most).toBeGreaterThan(1);
    expect(most).toBeLessThanOrEqual(16);
    expect(tasks).toHaveLength(1);
    expect(tasks[0]).toMatchObject({ issue: 9, pr: 10, headSha: "29".padStart(8, "a"), step: "review" });
  });
});

/* #420: a task links to its issue and PR from the record that opens it, not from a review comment.
 * Before, `repoUrl` was learned only from a verdict's `commentUrl`, so a task waiting for its first
 * review (the one the owner most wants to open) had no link. The record's optional `repo`
 * (`owner/name`) gives it from `task.claimed` on; a malformed one is refused like any bad field.
 * Cost: pure functions, milliseconds. */
describe("task links from the record's own repo", () => {
  it("knows the repository from task.claimed, before any verdict", () => {
    const states = foldTaskEvents([
      record(1, "task.claimed", "gh-claude-4", { issue: 902, lane: "gh-claude-4", branch: "issue-902-x", repo: "stabem/GraphHelm" }),
      record(2, "task.pr_opened", "gh-claude-4", { pr: 9002, headSha: "dddddddd", journeys: [], lane: "gh-claude-4" }),
    ]);
    expect(states[0]).toMatchObject({ issue: 902, pr: 9002, repoUrl: "https://github.com/stabem/GraphHelm" });
  });

  it("refuses a repo that is not owner/name", () => {
    for (const repo of ["stabem", "https://evil.example/x/y", "a/b/c", "../x", "a /b"]) {
      expect(parseTaskEvent("task.claimed", "gh-claude-4", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-1",
        revision: 1, at: "2026-10-08T00:00:00Z", issue: 1, lane: "gh-claude-4", branch: "b", repo })), repo).toBeNull();
    }
  });
});

/* #460: an issue worked in two PRs (#356: issue-356-heal as PR #456, then issue-356-explore-faults)
 * folded into one task, so the first PR's merge hid the second slice for its whole life. Each PR is
 * its own slice: a lane's claim on a new branch is a new slice, joined by its next pr_opened, and a
 * merge ends only its own slice. The records are #356's order on gh-team (seq 3252..3279). */
describe("foldTaskEvents slices (#460)", () => {
  const issue = { taskId: "issue-356" };
  const records = [
    record(1, "task.claimed", "gh-claude-4", { ...issue, issue: 356, lane: "gh-claude-4", branch: "issue-356-heal" }),
    record(2, "task.pr_opened", "gh-claude-4", { ...issue, pr: 456, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" }),
    record(3, "task.review_assigned", "gh-claude-4", { ...issue, pr: 456, headSha: "aaaaaaaa", reviewer: "gh-claude-6", ordinal: 1 }),
    record(4, "task.claimed", "gh-claude-4", { ...issue, issue: 356, lane: "gh-claude-4", branch: "issue-356-explore-faults" }),
    record(5, "task.review_verdict", "gh-claude-6", { ...issue, pr: 456, headSha: "aaaaaaaa", reviewer: "gh-claude-6", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/456#c1" }),
    record(6, "task.merged", "gh-claude-6", { ...issue, pr: 456, mergeSha: "cccccccc", closes: [], merger: "gh-claude-6" }),
    record(7, "task.pr_opened", "gh-claude-4", { ...issue, pr: 461, headSha: "dddddddd", journeys: [], lane: "gh-claude-4" }),
  ];

  it("keeps the second slice visible, with its own PR, after the first one merged", () => {
    const tasks = foldTaskEvents(records);
    expect(tasks.map((task) => [task.taskId, task.pr, task.step, task.issue])).toEqual([
      ["issue-356", 456, "merged", 356],
      ["issue-356", 461, "review", 356],
    ]);
    expect(new Set(tasks.map((task) => task.key)).size).toBe(2);
    expect(tasks[1].headSha).toBe("dddddddd");
  });

  // gh-claude-2's BLOCK on a87a28ad: one lane claims two slices before opening either PR (the
  // DELIVERY rule asks for exactly that). The second claim must not take over the first.
  it("keeps two claims of one lane apart, and the first PR joins the first claim", () => {
    const claim = (sequence: number, branch: string) =>
      record(sequence, "task.claimed", "gh-claude-4", { ...issue, issue: 356, lane: "gh-claude-4", branch });
    const tasks = foldTaskEvents([
      claim(1, "issue-356-heal"), claim(2, "issue-356-explore"),
      record(3, "task.pr_opened", "gh-claude-4", { ...issue, pr: 456, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" }),
    ]);
    expect(tasks.map((task) => [task.branch, task.pr, task.step]).sort()).toEqual([
      ["issue-356-explore", null, "implement"],
      ["issue-356-heal", 456, "review"],
    ]);
  });

  it("shows the second claim as its own slice before it has a PR", () => {
    const tasks = foldTaskEvents(records.slice(0, 4));
    expect(tasks.map((task) => [task.pr, task.step])).toEqual([[456, "review"], [null, "implement"]]);
  });
});
