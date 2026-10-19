import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
const userEvent = fastUserEvent();

import { ProjectRail, RunHistory } from "./rail";
import type { ExecutionSummary } from "../runtime/types";

/**
 * #1654 (D-060, owner decision 2026-10-18): one project with several executions shows ONE card on
 * the rail. The card opens the project's live run (the team run, guided by issues); every other run
 * is reachable as history inside the project's screen.
 */
const run = (executionId: string, status: string, lastEventAt: string): ExecutionSummary => ({
  executionId, mode: "supervised", status, attention: "can_sleep", startedAt: lastEventAt, lastEventAt, headSequence: 3,
});
const runs = [
  run("gh-team", "running", "2026-10-18T10:00:00.000Z"),
  run("complete-fixture", "completed", "2026-10-18T11:00:00.000Z"),
  run("old-paused", "paused", "2026-10-17T09:00:00.000Z"),
  run("old-cancelled", "cancelled", "2026-10-16T09:00:00.000Z"),
];
const teamRuns = new Set(["gh-team"]);

describe("one card per project", () => {
  it("shows one card for a project with several executions, and it opens the team run", async () => {
    const onSelect = vi.fn();
    render(
      <ProjectRail
        projects={[{ name: "GraphHelm", runs }]}
        selected="" connected hasMore={false} busy={false} teamRuns={teamRuns}
        onSelect={onSelect} onLoadMore={vi.fn()} onNewTask={vi.fn()} onAddProject={vi.fn()} onOpenModels={vi.fn()}
      />,
    );
    const rail = screen.getByRole("navigation", { name: "Projects" });
    const cards = within(rail).getAllByRole("button", { name: /^Open project / });
    expect(cards).toHaveLength(1);
    // No run is its own card: none of the executions is a button on the rail.
    for (const { executionId } of runs) {
      expect(within(rail).queryByRole("button", { name: new RegExp(executionId) })).toBeNull();
    }
    await userEvent.click(cards[0]);
    expect(onSelect).toHaveBeenCalledWith("gh-team");
  });

  it("keeps every older run reachable as history inside the project's screen", async () => {
    const onSelect = vi.fn();
    render(
      <RunHistory
        project={{ name: "GraphHelm", runs }}
        selected="gh-team" hasMore={false} busy={false} teamRuns={teamRuns}
        onSelect={onSelect} onLoadMore={vi.fn()} onNewTask={vi.fn()}
      />,
    );
    const history = screen.getByRole("region", { name: "History of GraphHelm" });
    await userEvent.click(within(history).getByRole("button", { name: "History (4)" }));
    expect(within(history).getByRole("button", { name: /^old-paused;/ })).toBeInTheDocument();
    await userEvent.click(within(history).getByRole("button", { name: /^old-cancelled;/ }));
    expect(onSelect).toHaveBeenCalledWith("old-cancelled");
  });

  it("starts closed, opens on the button, and closes on a pick, Escape or a click outside (#1696)", async () => {
    render(
      <div>
        <p>elsewhere</p>
        <RunHistory
          project={{ name: "GraphHelm", runs }}
          selected="gh-team" hasMore={false} busy={false} teamRuns={teamRuns}
          onSelect={vi.fn()} onLoadMore={vi.fn()} onNewTask={vi.fn()}
        />
      </div>,
    );
    const history = screen.getByRole("region", { name: "History of GraphHelm" });
    const toggle = within(history).getByRole("button", { name: "History (4)" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(within(history).queryByRole("button", { name: /^old-cancelled;/ })).toBeNull();
    await userEvent.click(toggle);
    await userEvent.click(within(history).getByRole("button", { name: /^old-cancelled;/ }));
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(toggle);
    await userEvent.keyboard("{Escape}");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(toggle);
    await userEvent.click(screen.getByText("elsewhere"));
    expect(toggle).toHaveAttribute("aria-expanded", "false");
  });
});
