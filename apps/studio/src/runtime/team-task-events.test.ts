import { describe, expect, it, vi } from "vitest";
import { digestOf } from "./customs";
import { laneBars } from "./lane-bars";
import { foldTaskEvents, parseTaskEvent, readTaskEventRecords, timingSamples, type TaskEventRecord } from "./team-tasks";
import type { RuntimeEvent } from "./types";

/* #386 (spec §7, §9 row F): the Team tab's per-task graph is folded from `task.*` records alone.
 * These cells observe the fold the spec names: a PR that is blocked, gets a new head and is then
 * approved and merged. They catch a fold that keeps a stale block after the author pushed an
 * answer (#608) or after the approval, and records attributed to an actor other than the one that
 * recorded them. Cost: pure functions, no I/O, milliseconds. */

function record(sequence: number, kind: string, actorId: string, document: Record<string, unknown>): TaskEventRecord {
  const parsed = parseTaskEvent(kind, actorId, JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "pr-384", revision: sequence, at: "2026-10-07T20:00:00Z", ...document }));
  if (parsed === null) throw new Error(`fixture ${kind} did not parse`);
  return { ...parsed, sequence };
}

describe("foldTaskEvents", () => {
  // Cost: a pure fold with no I/O, completed in milliseconds.
  it("recovers an issue from taskId when the claim is outside the recent page (#993)", () => {
    const tasks = foldTaskEvents([
      record(1, "task.pr_opened", "codex-6", { taskId: "issue-913", pr: 914, headSha: "aaaaaaaa", journeys: [], lane: "codex-6" }),
      record(2, "task.review_verdict", "gh-claude-4", { taskId: "issue-913", pr: 914, headSha: "aaaaaaaa", reviewer: "gh-claude-4", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/914#c1" }),
      record(3, "task.pr_opened", "codex-6", { taskId: "pr-384", pr: 384, headSha: "bbbbbbbb", journeys: [], lane: "codex-6" }),
    ]);

    expect(tasks.find((task) => task.taskId === "issue-913")).toMatchObject({ issue: 913, pr: 914 });
    expect(tasks.find((task) => task.taskId === "pr-384")?.issue).toBeNull();
  });
  it("attaches a plan recorded before its claim to the PR that later merges (#880)", () => {
    const taskId = "issue-758";
    const events = [
      record(4864, "task.planned", "codex-13", { taskId, lane: "codex-13", classes: ["code"], reviews: 1, proof: "both", critic: { mode: "none", passScore: 8, maxRounds: 3 }, summary: "Plan" }),
      record(4865, "task.claimed", "codex-13", { taskId, issue: 758, lane: "codex-13", branch: "issue-758-evidence-cache" }),
      record(4877, "task.pr_opened", "codex-13", { taskId, pr: 760, headSha: "aaaaaaaa", journeys: [], lane: "codex-13" }),
      record(4905, "task.merged", "codexrev-1", { taskId, pr: 760, mergeSha: "bbbbbbbb", closes: [758], merger: "codexrev-1" }),
    ];
    const tasks = foldTaskEvents(events);
    expect(tasks).toHaveLength(1);
    expect(tasks[0]).toMatchObject({ taskId, issue: 758, pr: 760, step: "merged" });
    expect(tasks.filter((task) => task.pr === null)).toHaveLength(0);
    const now = Date.parse("2026-10-07T20:31:00Z");
    const lanes = laneBars(events.map((event, index) => ({ ...event, at: new Date(now - 31 * 60_000 + index * 1_000).toISOString() })), now, 60 * 60_000);
    expect(lanes.find((lane) => lane.lane === "codex-13")?.bars.filter((bar) => bar.open)).toEqual([]);
  });
  it("does not reopen gh-claude-9's merged PR #516 (#881)", () => {
    const taskId = "issue-509";
    const events = [
      record(3460, "task.claimed", "gh-claude-9", { taskId, issue: 509, lane: "gh-claude-9", branch: "issue-509-journeys-plain-words" }),
      record(3475, "task.pr_opened", "gh-claude-9", { taskId, pr: 516, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-9" }),
      record(3486, "task.review_verdict", "gh-claude-6", { taskId, pr: 516, headSha: "aaaaaaaa", reviewer: "gh-claude-6", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/516#c1" }),
      record(3487, "task.merged", "gh-claude-6", { taskId, pr: 516, mergeSha: "bbbbbbbb", closes: [509], merger: "gh-claude-6" }),
      record(3528, "task.claimed", "gh-claude-9", { taskId: "issue-518", issue: 518, lane: "gh-claude-9", branch: "issue-518-mark-act-safe" }),
    ];
    const tasks = foldTaskEvents(events);
    expect(tasks.filter((task) => task.pr === 516)).toMatchObject([{ step: "merged", issue: 509 }]);
    expect(tasks.filter((task) => task.taskId === taskId && task.pr === null)).toEqual([]);
    const now = Date.parse("2026-10-09T12:00:00Z");
    expect(laneBars(events.map((event, index) => ({ ...event, at: new Date(now - 30_000 + index * 1_000).toISOString() })), now, 60 * 60_000)
      .find((lane) => lane.lane === "gh-claude-9")?.bars.filter((bar) => bar.open && bar.label === "#516") ?? []).toEqual([]);
  });
  it("keeps an interrupted pre-claim plan out of a later claim (#881)", () => {
    const taskId = "issue-100";
    const tasks = foldTaskEvents([
      record(1, "task.planned", "lane-a", { taskId, lane: "lane-a", classes: ["code"], reviews: 1, proof: "tests", critic: { mode: "none", passScore: 8, maxRounds: 3 }, summary: "Old plan" }),
      record(2, "task.claimed", "lane-b", { taskId, issue: 100, lane: "lane-b", branch: "issue-100-other" }),
      record(3, "task.claimed", "lane-a", { taskId, issue: 100, lane: "lane-a", branch: "issue-100-new" }),
    ]);
    expect(tasks.find((task) => task.branch === "issue-100-new")?.plan).toBeNull();
  });
  it("drops self-assigned or malformed claimed assigners (#86)", () => {
    for (const assignedBy of ["lane", "x".repeat(129), "", null, "bad\nname"]) {
      expect(parseTaskEvent("task.claimed", "lane", JSON.stringify({ schema: "graphhelm-task-event-v1",
        taskId: "issue-86", revision: 1, issue: 86, lane: "lane", branch: "issue-86-tree", assignedBy }))).toBeNull();
    }
    const assignedBy = "é".repeat(128);
    expect(parseTaskEvent("task.claimed", "lane", JSON.stringify({ schema: "graphhelm-task-event-v1",
      taskId: "issue-86", revision: 1, issue: 86, lane: "lane", branch: "issue-86-tree", assignedBy })))
      .toMatchObject({ assignedBy });
  });
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
      // #608: the pushed head answers the BLOCK; it is the re-review's head, no longer blocked.
      ["review", null],
      ["merge", null],
      ["merged", null],
    ]);
    expect(states[1].blockedBy?.reviewer).toBe("gh-claude-1");
    expect(states[2].headSha).toBe("bbbbbbbb");
    expect(states[4]).toMatchObject({ taskId: "pr-384", pr: 384, lane: "gh-claude-4", mergeSha: "cccccccc" });
  });

  // #608 (found by gh-design in the Graph, issue-591): after a BLOCK at head A, the author's
  // pr_opened at head B answers it. Every view (Team tab, Graph, Lanes) reads the fold, so the
  // fold says it: no `blockedBy`, the round keeps the BLOCK with its fixHead, and the step is the
  // re-review waiting on that reviewer. A pr_opened that repeats head A answers nothing. Catches
  // `blockedBy` cleared only by a verdict, which drew a pushed fix as "blocked" everywhere.
  it("a newer head after a BLOCK reads as the re-review, not as blocked (#608)", () => {
    const opened = record(1, "task.pr_opened", "gh-claude-4", { pr: 608, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" });
    const assigned = record(2, "task.review_assigned", "gh-claude-4", { pr: 608, headSha: "aaaaaaaa", reviewer: "gh-claude-5", lane: "gh-claude-4" });
    const block = record(3, "task.review_verdict", "gh-claude-5", { pr: 608, headSha: "aaaaaaaa", reviewer: "gh-claude-5", verdict: "BLOCK", commentUrl: "https://github.com/stabem/GraphHelm/pull/608#c1" });
    const same = record(4, "task.pr_opened", "gh-claude-4", { pr: 608, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" });
    const fix = record(5, "task.pr_opened", "gh-claude-4", { pr: 608, headSha: "bbbbbbbb", journeys: [], lane: "gh-claude-4" });

    expect(foldTaskEvents([opened, assigned, block])[0].blockedBy?.headSha).toBe("aaaaaaaa");
    expect(foldTaskEvents([opened, assigned, block, same])[0].blockedBy?.headSha).toBe("aaaaaaaa");

    const pushed = foldTaskEvents([opened, assigned, block, fix])[0];
    expect(pushed.blockedBy).toBeNull();
    expect(pushed.step).toBe("review");
    expect(pushed.headSha).toBe("bbbbbbbb");
    expect(pushed.reviewers).toEqual(["gh-claude-5"]);
    expect(pushed.rounds).toHaveLength(1);
    expect(pushed.rounds[0]).toMatchObject({ reviewer: "gh-claude-5", headSha: "aaaaaaaa", fixHead: "bbbbbbbb" });
  });

  it("tracks the newest pushed head as the fix before the next verdict (#990)", () => {
    const records = [
      record(1, "task.pr_opened", "codex-1", { pr: 990, headSha: "cccccccc", journeys: [], lane: "codex-1" }),
      record(2, "task.review_verdict", "codex-review", { pr: 990, headSha: "cccccccc", reviewer: "codex-review", verdict: "BLOCK", commentUrl: "https://github.com/stabem/GraphHelm/pull/990#c1" }),
      record(3, "task.pr_opened", "codex-1", { pr: 990, headSha: "dddddddd", journeys: [], lane: "codex-1" }),
      record(4, "task.pr_opened", "codex-1", { pr: 990, headSha: "eeeeeeee", journeys: [], lane: "codex-1" }),
    ];

    const state = foldTaskEvents(records)[0];
    expect(state.rounds.at(-1)?.fixHead).toBe("eeeeeeee");
    expect(state.headSha).toBe("eeeeeeee");
  });

  it("keeps a BLOCK until its reviewer answers, and records approval with risk (#1002)", () => {
    const opened = record(1, "task.pr_opened", "codex-1", { taskId: "issue-1002", pr: 1002, headSha: "aaaaaaaa", journeys: [], lane: "codex-1" });
    const block = record(2, "task.review_verdict", "reviewer-one", { taskId: "issue-1002", pr: 1002, headSha: "aaaaaaaa", reviewer: "reviewer-one", verdict: "BLOCK", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c1" });
    const otherApproval = record(3, "task.review_verdict", "reviewer-two", { taskId: "issue-1002", pr: 1002, headSha: "aaaaaaaa", reviewer: "reviewer-two", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c2" });
    const ownApproval = record(4, "task.review_verdict", "reviewer-one", { taskId: "issue-1002", pr: 1002, headSha: "aaaaaaaa", reviewer: "reviewer-one", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c3" });
    expect(foldTaskEvents([opened, block, otherApproval])[0]).toMatchObject({ step: "review", blockedBy: { reviewer: "reviewer-one" } });
    expect(foldTaskEvents([opened, block, otherApproval, ownApproval])[0]).toMatchObject({ step: "merge", blockedBy: null });
  });

  it("keeps every reviewer's unanswered BLOCK on the current head (#1002)", () => {
    const taskId = "issue-1002";
    const urlOne = "https://github.com/stabem/GraphHelm/pull/1002#c1";
    const urlTwo = "https://github.com/stabem/GraphHelm/pull/1002#c2";
    const opened = record(1, "task.pr_opened", "codex-1", { taskId, pr: 1002, headSha: "aaaaaaaa", journeys: [], lane: "codex-1" });
    const blockOne = record(2, "task.review_verdict", "reviewer-one", { taskId, pr: 1002, headSha: "aaaaaaaa", reviewer: "reviewer-one", verdict: "BLOCK", commentUrl: urlOne });
    const blockTwo = record(3, "task.review_verdict", "reviewer-two", { taskId, pr: 1002, headSha: "aaaaaaaa", reviewer: "reviewer-two", verdict: "BLOCK", commentUrl: urlTwo });
    const approveTwo = record(4, "task.review_verdict", "reviewer-two", { taskId, pr: 1002, headSha: "aaaaaaaa", reviewer: "reviewer-two", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c3" });
    const approveOne = record(5, "task.review_verdict", "reviewer-one", { taskId, pr: 1002, headSha: "aaaaaaaa", reviewer: "reviewer-one", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c4" });

    expect(foldTaskEvents([opened, blockOne, blockTwo, approveTwo])[0]).toMatchObject({
      step: "review", blockedBy: { reviewer: "reviewer-one", commentUrl: urlOne }, blockedSequence: 2,
    });
    expect(foldTaskEvents([opened, blockOne, blockTwo, approveOne])[0]).toMatchObject({
      step: "review", blockedBy: { reviewer: "reviewer-two", commentUrl: urlTwo }, blockedSequence: 3,
    });
    expect(foldTaskEvents([opened, blockOne, blockTwo, approveTwo, approveOne])[0]).toMatchObject({
      step: "merge", blockedBy: null, blockedSequence: null,
    });
  });

  it("records APPROVE-WITH-RISK and preserves it after a plain approval (#1002)", () => {
    const opened = record(1, "task.pr_opened", "codex-1", { taskId: "issue-1002", pr: 1002, headSha: "aaaaaaaa", journeys: [], lane: "codex-1" });
    const riskApproval = record(2, "task.review_verdict", "reviewer-two", { taskId: "issue-1002", pr: 1002, headSha: "aaaaaaaa", reviewer: "reviewer-two", verdict: "APPROVE-WITH-RISK", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c4" });
    const plainApproval = record(3, "task.review_verdict", "reviewer-one", { taskId: "issue-1002", pr: 1002, headSha: "aaaaaaaa", reviewer: "reviewer-one", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c5" });

    expect(foldTaskEvents([opened, riskApproval])[0]).toMatchObject({
      step: "merge", approvedWithRisk: { reviewer: "reviewer-two", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c4" },
    });
    expect(foldTaskEvents([opened, riskApproval, plainApproval])[0].approvedWithRisk).toEqual({
      reviewer: "reviewer-two", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c4",
    });
  });

  it("routes a shared PR review by the head named in the event (#1002)", () => {
    const taskA = "issue-1002-a";
    const taskB = "issue-1002-b";
    const records = [
      record(1, "task.claimed", "lane-a", { taskId: taskA, issue: 1002, lane: "lane-a", branch: "a" }),
      record(2, "task.pr_opened", "lane-a", { taskId: taskA, pr: 1002, headSha: "aaaaaaaa", journeys: [], lane: "lane-a" }),
      record(3, "task.claimed", "lane-b", { taskId: taskB, issue: 1002, lane: "lane-b", branch: "b" }),
      record(4, "task.pr_opened", "lane-b", { taskId: taskB, pr: 1002, headSha: "bbbbbbbb", journeys: [], lane: "lane-b" }),
      record(5, "task.review_verdict", "reviewer", { taskId: taskB, pr: 1002, headSha: "bbbbbbbb", reviewer: "reviewer", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c1" }),
    ];

    const states = foldTaskEvents(records);
    expect(states.find((state) => state.headSha === "aaaaaaaa")).toMatchObject({ step: "review", blockedBy: null });
    expect(states.find((state) => state.headSha === "bbbbbbbb")).toMatchObject({ step: "merge", blockedBy: null });
  });

  it("keeps an approved same-head repeat in Merge and preserves its review clock (#1002)", () => {
    const at = (sequence: number, kind: string, actor: string, document: Record<string, unknown>, occurredAt: string) => ({
      ...record(sequence, kind, actor, document), occurredAt,
    });
    const records = [
      at(1, "task.pr_opened", "codex-3", { pr: 1002, headSha: "aaaaaaaa", journeys: [], lane: "codex-3", title: "Before", summary: "Before summary" }, "2026-10-10T00:00:00Z"),
      at(2, "task.review_verdict", "codex-review", { pr: 1002, headSha: "aaaaaaaa", reviewer: "codex-review", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/1002#c1" }, "2026-10-10T00:01:00Z"),
      at(3, "task.pr_opened", "codex-3", { pr: 1002, headSha: "aaaaaaaa", journeys: [], lane: "codex-3", title: "After", summary: "Updated summary" }, "2026-10-10T01:00:00Z"),
    ];

    expect(foldTaskEvents(records)[0]).toMatchObject({
      step: "merge",
      clock: { since: "2026-10-10T00:01:00Z", spent: {} },
      prTitle: "After",
      prSummary: "Updated summary",
    });
    expect(foldTaskEvents([...records.slice(0, 2), at(3, "task.pr_opened", "codex-3",
      { pr: 1002, headSha: "bbbbbbbb", journeys: [], lane: "codex-3", title: "After", summary: "Updated summary" }, "2026-10-10T01:00:00Z")])[0].step).toBe("review");
  });

  it("keeps an early assignment for an unseen fix head until that head opens (#983)", () => {
    const common = { taskId: "issue-983", pr: 1004 };
    const records = [
      record(1, "task.claimed", "codex-1", { taskId: common.taskId, issue: 983, lane: "codex-1", branch: "issue-983-fix" }),
      record(2, "task.pr_opened", "codex-1", { ...common, headSha: "aaaaaaaa", journeys: [], lane: "codex-1" }),
      record(3, "task.review_verdict", "codex-review", { ...common, headSha: "aaaaaaaa", reviewer: "codex-review", verdict: "BLOCK", commentUrl: "https://github.com/stabem/GraphHelm/pull/1004#c1" }),
      record(4, "task.review_assigned", "codex-1", { ...common, headSha: "bbbbbbbb", reviewer: "codexrev-3" }),
      record(5, "task.pr_opened", "codex-1", { ...common, headSha: "bbbbbbbb", journeys: [], lane: "codex-1" }),
    ];

    const pending = foldTaskEvents(records.slice(0, 4))[0];
    expect(pending).toMatchObject({ headSha: "aaaaaaaa", currentReviewer: null, blockedBy: { headSha: "aaaaaaaa" } });
    const opened = foldTaskEvents(records)[0];
    expect(opened).toMatchObject({ headSha: "bbbbbbbb", currentReviewer: "codexrev-3", step: "review", blockedBy: null });
    expect(opened.rounds.at(-1)?.fixHead).toBe("bbbbbbbb");
    const stale = [
      record(1, "task.pr_opened", "codex-1", { ...common, headSha: "aaaaaaaa", journeys: [], lane: "codex-1" }),
      record(2, "task.review_verdict", "codex-review", { ...common, headSha: "aaaaaaaa", reviewer: "codex-review", verdict: "BLOCK", commentUrl: "https://github.com/stabem/GraphHelm/pull/1004#c1" }),
      record(3, "task.pr_opened", "codex-1", { ...common, headSha: "bbbbbbbb", journeys: [], lane: "codex-1" }),
      record(4, "task.review_assigned", "codex-1", { ...common, headSha: "aaaaaaaa", reviewer: "old-reviewer" }),
      record(5, "task.pr_opened", "codex-1", { ...common, headSha: "cccccccc", journeys: [], lane: "codex-1" }),
    ];
    expect(foldTaskEvents(stale)[0]).toMatchObject({ headSha: "cccccccc", currentReviewer: null, step: "review", blockedBy: null });
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

  it("parses a release only for its recording lane and exact positive claim sequence", () => {
    const doc = { schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 2, at: "now",
      lane: "lane-a", claimSequence: 4, reason: "Work moved" };
    expect(parseTaskEvent("task.released", "lane-a", JSON.stringify(doc))).toMatchObject({ kind: "task.released", claimSequence: 4 });
    expect(parseTaskEvent("task.released", "lane-b", JSON.stringify(doc))).toBeNull();
    expect(parseTaskEvent("task.released", "lane-a", JSON.stringify({ ...doc, claimSequence: 0 }))).toBeNull();
    expect(parseTaskEvent("task.released", "lane-a", JSON.stringify({ ...doc, reason: "" }))).toBeNull();
  });

  it("a release ends only its latest open claim while other claims stay open", () => {
    const records = [
      { ...parseTaskEvent("task.claimed", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 1, at: "now", issue: 729, lane: "lane-a", branch: "old" }))!, sequence: 10, claimSequence: 10 },
      { ...parseTaskEvent("task.claimed", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 1, at: "now", issue: 729, lane: "lane-a", branch: "new" }))!, sequence: 12, claimSequence: 12 },
      { ...parseTaskEvent("task.claimed", "lane-b", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 1, at: "now", issue: 729, lane: "lane-b", branch: "foreign" }))!, sequence: 13, claimSequence: 13 },
      { ...parseTaskEvent("task.released", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 2, at: "now", lane: "lane-a", claimSequence: 12, reason: "Stopped" }))!, sequence: 14 },
    ].map((event) => ({ ...event, occurredAt: null }));
    const folded = foldTaskEvents(records as TaskEventRecord[]);
    expect(folded.map((task) => [task.branch, task.step, task.releasedBy ?? null])).toEqual([["old", "plan", null], ["new", "plan", "lane-a"], ["foreign", "plan", null]]);
  });

  it("does not release a claim after that claim has a PR", () => {
    const claimed = { ...parseTaskEvent("task.claimed", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 1, at: "now", issue: 729, lane: "lane-a", branch: "old" }))!, sequence: 10, claimSequence: 10 };
    const opened = { ...parseTaskEvent("task.pr_opened", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 2, at: "now", pr: 800, headSha: "aaaaaaa", journeys: [], lane: "lane-a" }))!, sequence: 11 };
    const released = { ...parseTaskEvent("task.released", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 2, at: "now", lane: "lane-a", claimSequence: 10, reason: "Too late" }))!, sequence: 12 };
    expect(foldTaskEvents([claimed, opened, released] as TaskEventRecord[])[0]).toMatchObject({ pr: 800, step: "review" });
  });

  it("a new claim after release stays a separate active slice", () => {
    const claimed = (sequence: number) => ({
      ...parseTaskEvent("task.claimed", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 1, at: "now", issue: 729, lane: "lane-a", branch: "same" }))!,
      sequence,
    });
    const released = { ...parseTaskEvent("task.released", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 2, at: "now", lane: "lane-a", claimSequence: 10, reason: "Stopped" }))!, sequence: 11 };
    expect(foldTaskEvents([claimed(10), released, claimed(12)] as TaskEventRecord[]).map((task) => [task.branch, task.releasedBy ?? null]))
      .toEqual([["same", "lane-a"], ["same", null]]);
  });

  it("routes a review record to the new claim instead of the released slice (#851)", () => {
    const taskId = "issue-851";
    const events = [
      record(1, "task.claimed", "lane-a", { taskId, issue: 851, lane: "lane-a", branch: "old" }),
      record(2, "task.released", "lane-a", { taskId, lane: "lane-a", claimSequence: 1, reason: "Handed over" }),
      record(3, "task.claimed", "lane-b", { taskId, issue: 851, lane: "lane-b", branch: "new" }),
      record(4, "task.review_assigned", "lane-b", { taskId, pr: 852, headSha: "aaaaaaaa", reviewer: "reviewer" }),
    ];
    const slices = foldTaskEvents(events);
    expect(slices).toHaveLength(2);
    expect(slices.find((slice) => slice.branch === "old")).toMatchObject({ lane: "lane-a", releasedBy: "lane-a", pr: null, reviewers: [] });
    expect(slices.find((slice) => slice.branch === "new")).toMatchObject({ lane: "lane-b", pr: 852, reviewers: ["reviewer"] });
  });

  it("a claim after release reopens the same branch", () => {
    const claimed = (sequence: number) => ({
      ...parseTaskEvent("task.claimed", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 1, at: "now", issue: 729, lane: "lane-a", branch: "same" }))!,
      sequence,
    });
    const released = { ...parseTaskEvent("task.released", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 2, at: "now", lane: "lane-a", claimSequence: 10, reason: "Stopped" }))!, sequence: 11 };
    expect(foldTaskEvents([claimed(10), released, claimed(12)] as TaskEventRecord[]).map((task) => task.releasedBy ?? null))
      .toEqual(["lane-a", null]);
  });

  it("a release of an older claim leaves both claims open", () => {
    const claimed = (sequence: number) => ({
      ...parseTaskEvent("task.claimed", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 1, at: "now", issue: 729, lane: "lane-a", branch: "same" }))!,
      sequence,
    });
    const released = { ...parseTaskEvent("task.released", "lane-a", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-729", revision: 2, at: "now", lane: "lane-a", claimSequence: 10, reason: "Stopped" }))!, sequence: 12 };
    expect(foldTaskEvents([claimed(10), claimed(11), released] as TaskEventRecord[]).map((task) => task.releasedBy ?? null))
      .toEqual([null, null]);
  });
});

describe("readTaskEventRecords composed with foldTaskEvents", () => {
  async function sealed(id: string, kind: string, signer: string, document: Record<string, unknown>) {
    const content = JSON.stringify({ id: `signal-${id}`, source: { type: "user", id: signer }, type: kind, description: JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-9", revision: 1, at: "2026-10-07T20:00:00Z", ...document }) });
    const hash = (await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle)).slice("sha256:".length);
    return { evidenceId: id, mediaType: "application/json", sensitivity: "internal" as const, contentSha256: hash, content };
  }
  function event(sequence: number, actorId: string, kind: string, evidence: { evidenceId: string; contentSha256: string }): RuntimeEvent {
    return { sequence, kind: "signal_recorded", payload: { kind, sourceId: actorId, sourceKind: "user", signalId: `signal-${evidence.evidenceId}`, envelopeSha256: evidence.contentSha256 }, occurredAt: null, actorId, actorType: "agent", idempotencyKey: null, eventId: `event-${sequence}`, evidenceRefs: [evidence.evidenceId] };
  }

  it("retries a failed sealed read and reports only evidence still unreadable", async () => {
    const recovered = await sealed("retry-ok", "task.claimed", "lane-a", { issue: 1, lane: "lane-a", branch: "issue-1" });
    const missing = await sealed("retry-missing", "task.claimed", "lane-a", { taskId: "issue-2", issue: 2, lane: "lane-a", branch: "issue-2" });
    const attempts = new Map<string, number>();
    const failures: number[] = [];
    const records = await readTaskEventRecords({
      executionId: "run", events: [event(1, "lane-a", "task.claimed", recovered), event(2, "lane-a", "task.claimed", missing)],
      readEvidence: async (_, id) => {
        const count = (attempts.get(id) ?? 0) + 1;
        attempts.set(id, count);
        if (id === missing.evidenceId || count < 3) throw new Error("temporary read failure");
        return recovered;
      },
      onReadFailure: (count) => failures.push(count),
    });
    expect(records.map((record) => record.sequence)).toEqual([1]);
    expect([...attempts.values()]).toEqual([3, 3]);
    expect(failures).toEqual([1]);
  });

  it("progressive task history publishes verified ordered chunks before the final read", async () => {
    const envelopes = await Promise.all(Array.from({ length: 65 }, (_, index) =>
      sealed(`progress-${index}`, "task.claimed", "lane-a", { taskId: `issue-${index}`, issue: index + 1, lane: "lane-a", branch: `issue-${index}` })));
    const pending = new Map<string, (value: typeof envelopes[number]) => void>();
    const published: number[][] = [];
    const read = readTaskEventRecords({
      executionId: "run", events: envelopes.map((item, index) => event(index + 1, index === 1 ? "forged-actor" : "lane-a", "task.claimed", item)),
      readEvidence: async (_, id) => new Promise((resolve) => { pending.set(id, resolve); }),
      onProgress: (records) => published.push(records.map((record) => record.sequence)),
    });
    await vi.waitFor(() => expect(pending.size).toBe(8));
    for (let start = 0; start < 64; start += 8) {
      for (let index = start + 7; index >= start; index--) pending.get(`progress-${index}`)!(envelopes[index]);
      if (start < 56) await vi.waitFor(() => expect(pending.has(`progress-${start + 8}`)).toBe(true));
    }
    const verifiedPrefix = Array.from({ length: 64 }, (_, i) => i + 1).filter((sequence) => sequence !== 2);
    await vi.waitFor(() => expect(published).toEqual([verifiedPrefix]));
    expect(pending.has("progress-64")).toBe(true);
    pending.get("progress-64")!(envelopes[64]);
    expect((await read).map((record) => record.sequence)).toEqual([...verifiedPrefix, 65]);
    expect(published.map((batch) => batch.length)).toEqual([63, 64]);
  });

  it("folds sealed task records and skips one whose envelope was signed by another actor", async () => {
    const claimed = await sealed("e1", "task.claimed", "gh-claude-4", { issue: 9, lane: "gh-claude-4", branch: "issue-9-x" });
    const forged = await sealed("e2", "task.pr_opened", "gh-claude-4", { pr: 10, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" });
    const store = new Map([[claimed.evidenceId, claimed], [forged.evidenceId, forged]]);
    const tasks = foldTaskEvents(await readTaskEventRecords({
      executionId: "gh-team",
      events: [event(1, "gh-claude-4", "task.claimed", claimed), event(2, "agent-chat", "task.pr_opened", forged)],
      readEvidence: async (_, id) => store.get(id)!,
    }));
    expect(tasks).toHaveLength(1);
    expect(tasks[0]).toMatchObject({ taskId: "issue-9", issue: 9, lane: "gh-claude-4", step: "plan", pr: null });
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
    const tasks = foldTaskEvents(await readTaskEventRecords({
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
    }));
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

  it("joins the latest open claim and absorbs earlier claims of the same lane", () => {
    const claim = (sequence: number, branch: string) =>
      record(sequence, "task.claimed", "gh-claude-4", { ...issue, issue: 356, lane: "gh-claude-4", branch });
    const tasks = foldTaskEvents([
      claim(1, "issue-356-heal"), claim(2, "issue-356-explore"),
      record(3, "task.pr_opened", "gh-claude-4", { ...issue, pr: 456, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" }),
    ]);
    expect(tasks.map((task) => [task.branch, task.pr, task.step]))
      .toEqual([["issue-356-explore", 456, "review"]]);
  });

  it("keeps a later head of the same PR on the latest claim (#859)", () => {
    const claim = (sequence: number) =>
      record(sequence, "task.claimed", "codex-2", { taskId: "issue-830", issue: 830, lane: "codex-2", branch: "issue-830-fix" });
    const opened = (sequence: number, headSha: string) =>
      record(sequence, "task.pr_opened", "codex-2", { taskId: "issue-830", pr: 854, headSha, journeys: [], lane: "codex-2" });
    const tasks = foldTaskEvents([claim(1), claim(2), opened(3, "aaaaaaaa"), opened(4, "bbbbbbbb")]);

    expect(tasks.filter((task) => task.pr === 854)).toHaveLength(1);
    expect(tasks.find((task) => task.pr === 854)).toMatchObject({ headSha: "bbbbbbbb", branch: "issue-830-fix" });
    expect(tasks.filter((task) => task.pr === null)).toHaveLength(0);
  });

  it("shows the second claim as its own slice before it has a PR", () => {
    const tasks = foldTaskEvents(records.slice(0, 4));
    expect(tasks.map((task) => [task.pr, task.step])).toEqual([[456, "review"], [null, "plan"]]);
  });

  it("removes a closed PR slice from the work model without ending a sibling PR", () => {
    const opened = record(1, "task.pr_opened", "codex-6", { taskId: "issue-685", pr: 685, headSha: "aaaaaaaa", journeys: [], lane: "codex-6" });
    const sibling = record(2, "task.pr_opened", "codex-6", { taskId: "issue-685", pr: 688, headSha: "bbbbbbbb", journeys: [], lane: "codex-6" });
    const closed = record(3, "task.closed", "codex-6", { taskId: "issue-685", pr: 685, reason: "superseded", by: 740 });
    expect(foldTaskEvents([opened, sibling, closed]).map((task) => task.pr)).toEqual([688]);
  });

  it("restores only the closed PR slice reopened by its author (#1003)", () => {
    const issue = { taskId: "issue-1003" };
    const opened = record(1, "task.pr_opened", "codex-1", { ...issue, pr: 1004, headSha: "aaaaaaaa", journeys: [], lane: "codex-1" });
    const sibling = record(2, "task.pr_opened", "codex-1", { ...issue, pr: 1005, headSha: "bbbbbbbb", journeys: [], lane: "codex-1" });
    const closed = record(3, "task.closed", "coordinator", { ...issue, pr: 1004, reason: "superseded", by: 1006 });
    const siblingClosed = record(4, "task.closed", "coordinator", { ...issue, pr: 1005, reason: "superseded", by: 1006 });
    const reopened = record(5, "task.pr_opened", "codex-1", { ...issue, pr: 1004, headSha: "cccccccc", journeys: [], lane: "codex-1" });
    const approved = record(6, "task.review_verdict", "codex-review", { ...issue, pr: 1004, headSha: "cccccccc", reviewer: "codex-review", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/1004#c1" });
    const merged = record(7, "task.merged", "codex-review", { ...issue, pr: 1004, mergeSha: "dddddddd", closes: [], merger: "codex-review" });
    const repeated = record(8, "task.pr_opened", "codex-1", { ...issue, pr: 1004, headSha: "eeeeeeee", journeys: [], lane: "codex-1" });

    expect(foldTaskEvents([opened, sibling, closed, siblingClosed])).toEqual([]);
    expect(foldTaskEvents([opened, sibling, closed, siblingClosed, reopened]))
      .toMatchObject([{ pr: 1004, headSha: "cccccccc", step: "review" }]);
    expect(foldTaskEvents([opened, sibling, closed, siblingClosed, reopened, approved]))
      .toMatchObject([{ pr: 1004, headSha: "cccccccc", step: "merge", reviewers: ["codex-review"] }]);
    expect(foldTaskEvents([opened, sibling, closed, siblingClosed, reopened, approved, merged, repeated]))
      .toMatchObject([{ pr: 1004, headSha: "cccccccc", step: "merged", mergeSha: "dddddddd" }]);
  });
});

describe("merges recorded by another lane (#765)", () => {
  it("absorbs a same-lane re-claim into its existing PR before merge (#871)", () => {
    const events = [
      record(4417, "task.claimed", "codex-4", { taskId: "issue-86", issue: 86, lane: "codex-4", branch: "issue-86-live-tree" }),
      record(4422, "task.pr_opened", "codex-4", { taskId: "issue-86", pr: 658, headSha: "aaaaaaaa", journeys: [], lane: "codex-4" }),
      record(4426, "task.claimed", "codex-4", { taskId: "issue-86", issue: 86, lane: "codex-4", branch: "issue-86-live-tree" }),
      record(4430, "task.pr_opened", "codex-4", { taskId: "issue-86", pr: 658, headSha: "bbbbbbbb", journeys: [], lane: "codex-4" }),
      record(4437, "task.merged", "coordinator", { taskId: "pr-658", pr: 658, mergeSha: "cccccccc", closes: [], merger: "coordinator" }),
    ];
    expect(foldTaskEvents(events.slice(0, 4)).filter((task) => task.pr === null)).toHaveLength(0);
    const tasks = foldTaskEvents(events);
    expect(tasks.filter((task) => task.pr === null)).toHaveLength(0);
    expect(tasks.filter((task) => task.pr === 658)).toMatchObject([{ step: "merged", mergeSha: "cccccccc" }]);
  });

  it("joins the latest pre-PR claim and absorbs earlier same-lane claims (#871)", () => {
    const events = [
      record(5299, "task.claimed", "codex-2", { taskId: "issue-830", issue: 830, lane: "codex-2", branch: "issue-830-proof" }),
      record(5409, "task.claimed", "codex-2", { taskId: "issue-830", issue: 830, lane: "codex-2", branch: "issue-830-proof" }),
      record(5430, "task.pr_opened", "codex-2", { taskId: "issue-830", pr: 854, headSha: "aaaaaaaa", journeys: [], lane: "codex-2" }),
      record(5460, "task.pr_opened", "codex-2", { taskId: "issue-830", pr: 854, headSha: "bbbbbbbb", journeys: [], lane: "codex-2" }),
      record(5618, "task.merged", "coordinator", { taskId: "pr-854", pr: 854, mergeSha: "cccccccc", closes: [], merger: "coordinator" }),
    ];
    const tasks = foldTaskEvents(events);
    expect(tasks).toHaveLength(1);
    expect(tasks[0]).toMatchObject({ claimSequence: 5409, pr: 854, step: "merged", mergeSha: "cccccccc" });
  });

  it("ends both claims of one PR when its merge closes only the first issue (#807)", () => {
    const events = [
      record(1, "task.claimed", "codex-14", { taskId: "issue-691", issue: 691, lane: "codex-14", branch: "issue-691-fix" }),
      record(2, "task.pr_opened", "codex-14", { taskId: "issue-691", pr: 9, headSha: "aaaaaaaa", journeys: [], lane: "codex-14" }),
      record(3, "task.claimed", "codex-14", { taskId: "issue-722", issue: 722, lane: "codex-14", branch: "issue-722-followup" }),
      record(4, "task.pr_opened", "codex-14", { taskId: "issue-722", pr: 9, headSha: "aaaaaaaa", journeys: [], lane: "codex-14" }),
      record(5, "task.merged", "coordinator", { taskId: "pr-9", pr: 9, mergeSha: "bbbbbbbb", closes: [691], merger: "coordinator" }),
    ];
    const tasks = foldTaskEvents(events);
    expect(tasks.filter((task) => task.pr === 9)).toHaveLength(2);
    expect(tasks.filter((task) => task.pr === 9).map((task) => [task.issue, task.step, task.mergeSha, task.lastSequence]).sort((a, b) => Number(a[0]) - Number(b[0])))
      .toEqual([[691, "merged", "bbbbbbbb", 5], [722, "merged", "bbbbbbbb", 5]]);
  });

  it("removes the closed PR slice while a duplicate claim stays PR-less (#807, #859)", () => {
    const events = [
      record(1, "task.claimed", "codex-4", { taskId: "issue-86", issue: 86, lane: "codex-4", branch: "issue-86-first" }),
      record(2, "task.pr_opened", "codex-4", { taskId: "issue-86", pr: 9, headSha: "aaaaaaaa", journeys: [], lane: "codex-4" }),
      record(3, "task.claimed", "codex-4", { taskId: "issue-86", issue: 86, lane: "codex-4", branch: "issue-86-second" }),
      record(4, "task.pr_opened", "codex-4", { taskId: "issue-86", pr: 9, headSha: "aaaaaaaa", journeys: [], lane: "codex-4" }),
      record(5, "task.closed", "coordinator", { taskId: "pr-9", pr: 9, reason: "superseded", by: 10 }),
    ];
    expect(foldTaskEvents(events.slice(0, 4)).filter((task) => task.pr === 9)).toHaveLength(1);
    expect(foldTaskEvents(events).filter((task) => task.pr === 9)).toHaveLength(0);
  });

  it("ends a duplicate-PR claim when the existing PR slice ends (#795)", () => {
    const tasks = foldTaskEvents([
      record(1, "task.claimed", "lane-a", { taskId: "issue-1", issue: 1, lane: "lane-a", branch: "issue-1-first" }),
      record(2, "task.pr_opened", "lane-a", { taskId: "issue-1", pr: 9, headSha: "aaaaaaaa", journeys: [], lane: "lane-a" }),
      record(3, "task.claimed", "lane-b", { taskId: "issue-1", issue: 1, lane: "lane-b", branch: "issue-1-duplicate" }),
      record(4, "task.pr_opened", "lane-b", { taskId: "issue-1", pr: 9, headSha: "cccccccc", journeys: [], lane: "lane-b" }),
      record(5, "task.merged", "coordinator", { taskId: "pr-9", pr: 9, mergeSha: "bbbbbbbb", closes: [1], merger: "coordinator" }),
    ]);

    expect(tasks.find((task) => task.lane === "lane-b" && task.pr === 9)).toMatchObject({ step: "merged", mergeSha: "bbbbbbbb" });
  });

  it("ends the opened PR slice even when the merge uses a PR task id", () => {
    const tasks = foldTaskEvents([
      record(1, "task.claimed", "codex-14", { taskId: "issue-691", issue: 691, lane: "codex-14", branch: "issue-691-fix" }),
      record(2, "task.pr_opened", "codex-14", { taskId: "issue-691", pr: 722, headSha: "aaaaaaaa", journeys: [], lane: "codex-14" }),
      record(3, "task.review_assigned", "codex-14", { taskId: "issue-691", pr: 722, headSha: "aaaaaaaa", reviewer: "gh-claude-1" }),
      record(4, "task.merged", "coordinator", { taskId: "pr-722", pr: 722, mergeSha: "bbbbbbbb", closes: [691], merger: "coordinator" }),
    ]);
    expect(tasks.filter((task) => task.pr === 722)).toHaveLength(1);
    expect(tasks.find((task) => task.pr === 722)).toMatchObject({ issue: 691, lane: "codex-14", step: "merged", mergeSha: "bbbbbbbb" });
  });

  it("ends a PR-less claim only when the merge closes its issue", () => {
    const tasks = foldTaskEvents([
      record(1, "task.claimed", "codex-13", { taskId: "issue-758", issue: 758, lane: "codex-13", branch: "issue-758-fix" }),
      record(2, "task.claimed", "codex-12", { taskId: "issue-759", issue: 759, lane: "codex-12", branch: "issue-759-work" }),
      record(3, "task.merged", "coordinator", { taskId: "pr-760", pr: 760, mergeSha: "cccccccc", closes: [758], merger: "coordinator" }),
    ]);
    expect(tasks.find((task) => task.issue === 758)).toMatchObject({ pr: null, step: "merged" });
    expect(tasks.find((task) => task.issue === 759)).toMatchObject({ pr: null, step: "plan" });
  });
});

/* #480: a claimed task is planning until the lane records its keel plan (`task.planned`); a
 * `design` plan then waits on its critic (#467) before Implement. Catches a fold that lights
 * Implement on the claim alone (no Plan step), one that skips the Critic a design plan asks for,
 * one that sends a PR under review back to Plan when the plan is recorded late, and a planned
 * record that names another lane or carries an out-of-bounds field. Cost: pure functions. */
describe("foldTaskEvents plan step (#480)", () => {
  const issue = { taskId: "issue-480" };
  const claim = record(1, "task.claimed", "gh-claude-1", { ...issue, issue: 480, lane: "gh-claude-1", branch: "issue-480-plan-step" });
  const plan = (sequence: number, mode: "none" | "design") => record(sequence, "task.planned", "gh-claude-1", {
    ...issue, lane: "gh-claude-1", classes: ["user_visible"], reviews: 1, proof: "both",
    critic: { mode, passScore: 8, maxRounds: 3 }, summary: "Record the plan; light Plan.",
  });
  const opened = (sequence: number) => record(sequence, "task.pr_opened", "gh-claude-1", { ...issue, pr: 481, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-1" });

  it("lights Plan on the claim, then Implement (or Critic for a design plan) once the plan is recorded", () => {
    expect(foldTaskEvents([claim])[0].step).toBe("plan");
    const coded = foldTaskEvents([claim, plan(2, "none")])[0];
    expect(coded).toMatchObject({ step: "implement", plan: { summary: "Record the plan; light Plan.", critic: { mode: "none" } } });
    expect(foldTaskEvents([claim, plan(2, "design")])[0].step).toBe("critic");
    expect(foldTaskEvents([claim, plan(2, "design"), opened(3)])[0].step).toBe("review");
  });

  it("ends Critic only on a passing critic round; revise and exhausted keep it lit", () => {
    const round = (sequence: number, n: number, score: number, verdict: string) => record(sequence, "task.critic_verdict", "gh-claude-1", {
      ...issue, lane: "gh-claude-1", round: n, score, passScore: 8, maxRounds: 3, verdict, designRef: "d", reasons: ["r"] });
    expect(foldTaskEvents([claim, plan(2, "design"), round(3, 1, 5, "revise")])[0].step).toBe("critic");
    expect(foldTaskEvents([claim, plan(2, "design"), round(3, 1, 5, "revise"), round(4, 2, 9, "pass")])[0].step).toBe("implement");
    expect(foldTaskEvents([claim, plan(2, "design"), round(3, 3, 5, "exhausted")])[0].step).toBe("critic");
  });

  // gh-claude-10's BLOCK on 2bba9de4: two lanes hold PR-less claims of one issue (a handover);
  // lane-b's plan and critic round must land on lane-b's slice, not on lane-a's older one.
  it("lands a lane's plan and critic round on that lane's own claim, not another lane's", () => {
    const claimOf = (sequence: number, lane: string) => record(sequence, "task.claimed", lane, { ...issue, issue: 480, lane, branch: `issue-480-${lane}` });
    const planOf = (sequence: number, lane: string) => record(sequence, "task.planned", lane, { ...issue, lane, classes: ["code"], reviews: 1,
      proof: "tests", critic: { mode: "design", passScore: 8, maxRounds: 3 }, summary: `${lane}'s plan` });
    const roundOf = (sequence: number, lane: string) => record(sequence, "task.critic_verdict", lane, { ...issue, lane, round: 1, score: 9,
      passScore: 8, maxRounds: 3, verdict: "pass", designRef: "d", reasons: ["r"] });
    const tasks = foldTaskEvents([claimOf(1, "lane-a"), claimOf(2, "lane-b"), planOf(3, "lane-b"), roundOf(4, "lane-b")]);
    const byLane = Object.fromEntries(tasks.map((task) => [task.lane, task]));
    expect(byLane["lane-a"]).toMatchObject({ step: "plan", plan: null });
    expect(byLane["lane-b"]).toMatchObject({ step: "implement", plan: { summary: "lane-b's plan" } });
  });

  it("keeps a plan recorded after the PR without moving the graph back", () => {
    const state = foldTaskEvents([claim, opened(2), plan(3, "design")]);
    expect(state).toHaveLength(1);
    expect(state[0]).toMatchObject({ step: "review", plan: { critic: { mode: "design" } } });
  });

  it("starts a new lane-owned slice for plan and critic records after merge", () => {
    const claim = record(1, "task.claimed", "lane-a", { ...issue, issue: 480, lane: "lane-a", branch: "first" });
    const opened = record(2, "task.pr_opened", "lane-a", { ...issue, pr: 481, headSha: "aaaaaaaa", journeys: [], lane: "lane-a" });
    const merged = record(3, "task.merged", "reviewer", { ...issue, pr: 481, mergeSha: "bbbbbbbb", closes: [], merger: "reviewer" });
    const foreign = record(4, "task.claimed", "lane-b", { ...issue, issue: 480, lane: "lane-b", branch: "foreign" });
    const plan = record(5, "task.planned", "lane-a", { ...issue, lane: "lane-a", classes: ["code"], reviews: 1,
      proof: "tests", critic: { mode: "design", passScore: 8, maxRounds: 3 }, summary: "next plan" });
    const round = record(6, "task.critic_verdict", "lane-a", { ...issue, lane: "lane-a", round: 1, score: 9,
      passScore: 8, maxRounds: 3, verdict: "pass", designRef: "d", reasons: ["r"] });
    const tasks = foldTaskEvents([claim, opened, merged, foreign, plan, round]);
    expect(tasks).toHaveLength(3);
    expect(tasks.find((task) => task.pr === 481)).toMatchObject({ step: "merged", mergeSha: "bbbbbbbb", plan: null, critic: null });
    expect(tasks.find((task) => task.branch === "foreign")).toMatchObject({ lane: "lane-b", pr: null, plan: null });
    expect(tasks.find((task) => task.lane === "lane-a" && task.pr === null)).toMatchObject({ step: "implement", plan: { summary: "next plan" }, critic: { verdict: "pass" } });
  });

  it("omits cascade-merged siblings from typical-time samples", () => {
    const events = [
      record(1, "task.claimed", "lane-a", { taskId: "issue-1", issue: 1, lane: "lane-a", branch: "first" }),
      record(2, "task.pr_opened", "lane-a", { taskId: "issue-1", pr: 10, headSha: "aaaaaaaa", journeys: [], lane: "lane-a" }),
      record(3, "task.claimed", "lane-b", { taskId: "issue-2", issue: 2, lane: "lane-b", branch: "second" }),
      record(4, "task.pr_opened", "lane-b", { taskId: "issue-2", pr: 11, headSha: "bbbbbbbb", journeys: [], lane: "lane-b" }),
      record(5, "task.review_verdict", "reviewer", { taskId: "issue-1", pr: 10, headSha: "aaaaaaaa", reviewer: "reviewer", verdict: "APPROVE", commentUrl: "https://github.com/o/r/pull/10#c1" }),
      record(6, "task.merged", "reviewer", { taskId: "issue-1", pr: 10, mergeSha: "cccccccc", closes: [], merger: "reviewer" }),
    ];
    const tasks = foldTaskEvents(events);
    expect(tasks.filter((task) => task.step === "merged")).toHaveLength(2);
    expect(timingSamples(tasks)).toHaveLength(1);
    expect(timingSamples(tasks)[0]).toEqual(tasks.find((task) => task.pr === 10)?.clock);
  });

  it("refuses a planned record for another lane or out of the schema's bounds", () => {
    const parse = (actor: string, document: Record<string, unknown>) => parseTaskEvent("task.planned", actor, JSON.stringify({
      schema: "graphhelm-task-event-v1", taskId: "issue-480", revision: 1, at: "2026-10-08T00:00:00Z", lane: "gh-claude-1",
      classes: ["code"], reviews: 1, proof: "tests", critic: { mode: "none", passScore: 8, maxRounds: 3 }, summary: "s", ...document,
    }));
    expect(parse("gh-claude-1", {})).not.toBeNull();
    expect(parse("gh-claude-2", {})).toBeNull();
    expect(parse("gh-claude-1", { classes: ["code", "code"] })).toBeNull();
    expect(parse("gh-claude-1", { critic: { mode: "design", passScore: 11, maxRounds: 3 } })).toBeNull();
    expect(parse("gh-claude-1", { summary: "x".repeat(301) })).toBeNull();
  });
});
