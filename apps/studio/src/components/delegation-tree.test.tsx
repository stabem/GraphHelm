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
