import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TaskGraphs } from "./task-graphs";
import { foldTaskEvents, parseTaskEvent, type TaskEventRecord } from "../runtime/team-tasks";

/* #391 (spec §7, §9 row G): the Team tab draws one small graph per task from the `task.*` fold.
 * These cells observe what the owner reads off it: which step is lit, which agent stands on each
 * node, and an unanswered BLOCK as a red edge with its reviewer, plus the links out. They catch a
 * view that lights the wrong step, drops the agent names, or loses the block once a new head is
 * pushed. Cost: jsdom only, no I/O, well under a second. */

function record(sequence: number, taskId: string, kind: string, actorId: string, fields: Record<string, unknown>): TaskEventRecord {
  const parsed = parseTaskEvent(kind, actorId, JSON.stringify({ schema: "graphhelm-task-event-v1", taskId, revision: sequence, at: "2026-10-07T20:00:00Z", ...fields }));
  if (parsed === null) throw new Error(`fixture ${kind} did not parse`);
  return { ...parsed, sequence };
}

const commentUrl = "https://github.com/stabem/GraphHelm/pull/388#issuecomment-1";

function twoTasks() {
  return foldTaskEvents([
    record(1, "issue-386", "task.claimed", "gh-claude-4", { issue: 386, lane: "gh-claude-4", branch: "issue-386-task-events" }),
    record(2, "issue-386", "task.pr_opened", "gh-claude-4", { pr: 388, headSha: "aaaaaaaa", journeys: ["studio-see-team"], lane: "gh-claude-4" }),
    record(3, "issue-386", "task.review_verdict", "gh-claude-1", { pr: 388, headSha: "aaaaaaaa", reviewer: "gh-claude-1", verdict: "BLOCK", commentUrl }),
    record(4, "issue-386", "task.pr_opened", "gh-claude-4", { pr: 388, headSha: "bbbbbbbb", journeys: ["studio-see-team"], lane: "gh-claude-4" }),
    record(5, "issue-380", "task.claimed", "gh-claude-2", { issue: 380, lane: "gh-claude-2", branch: "issue-380-owner-credential" }),
  ]);
}

