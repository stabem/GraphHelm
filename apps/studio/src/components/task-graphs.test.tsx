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
    // The author stands on Implement and, since #514, on the Fix of the BLOCK round.
    expect(within(blocked).getAllByText("gh-claude-4").length).toBeGreaterThanOrEqual(1);
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
    fireEvent.click(screen.getByRole("link", { name: "studio-see-team" }));
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

/* #481 (owner, on the live Team tab): the active lanes sat at the bottom under ~15 merged rows.
 * Rows are ordered by state — blocked, then in review, then implementing, newest activity first —
 * and delivered tasks go last, collapsed under "Delivered (N)". Cost: jsdom only. */
describe("TaskGraphs order (#481)", () => {
  const head = "a".repeat(40);
  const claim = (n: number) => record(n * 10, `issue-${n}`, "task.claimed", "gh-claude-2", { issue: n, lane: "gh-claude-2", branch: `issue-${n}-x` });
  const opened = (n: number, seq: number) => record(seq, `issue-${n}`, "task.pr_opened", "gh-claude-2", { pr: n + 100, headSha: head, journeys: [], lane: "gh-claude-2" });
  const number = (group: HTMLElement) => /#(\d+)/.exec(group.getAttribute("aria-label") ?? "")?.[1];

  it("puts blocked, then in review, then implementing first, newest first, and folds delivered tasks last", () => {
    const tasks = foldTaskEvents([
      claim(1),
      claim(2), opened(2, 21),
      claim(3), opened(3, 31), record(32, "issue-3", "task.review_verdict", "gh-claude-5", { pr: 103, headSha: head, reviewer: "gh-claude-5", verdict: "BLOCK", commentUrl }),
      claim(4), opened(4, 41), record(42, "issue-4", "task.merged", "gh-claude-5", { pr: 104, mergeSha: "c".repeat(40), closes: [4], merger: "gh-claude-5" }),
      claim(5), opened(5, 51), record(52, "issue-5", "task.merged", "gh-claude-5", { pr: 105, mergeSha: "d".repeat(40), closes: [5], merger: "gh-claude-5" }),
      claim(6),
    ]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} />);
    const delivered = screen.getByText("Delivered (2)").closest("details")!;
    expect(delivered).not.toHaveAttribute("open");
    const active = screen.getAllByRole("group").filter((group) => !delivered.contains(group));
    expect(active.map(number)).toEqual(["3", "2", "6", "1"]);
    expect(within(delivered).getAllByRole("group").map(number)).toEqual(["5", "4"]);
  });

  it("moves a row to its new place when a record arrives", () => {
    const { rerender } = render(<TaskGraphs tasks={foldTaskEvents([claim(7), claim(8)])} onOpenJourney={vi.fn()} />);
    expect(screen.getAllByRole("group").map(number)).toEqual(["8", "7"]);
    rerender(<TaskGraphs tasks={foldTaskEvents([claim(7), claim(8), opened(7, 99)])} onOpenJourney={vi.fn()} />);
    expect(screen.getAllByRole("group").map(number)).toEqual(["7", "8"]);
  });
});

/* #477 (owner, on the live Team tab): a row said only "#454". It now reads "#454" (linked to the
 * issue) · the issue's title, with its one-line summary under it; a PR shows its own title without
 * the `type(area):` prefix; journeys are links into the Journey tab; the confusing stray-verdict
 * lines sit behind a details toggle; and a row that just changed is marked. Cost: jsdom only, well under a second. */
