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

  it("renders nothing when no task has been recorded", () => {
    const { container } = render(<TaskGraphs tasks={[]} onOpenJourney={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });
});