describe("TaskGraphs", () => {
  it("lights the current step, names the agent on each node and draws an open BLOCK as a red edge", () => {
    render(<TaskGraphs tasks={twoTasks()} onOpenJourney={vi.fn()} />);
    const blocked = screen.getByRole("group", { name: /issue #386/i });
    const lit = within(blocked).getByRole("listitem", { current: "step" });
    expect(lit).toHaveTextContent(/review/i);
    expect(lit).toHaveTextContent("gh-claude-1");
    expect(within(blocked).getByText("gh-claude-4")).toBeInTheDocument();
    const edge = within(blocked).getByRole("link", { name: /blocked by gh-claude-1/i });
    expect(edge).toHaveAttribute("href", commentUrl);
    expect(within(blocked).getByRole("link", { name: "PR #388" })).toHaveAttribute("href", "https://github.com/stabem/GraphHelm/pull/388");

    const other = screen.getByRole("group", { name: /issue #380/i });
    expect(within(other).getByRole("listitem", { current: "step" })).toHaveTextContent(/implement/i);
    expect(within(other).getByText("gh-claude-2")).toBeInTheDocument();
    expect(within(other).queryByRole("link", { name: /blocked by/i })).toBeNull();
  });

  it("opens the Journey tab on a journey the PR named", () => {
    const onOpenJourney = vi.fn();
    render(<TaskGraphs tasks={twoTasks()} onOpenJourney={onOpenJourney} />);
    fireEvent.click(screen.getByRole("button", { name: "studio-see-team" }));
    expect(onOpenJourney).toHaveBeenCalledWith("studio-see-team");
  });

  it("never turns a recorded comment URL that is not a github.com page into a link", () => {
    const tasks = foldTaskEvents([
      record(1, "issue-9", "task.pr_opened", "gh-claude-4", { pr: 9, headSha: "aaaaaaaa", journeys: [], lane: "gh-claude-4" }),
      record(2, "issue-9", "task.review_verdict", "gh-claude-1", { pr: 9, headSha: "aaaaaaaa", reviewer: "gh-claude-1", verdict: "BLOCK", commentUrl: "javascript:alert(1)" }),
    ]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} />);
    expect(screen.getByText(/blocked by gh-claude-1/)).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /blocked by/ })).toBeNull();
  });

  it("shows a late verdict on an older, recorded head as superseded, not as missing (#459 a)", () => {
    const tasks = foldTaskEvents([
      record(1, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: "a".repeat(40), journeys: [], lane: "gh-claude-2" }),
      record(2, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: "b".repeat(40), journeys: [], lane: "gh-claude-2" }),
      record(3, "issue-9", "task.review_verdict", "gh-claude-5", { pr: 19, headSha: "a".repeat(40), reviewer: "gh-claude-5", verdict: "APPROVE", commentUrl }),
    ]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} />);
    const graph = screen.getByRole("group", { name: /PR #19/i });
    expect(within(graph).getByText(/APPROVE by gh-claude-5 on aaaaaaaa, superseded by bbbbbbbb/)).toBeInTheDocument();
    expect(within(graph).queryByText(/no pr_opened record/)).toBeNull();
    expect(within(graph).getByRole("listitem", { current: "step" })).toHaveTextContent(/review/i);
  });

  it("applies a verdict recorded before its own pr_opened once that pr_opened arrives (#459 b)", () => {
    const tasks = foldTaskEvents([
      record(1, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: "a".repeat(40), journeys: [], lane: "gh-claude-2" }),
      record(2, "issue-9", "task.review_verdict", "gh-claude-5", { pr: 19, headSha: "b".repeat(40), reviewer: "gh-claude-5", verdict: "APPROVE", commentUrl }),
      record(3, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: "b".repeat(40), journeys: [], lane: "gh-claude-2" }),
    ]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} />);
    const graph = screen.getByRole("group", { name: /PR #19/i });
    expect(within(graph).getByRole("listitem", { current: "step" })).toHaveTextContent(/merge/i);
    expect(within(graph).getByText("gh-claude-5")).toBeInTheDocument();
    expect(within(graph).queryByText(/no pr_opened record|superseded/)).toBeNull();
  });

  it("names a verdict on a head that has no pr_opened record instead of dropping it (#457)", () => {
    const tasks = foldTaskEvents([
      record(1, "issue-439", "task.pr_opened", "gh-claude-2", { pr: 449, headSha: "8aeaef0e380e0eb487b810e994f1b9d8d01f077e", journeys: [], lane: "gh-claude-2", repo: "stabem/GraphHelm" }),
      record(2, "issue-439", "task.review_assigned", "gh-claude-2", { pr: 449, headSha: "8aeaef0e380e0eb487b810e994f1b9d8d01f077e", reviewer: "gh-claude-6", ordinal: 1 }),
      record(3, "issue-439", "task.review_verdict", "gh-claude-5", { pr: 449, headSha: "a09b9aa343ba80f78b437b7b38e873950ede2b37", reviewer: "gh-claude-5", verdict: "APPROVE", commentUrl: "https://github.com/stabem/GraphHelm/pull/449#issuecomment-6059525901" }),
    ]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} />);
    const graph = screen.getByRole("group", { name: /PR #449/i });
    expect(within(graph).getByText(/APPROVE by gh-claude-5 on a09b9aa3, a head with no pr_opened record/)).toBeInTheDocument();
  });

  it("prints the merge sha short and links it to the commit when the repository is known (#458)", () => {
    const merge = "e6c910cfb63cd65cb6d89aea7c285b4037b2c797";
    const tasks = foldTaskEvents([
      record(1, "issue-439", "task.claimed", "gh-claude-2", { issue: 439, lane: "gh-claude-2", branch: "issue-439-x", repo: "stabem/GraphHelm" }),
      record(2, "issue-439", "task.merged", "gh-claude-5", { pr: 449, mergeSha: merge, closes: [439], merger: "gh-claude-5" }),
    ]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} />);
    const graph = screen.getByRole("group", { name: /issue #439/i });
    expect(within(graph).queryByText(merge)).toBeNull();
    expect(within(graph).getByRole("link", { name: "e6c910cf" })).toHaveAttribute("href", `https://github.com/stabem/GraphHelm/commit/${merge}`);
  });

  it("renders nothing when no task has been recorded", () => {
    const { container } = render(<TaskGraphs tasks={[]} onOpenJourney={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });
});
