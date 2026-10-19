import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
const userEvent = fastUserEvent();

import { ProjectRail, homeRun } from "./rail";
import type { ExecutionSummary } from "../runtime/types";

/**
 * #1696 (D-060, owner decision 2026-10-19): the sidebar shows one card per project, labelled with
 * the project name; clicking it opens the project screen directly. There is no run history in the
 * UI, and a demonstration (fixture) run is never the project's view.
 */
const run = (executionId: string, status: string, lastEventAt: string, executor?: "fixture" | "gateway"): ExecutionSummary => ({
  executionId, mode: "supervised", status, attention: "can_sleep", startedAt: lastEventAt, lastEventAt, headSequence: 3,
  ...(executor ? { executor } : {}),
});
const runs = [
  run("gh-team", "running", "2026-10-18T10:00:00.000Z"),
  run("complete-fixture", "completed", "2026-10-18T11:00:00.000Z", "fixture"),
  run("old-paused", "paused", "2026-10-17T09:00:00.000Z"),
];
const teamRuns = new Set(["gh-team"]);

function mount(projectRuns: ExecutionSummary[], extra: Partial<Parameters<typeof ProjectRail>[0]> = {}) {
  const onSelect = vi.fn();
  const onNewTask = vi.fn();
  render(
    <ProjectRail
      projects={[{ name: "GraphHelm", runs: projectRuns }]}
      selected="" connected hasMore={false} busy={false} teamRuns={teamRuns}
      onSelect={onSelect} onLoadMore={vi.fn()} onNewTask={onNewTask} onAddProject={vi.fn()} onOpenModels={vi.fn()}
      {...extra}
    />,
  );
  return { onSelect, onNewTask, rail: screen.getByRole("navigation", { name: "Projects" }) };
}

describe("one card per project", () => {
  it("shows one card labelled with the project name, never a run title, and it opens the team run", async () => {
    const { onSelect, rail } = mount(runs, { selectedPresentation: { key: "calm", status: "agent work recorded · graph step waiting" }, selected: "gh-team" });
    const cards = within(rail).getAllByRole("button", { name: /^Open project / });
    expect(cards).toHaveLength(1);
    expect(within(cards[0]).getByText("GraphHelm")).toHaveClass("run-id");
    expect(cards[0]).not.toHaveTextContent(/agent work recorded|graph step|gh-team/);
    for (const { executionId } of runs) {
      expect(within(rail).queryByRole("button", { name: new RegExp(executionId) })).toBeNull();
    }
    await userEvent.click(cards[0]);
    expect(onSelect).toHaveBeenCalledWith("gh-team");
  });

  it("offers no run history: no strip, no history button, no run list", () => {
    mount(runs);
    expect(screen.queryByRole("region", { name: /History/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /history/i })).toBeNull();
    expect(screen.queryByText(/Removed from this browser/)).toBeNull();
  });

  it("never opens a demonstration run as the project's view", async () => {
    const fixtureOnly = [run("complete-fixture", "completed", "2026-10-18T11:00:00.000Z", "fixture")];
    expect(homeRun(fixtureOnly)).toBeNull();
    const { onSelect, onNewTask, rail } = mount(fixtureOnly, { teamRuns: new Set() });
    await userEvent.click(within(rail).getByRole("button", { name: /^Open project / }));
    expect(onSelect).not.toHaveBeenCalled();
    expect(onNewTask).toHaveBeenCalledTimes(1);
    // Newer fixture run beside an older real run: the real run wins.
    expect(homeRun([fixtureOnly[0], run("real", "completed", "2026-10-01T00:00:00.000Z")])?.executionId).toBe("real");
  });

  // #1171: a prop a suite tolerates is not a prop a suite constrains; press the button.
  it("opens the models screen from the rail, and not the add-project screen", async () => {
    const onOpenModels = vi.fn();
    const onAddProject = vi.fn();
    const { rail } = mount(runs, { onOpenModels, onAddProject });
    await userEvent.click(within(rail).getByRole("button", { name: "models" }));
    expect(onOpenModels).toHaveBeenCalledTimes(1);
    expect(onAddProject).not.toHaveBeenCalled();
  });
});