describe("TaskGraphs titles (#477)", () => {
  const repo = "stabem/GraphHelm";
  function task(n: number, extra: TaskEventRecord[] = []) {
    return [
      record(n * 10, `issue-${n}`, "task.claimed", "gh-claude-2", { issue: n, lane: "gh-claude-2", branch: `issue-${n}-x`, repo,
        title: `Studio: task ${n} title`, summary: `The owner gets thing ${n}.` }),
      ...extra,
    ];
  }

  it("shows the issue number as a link, the title, and the summary line", () => {
    render(<TaskGraphs tasks={foldTaskEvents(task(477))} onOpenJourney={vi.fn()} />);
    const row = screen.getByRole("group", { name: /issue #477/i });
    expect(within(row).getByRole("link", { name: "#477" })).toHaveAttribute("href", "https://github.com/stabem/GraphHelm/issues/477");
    const heading = within(row).getByText("Studio: task 477 title");
    expect(heading).toHaveAttribute("title", "Studio: task 477 title");
    expect(within(row).getByText("The owner gets thing 477.")).toBeInTheDocument();
  });

  it("accepts a title of 200 characters even when accents or emoji make it longer in bytes or UTF-16 units", () => {
    const title = Array.from("Correção 🚀 ".repeat(20)).slice(0, 200).join("");
    expect(Array.from(title)).toHaveLength(200);
    const tasks = foldTaskEvents([record(1, "issue-9", "task.claimed", "gh-claude-2", { issue: 9, lane: "gh-claude-2", branch: "b", repo, title })]);
    expect(tasks[0].title).toBe(title);
    expect(parseTaskEvent("task.claimed", "gh-claude-2", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-9", revision: 1,
      at: "2026-10-08T00:00:00Z", issue: 9, lane: "gh-claude-2", branch: "b", title: "ç".repeat(201) }))).toBeNull();
  });

  it("falls back to the number when no title was recorded", () => {
    const tasks = foldTaskEvents([record(1, "issue-5", "task.claimed", "gh-claude-2", { issue: 5, lane: "gh-claude-2", branch: "b", repo })]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} />);
    expect(within(screen.getByRole("group", { name: /issue #5/i })).getByRole("link", { name: "#5" })).toBeInTheDocument();
  });

  it("shows the PR's own title without its type(area) prefix, and journeys as links", () => {
    const onOpenJourney = vi.fn();
    const tasks = foldTaskEvents(task(478, [
      record(4781, "issue-478", "task.pr_opened", "gh-claude-2", { pr: 479, headSha: "a".repeat(40), journeys: ["studio-see-team"], lane: "gh-claude-2", repo,
        title: "feat(studio): team tab shows task titles", summary: "The owner reads titles." }),
    ]));
    render(<TaskGraphs tasks={tasks} onOpenJourney={onOpenJourney} />);
    const row = screen.getByRole("group", { name: /issue #478/i });
    expect(within(row).getByText(/team tab shows task titles/)).toBeInTheDocument();
    expect(within(row).queryByText(/feat\(studio\)/)).toBeNull();
    fireEvent.click(within(row).getByRole("link", { name: "studio-see-team" }));
    expect(onOpenJourney).toHaveBeenCalledWith("studio-see-team");
  });

  it("keeps stray verdicts behind a details toggle", () => {
    const tasks = foldTaskEvents(task(480, [
      record(4801, "issue-480", "task.pr_opened", "gh-claude-2", { pr: 481, headSha: "a".repeat(40), journeys: [], lane: "gh-claude-2" }),
      record(4802, "issue-480", "task.review_verdict", "gh-claude-5", { pr: 481, headSha: "b".repeat(40), reviewer: "gh-claude-5", verdict: "APPROVE", commentUrl }),
    ]));
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} />);
    const stray = screen.getByText(/a head with no pr_opened record/);
    expect(stray.closest("details")).not.toBeNull();
    expect(stray.closest("details")).not.toHaveAttribute("open");
  });

  it("marks a row that just changed and moves it live", () => {
    const head = "a".repeat(40);
    const before = foldTaskEvents([...task(7), ...task(8)]);
    const { rerender } = render(<TaskGraphs tasks={before} onOpenJourney={vi.fn()} />);
    const after = foldTaskEvents([...task(7), ...task(8),
      record(99, "issue-7", "task.pr_opened", "gh-claude-2", { pr: 17, headSha: head, journeys: [], lane: "gh-claude-2" })]);
    rerender(<TaskGraphs tasks={after} onOpenJourney={vi.fn()} />);
    const rows = screen.getAllByRole("group");
    expect(rows[0]).toHaveAttribute("aria-label", expect.stringMatching(/#7\b/));
    expect(rows[0]).toHaveClass("task-graph-changed");
    expect(rows[1]).not.toHaveClass("task-graph-changed");
  });
});

/* #508 (owner: "tinha q ta o nome dele ali se n ta tem algo indo errado"): a lit Review with no
 * reviewer recorded is a warning, never a blank; and a verdict from a lane nobody assigned still
 * names that lane as the reviewer. Cost: jsdom only. */
describe("TaskGraphs review names its reviewer (#508)", () => {
  const head = "a".repeat(40);
  it("warns on a lit Review with no reviewer recorded", () => {
    const tasks = foldTaskEvents([record(1, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: head, journeys: [], lane: "gh-claude-2" })]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} />);
    const lit = screen.getByRole("listitem", { current: "step" });
    expect(lit).toHaveTextContent(/review/i);
    expect(within(lit).getByText("no reviewer recorded")).toHaveClass("task-node-missing");
  });

  it("names the lane whose verdict arrived without an assignment", () => {
    const tasks = foldTaskEvents([
      record(1, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: head, journeys: [], lane: "gh-claude-2" }),
      record(2, "issue-9", "task.review_verdict", "gh-claude-5", { pr: 19, headSha: head, reviewer: "gh-claude-5", verdict: "BLOCK", commentUrl }),
    ]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} />);
    // Since #514 a BLOCK lights the author's Fix; the reviewer stands on the blocked Review node.
    const review = screen.getAllByRole("listitem").find((node) => /^Review/.test(node.textContent ?? ""))!;
    expect(review).toHaveTextContent("gh-claude-5");
    expect(screen.queryByText("no reviewer recorded")).toBeNull();
  });
});

/* #514 (owner, seeing #356 as two cards for PR #501 and PR #487): one card per issue. Each PR of
 * that issue is its own row inside the card, and a task spawned from another task's finding
 * (`claimed` with `parent`) is a row in the parent's card, marked "found while working on #N". The
 * card's place is its worst open row; it is delivered only when every row is merged. Cost: jsdom. */
describe("TaskGraphs issue cards (#514)", () => {
  const head = "a".repeat(40);
  const claim = (seq: number, issue: number, branch: string, extra: Record<string, unknown> = {}) =>
    record(seq, `issue-${issue}`, "task.claimed", "gh-claude-4", { issue, lane: "gh-claude-4", branch, ...extra });
  const opened = (seq: number, issue: number, pr: number) =>
    record(seq, `issue-${issue}`, "task.pr_opened", "gh-claude-4", { pr, headSha: head, journeys: [], lane: "gh-claude-4" });
  const merged = (seq: number, issue: number, pr: number) =>
    record(seq, `issue-${issue}`, "task.merged", "gh-claude-5", { pr, mergeSha: "c".repeat(40), closes: [], merger: "gh-claude-5" });
  const block = (seq: number, issue: number, pr: number) =>
    record(seq, `issue-${issue}`, "task.review_verdict", "gh-claude-5", { pr, headSha: head, reviewer: "gh-claude-5", verdict: "BLOCK", commentUrl });
  const cards = () => screen.getAllByRole("article");
  const prs = (card: HTMLElement) => within(card).getAllByRole("group").map((row) => within(row).queryByText(/^PR #\d+$/)?.textContent ?? "claim");

  it("draws two PRs of one issue as two rows inside one card", () => {
    render(<TaskGraphs tasks={foldTaskEvents([claim(1, 356, "a"), opened(2, 356, 487), claim(3, 356, "b"), opened(4, 356, 501)])} onOpenJourney={vi.fn()} />);
    expect(cards()).toHaveLength(1);
    expect(cards()[0]).toHaveAttribute("aria-label", expect.stringMatching(/#356/));
    expect(prs(cards()[0]).sort()).toEqual(["PR #487", "PR #501"]);
  });

  it("draws a task found while working on another issue as a row in that issue's card", () => {
    render(<TaskGraphs tasks={foldTaskEvents([claim(1, 356, "a"), opened(2, 356, 487), claim(3, 514, "c", { parent: 356 })])} onOpenJourney={vi.fn()} />);
    expect(cards()).toHaveLength(1);
    const child = within(cards()[0]).getByRole("group", { name: /issue #514/i });
    expect(child).toHaveTextContent("found while working on #356");
  });

  it("gives a child its own card, still marked, when the parent has no card", () => {
    render(<TaskGraphs tasks={foldTaskEvents([claim(3, 514, "c", { parent: 356 })])} onOpenJourney={vi.fn()} />);
    expect(cards()).toHaveLength(1);
    expect(cards()[0]).toHaveTextContent("found while working on #356");
  });

  it("places a card by its worst open row and delivers it only when every row is merged", () => {
    render(<TaskGraphs tasks={foldTaskEvents([
      claim(1, 356, "a"), opened(2, 356, 487), merged(3, 356, 487), claim(4, 356, "b"), opened(5, 356, 501), block(6, 356, 501),
      claim(10, 7, "x"), opened(11, 7, 107),
      claim(20, 8, "y"), opened(21, 8, 108), merged(22, 8, 108),
    ])} onOpenJourney={vi.fn()} />);
    const delivered = screen.getByText("Delivered (1)").closest("details")!;
    const active = cards().filter((card) => !delivered.contains(card));
    expect(active.map((card) => /#(\d+)/.exec(card.getAttribute("aria-label") ?? "")?.[1])).toEqual(["356", "7"]);
    expect(within(delivered).getAllByRole("article")).toHaveLength(1);
  });
});

/* #514 (owner, on a BLOCKed row: "a node for it: re-review and the agent working"): each BLOCK
 * grows the row by a Fix (the author's, lit until a newer head is recorded) and a Re-review (lit on
 * that head), labelled by round, with the blocked review marked and its reason linked. */
describe("TaskGraphs review rounds (#514)", () => {
  const A = "a".repeat(40), B = "b".repeat(40), C = "c".repeat(40);
  const url = (n: number) => `https://github.com/stabem/GraphHelm/pull/19#issuecomment-${n}`;
  const steps = [
    record(1, "issue-9", "task.claimed", "gh-claude-2", { issue: 9, lane: "gh-claude-2", branch: "b" }),
    record(2, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: A, journeys: [], lane: "gh-claude-2" }),
    record(3, "issue-9", "task.review_verdict", "gh-claude-5", { pr: 19, headSha: A, reviewer: "gh-claude-5", verdict: "BLOCK", commentUrl: url(1) }),
    record(4, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: B, journeys: [], lane: "gh-claude-2" }),
    record(5, "issue-9", "task.review_verdict", "gh-claude-5", { pr: 19, headSha: B, reviewer: "gh-claude-5", verdict: "BLOCK", commentUrl: url(2) }),
    record(6, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: C, journeys: [], lane: "gh-claude-2" }),
    record(7, "issue-9", "task.review_verdict", "gh-claude-5", { pr: 19, headSha: C, reviewer: "gh-claude-5", verdict: "APPROVE", commentUrl: url(3) }),
  ];
  const nodes = (n: number) => {
    const { unmount } = render(<TaskGraphs tasks={foldTaskEvents(steps.slice(0, n))} onOpenJourney={vi.fn()} />);
    const items = screen.getAllByRole("listitem").map((item) => {
      const label = item.querySelector(".task-node-label")?.textContent ?? "";
      const state = item.className.replace(/.*task-node-/, "");
      return `${label}:${state}`;
    });
    unmount();
    return items;
  };

  it("grows Fix and Re-review per BLOCK round and lights the right one at each step", () => {
    expect(nodes(3)).toEqual(["Implement:done", "Review ✗:blocked", "Fix · round 1:current", "Re-review · round 1:next", "Merge:next"]);
    expect(nodes(4)).toEqual(["Implement:done", "Review ✗:blocked", "Fix · round 1:done", "Re-review · round 1:current", "Merge:next"]);
    expect(nodes(5)).toEqual(["Implement:done", "Review ✗:blocked", "Fix · round 1:done", "Re-review · round 1 ✗:blocked",
      "Fix · round 2:current", "Re-review · round 2:next", "Merge:next"]);
    expect(nodes(7)).toEqual(["Implement:done", "Review ✗:blocked", "Fix · round 1:done", "Re-review · round 1 ✗:blocked",
      "Fix · round 2:done", "Re-review · round 2:done", "Merge:current"]);
  });

  it("names the author on Fix and the reviewer on each review, with the reason linked", () => {
    render(<TaskGraphs tasks={foldTaskEvents(steps.slice(0, 5))} onOpenJourney={vi.fn()} />);
    const items = screen.getAllByRole("listitem");
    const by = (label: RegExp) => items.find((item) => label.test(item.querySelector(".task-node-label")?.textContent ?? ""))!;
    expect(by(/^Fix · round 2/)).toHaveTextContent("gh-claude-2");
    expect(by(/^Re-review · round 1/)).toHaveTextContent("gh-claude-5");
    expect(within(by(/^Review/)).getByRole("link", { name: "reason" })).toHaveAttribute("href", url(1));
    expect(within(by(/^Re-review · round 1/)).getByRole("link", { name: "reason" })).toHaveAttribute("href", url(2));
  });
});

/* #502 (owner: "a timer to know how long it has been there"): the lit step shows the time in that
 * step, ticking from the Runtime's append time of the record that entered it, and a thin bar
 * against the run's typical time for that step (median over merged slices, 3+ samples), coloured by
 * pace with the typical value in its tooltip. With fewer samples it says so instead of inventing a
 * target. A merged task shows the time it spent in each step. Cost: jsdom only. */
describe("TaskGraphs step timer (#502)", () => {
  const T0 = Date.parse("2026-10-08T10:00:00Z");
  const at = (minutes: number) => new Date(T0 + minutes * 60_000).toISOString();
  const head = "a".repeat(40);
  const timed = (sequence: number, minutes: number, taskId: string, kind: string, actorId: string, fields: Record<string, unknown>) =>
    ({ ...record(sequence, taskId, kind, actorId, fields), occurredAt: at(minutes) });
  /** A merged slice that spent `review` minutes in Review. */
  const done = (n: number, review: number) => [
    timed(n * 10, 0, `issue-${n}`, "task.claimed", "l1", { issue: n, lane: "l1", branch: `b${n}` }),
    timed(n * 10 + 1, 10, `issue-${n}`, "task.pr_opened", "l1", { pr: n + 100, headSha: head, journeys: [], lane: "l1" }),
    timed(n * 10 + 2, 10 + review, `issue-${n}`, "task.review_verdict", "l2", { pr: n + 100, headSha: head, reviewer: "l2", verdict: "APPROVE", commentUrl }),
    timed(n * 10 + 3, 12 + review, `issue-${n}`, "task.merged", "l2", { pr: n + 100, mergeSha: "c".repeat(40), closes: [n], merger: "l2" }),
  ];
  const reviewing = [
    timed(900, 0, "issue-90", "task.claimed", "l1", { issue: 90, lane: "l1", branch: "b90" }),
    timed(901, 100, "issue-90", "task.pr_opened", "l1", { pr: 190, headSha: head, journeys: [], lane: "l1" }),
  ];

  it("shows the time in the lit step and a bar coloured by pace against the typical time", () => {
    const tasks = foldTaskEvents([...done(1, 20), ...done(2, 30), ...done(3, 40), ...reviewing]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} now={T0 + 165 * 60_000} />);
    const row = screen.getByRole("group", { name: /issue #90/i });
    const lit = within(row).getByRole("listitem", { current: "step" });
    expect(lit).toHaveTextContent("in this step: 1 h 05");
    const bar = within(lit).getByRole("meter");
    expect(bar).toHaveAttribute("data-pace", "stuck");
    expect(bar).toHaveAttribute("title", expect.stringMatching(/typical 30 min \(median of 3\)/));
  });

  it("shows only the timer, and says why, with fewer than three past samples", () => {
    const tasks = foldTaskEvents([...done(1, 20), ...reviewing]);
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} now={T0 + 110 * 60_000} />);
    const lit = within(screen.getByRole("group", { name: /issue #90/i })).getByRole("listitem", { current: "step" });
    expect(lit).toHaveTextContent("in this step: 10 min");
    expect(within(lit).queryByRole("meter")).toBeNull();
    expect(lit).toHaveTextContent(/no typical time yet/);
  });

  it("shows how long a merged task spent in each step", () => {
    const tasks = foldTaskEvents(done(1, 20));
    render(<TaskGraphs tasks={tasks} onOpenJourney={vi.fn()} now={T0 + 999 * 60_000} />);
    const row = screen.getByRole("group", { name: /issue #1\b/i });
    const nodes = within(row).getAllByRole("listitem");
    expect(nodes.map((node) => node.querySelector(".task-node-time")?.textContent ?? "")).toEqual(["10 min", "20 min", "2 min"]);
  });
});

describe("TaskGraphs round timers (#502 on #514)", () => {
  it("times a lit Fix from its BLOCK and a lit Re-review from the fix, not from when Review began", () => {
    const T0 = Date.parse("2026-10-08T10:00:00Z");
    const at = (m: number) => new Date(T0 + m * 60_000).toISOString();
    const A = "a".repeat(40), B = "b".repeat(40);
    const steps = [
      { ...record(1, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: A, journeys: [], lane: "gh-claude-2" }), occurredAt: at(0) },
      { ...record(2, "issue-9", "task.review_verdict", "gh-claude-5", { pr: 19, headSha: A, reviewer: "gh-claude-5", verdict: "BLOCK", commentUrl }), occurredAt: at(40) },
      { ...record(3, "issue-9", "task.pr_opened", "gh-claude-2", { pr: 19, headSha: B, journeys: [], lane: "gh-claude-2" }), occurredAt: at(55) },
    ];
    const { unmount } = render(<TaskGraphs tasks={foldTaskEvents(steps.slice(0, 2))} onOpenJourney={vi.fn()} now={T0 + 50 * 60_000} />);
    expect(screen.getByRole("listitem", { current: "step" })).toHaveTextContent(/^Fix · round 1.*in this step: 10 min/);
    unmount();
    render(<TaskGraphs tasks={foldTaskEvents(steps)} onOpenJourney={vi.fn()} now={T0 + 60 * 60_000} />);
    expect(screen.getByRole("listitem", { current: "step" })).toHaveTextContent(/^Re-review · round 1.*in this step: 5 min/);
  });
});

describe("TaskGraphs card home follows the parent chain (#524 review)", () => {
  it("puts a grandchild in the root issue's card, never in a card without its parent's row", () => {
    const claim = (seq: number, issue: number, parent?: number) =>
      record(seq, `issue-${issue}`, "task.claimed", "gh-claude-4", { issue, lane: "gh-claude-4", branch: `b${issue}`, ...(parent ? { parent } : {}) });
    render(<TaskGraphs tasks={foldTaskEvents([claim(1, 100), claim(2, 200, 100), claim(3, 300, 200)])} onOpenJourney={vi.fn()} />);
    const cards = screen.getAllByRole("article");
    expect(cards).toHaveLength(1);
    expect(within(cards[0]).getAllByRole("group")).toHaveLength(3);
  });

  it("survives a parent cycle", () => {
    const claim = (seq: number, issue: number, parent: number) =>
      record(seq, `issue-${issue}`, "task.claimed", "gh-claude-4", { issue, lane: "gh-claude-4", branch: `b${issue}`, parent });
    render(<TaskGraphs tasks={foldTaskEvents([claim(1, 1, 2), claim(2, 2, 1)])} onOpenJourney={vi.fn()} />);
    expect(screen.getAllByRole("group")).toHaveLength(2);
  });
});
