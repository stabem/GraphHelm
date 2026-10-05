import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";

import { ProjectRail, dayOf } from "./rail";
import type { ExecutionSummary } from "../runtime/types";

/**
 * The rail groups by the day work happened and sorts by the last event, newest first, so the run
 * the owner touched last is the first row (2026-10-05: the list read in no order at all).
 */
const at = (daysAgo: number, hour: number): string => {
  const date = new Date();
  date.setDate(date.getDate() - daysAgo);
  date.setHours(hour, 0, 0, 0);
  return date.toISOString();
};
const run = (executionId: string, lastEventAt: string, startedAt: string, status = "paused"): ExecutionSummary => ({
  executionId, mode: "supervised", status, attention: "can_sleep", startedAt, lastEventAt, headSequence: 3,
});

const rail = (runs: ExecutionSummary[]) => render(
  <ProjectRail
    projects={[{ name: "store", runs }]}
    selected="" connected hasMore={false} busy={false}
    onSelect={vi.fn()} onLoadMore={vi.fn()} onNewTask={vi.fn()} onAddProject={vi.fn()} onOpenModels={vi.fn()}
  />,
);

const order = () => screen.getAllByRole("button", { name: /; Last event/ }).map((button) => button.textContent ?? "");

afterEach(() => window.localStorage.clear());

describe("rail grouping", () => {
  const runs = [
    run("old-work", at(3, 9), at(3, 8), "completed"),
    run("today-early", at(0, 1), at(5, 8)),
    run("yesterday", at(1, 10), at(1, 9)),
    run("today-late", at(0, 2), at(0, 0)),
  ];

  it("groups by day and puts the last touched run first by default", () => {
    rail(runs);
    expect(screen.getByRole("group", { name: "Today (2)" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Yesterday (1)" })).toBeInTheDocument();
    expect(order().map((text) => ["today-late", "today-early", "yesterday", "old-work"].find((id) => text.includes(id)))).toEqual(["today-late", "today-early", "yesterday", "old-work"]);
  });

  it("switches to sort by start and to status groups from the menu, and remembers it", () => {
    const { unmount } = rail(runs);
    fireEvent.click(screen.getByRole("button", { name: "Group and sort tasks" }));
    fireEvent.click(screen.getByRole("button", { name: "Started" }));
    const today = screen.getByRole("group", { name: "Today (1)" });
    expect(within(today).getByText("today-late")).toBeInTheDocument();
    expect(order()[order().length - 1]).toContain("today-early");
    fireEvent.click(screen.getByRole("button", { name: "Status" }));
    expect(screen.getByRole("group", { name: "Ongoing runs (3)" })).toBeInTheDocument();
    unmount();
    rail(runs);
    expect(screen.getByRole("group", { name: "Ongoing runs (3)" })).toBeInTheDocument();
  });

  it("labels days by name", () => {
    const now = new Date(2026, 9, 5, 12);
    expect(dayOf(new Date(2026, 9, 5, 1).getTime(), now).label).toBe("Today");
    expect(dayOf(new Date(2026, 9, 4, 23).getTime(), now).label).toBe("Yesterday");
    expect(dayOf(new Date(2026, 9, 2, 9).getTime(), now).key).toBe("2026-10-02");
  });
});
