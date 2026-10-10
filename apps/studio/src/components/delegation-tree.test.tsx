import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { DelegationTree } from "./delegation-tree";
import { foldTaskEvents, type TaskEventRecord } from "../runtime/team-tasks";

// User contract: keyboard navigation and literal recorded text. Existing task-graph tests
// cannot observe this new tree. Cost: jsdom only, milliseconds; no production test seam.
const records: TaskEventRecord[] = [
  { kind: "task.claimed", actorId: "lane", sequence: 1, taskId: "issue-86", issue: 86,
    lane: "lane", branch: "issue-86-tree", assignedBy: "coord", title: "<img src=x onerror=alert(1)>" },
  { kind: "task.review_assigned", actorId: "lane", sequence: 2, taskId: "issue-86", pr: 99,
    headSha: "aaaaaaaa", reviewer: "rev" },
  { kind: "task.claimed", actorId: "old-lane", sequence: 3, taskId: "issue-87", issue: 87,
    lane: "old-lane", branch: "issue-87-old" },
];
describe("DelegationTree", () => {
  it("renders reported and unknown assignments as text and supports arrow-key focus", () => {
    const { container } = render(<DelegationTree tasks={foldTaskEvents(records)} />);
    const tree = screen.getByRole("tree", { name: "Recorded handoffs" });
    expect(screen.getAllByText("assigner unrecorded").length).toBeGreaterThan(0);
    expect(screen.getAllByText(/reported by lane/).length).toBeGreaterThan(0);
    expect(screen.getByText("review_assigned")).toBeInTheDocument();
    expect(screen.getByText(/<img src=x onerror=alert\(1\)>/)).toBeInTheDocument();
    expect(container.querySelector("img")).toBeNull();
    const items = within(tree).getAllByRole("treeitem");
    items[0].focus();
    fireEvent.keyDown(items[0], { key: "ArrowRight" });
    expect(items[1]).toHaveFocus();
    fireEvent.keyDown(items[1], { key: "ArrowDown" });
    expect(items[2]).toHaveFocus();
    fireEvent.keyDown(items[2], { key: "ArrowLeft" });
    expect(items[1]).toHaveFocus();
    fireEvent.keyDown(items[1], { key: "End" });
    expect(items.at(-1)).toHaveFocus();
    fireEvent.keyDown(items.at(-1)!, { key: "Home" });
    expect(items[0]).toHaveFocus();
    expect(items.filter((item) => item.tabIndex === 0)).toHaveLength(1);
  });
  it("keeps one tab stop when a live claim inserts a new assigner ahead of the focused unknown group", () => {
    const { rerender } = render(<DelegationTree tasks={foldTaskEvents([records[2]])} />);
    const focused = screen.getAllByRole("treeitem")[2];
    focused.focus();
    rerender(<DelegationTree tasks={foldTaskEvents(records)} />);
    expect(focused).toHaveFocus();
    expect(screen.getAllByRole("treeitem").filter((item) => item.tabIndex === 0)).toEqual([focused]);
  });
});

// Contract: task disclosure and safe evidence URLs, absent from the assignment-only tree tests.
// Regression: keyboard cannot open history, or an untrusted URL becomes a navigable link.
// Cost: jsdom only, milliseconds; the real component consumes the fold, with no test seam.
it("opens recorded BLOCK evidence with Enter and keeps unsafe URLs as text", () => {
  const tasks = foldTaskEvents([
    { ...records[0], taskId: "issue-901", issue: 901 },
    { kind: "task.pr_opened", actorId: "lane", sequence: 4, taskId: "issue-901", pr: 903, lane: "lane", headSha: "aaaaaaaa" },
    { kind: "task.review_verdict", actorId: "rev", sequence: 5, taskId: "issue-901", pr: 903, reviewer: "rev", headSha: "aaaaaaaa", verdict: "BLOCK", commentUrl: "https://github.com/stabem/GraphHelm/pull/903#issuecomment-1" },
  ]);
  const { rerender } = render(<DelegationTree tasks={tasks} />);
  const item = screen.getByRole("treeitem", { name: /#901/ });
  fireEvent.keyDown(item, { key: "Enter" });
  expect(item).toHaveAttribute("aria-expanded", "true");
  const details = screen.getByRole("region", { name: "#901 details" });
  expect(within(details).getByRole("link")).toHaveAttribute("href", "https://github.com/stabem/GraphHelm/pull/903#issuecomment-1");
  expect(within(details).getByRole("link")).toHaveAttribute("rel", "noreferrer");
  for (const url of ["http://github.com/stabem/GraphHelm/pull/903", "https://github.com.evil.test/pull/903", "javascript:alert(1)"]) {
    tasks[0].rounds[0].commentUrl = url;
    rerender(<DelegationTree tasks={[...tasks]} />);
    expect(within(details).queryByRole("link")).toBeNull();
    expect(within(details).getByText(url)).toBeInTheDocument();
  }
  fireEvent.keyDown(item, { key: " " });
  expect(screen.queryByRole("region", { name: "#901 details" })).toBeNull();
  fireEvent.click(within(item).getByText(/#901/));
  expect(screen.getByRole("region", { name: "#901 details" })).toBeInTheDocument();
});
