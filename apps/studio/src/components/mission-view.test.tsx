import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { MissionView } from "./mission-view";
import type { JourneyView } from "../runtime/types";
import type { TaskState } from "../runtime/team-tasks";

const journeys: JourneyView[] = [{
  contractId: "watch", title: "Watch plays inside the Studio", arrows: [],
  steps: [{ stepId: "mark", screen: { screenId: "mark", title: "Mark a skipped step safe", scopePaths: [] }, promises: [] }],
}];

describe("MissionView", () => {
  it("goes graph → proof → test canvas on the chosen step", async () => {
    const userEvent = fastUserEvent();
    const onMarkSafe = vi.fn();
    render(<MissionView journeys={journeys} tasks={[]} lanes={[]} now={0}
      runFor={() => ({ state: "ready", kind: "replay", screens: {}, edges: { mark: { result: "skipped", reason: "data-changing" } } })}
      frameUrl={() => null} onMarkSafe={onMarkSafe} onSendBack={vi.fn()} />);
    expect(screen.getByText("0/1 proven · 0 in flight · 1 need you")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "Proof" }));
    await userEvent.click(screen.getByRole("button", { name: "Open test for step 1" }));
    expect(screen.getByRole("region", { name: "Emulated browser" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "I watched it — mark safe" }));
    expect(onMarkSafe).toHaveBeenCalledWith("mark", "watch");
  });

  it("no journeys: says so", () => {
    render(<MissionView journeys={[]} tasks={[]} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} onSendBack={vi.fn()} />);
    expect(screen.getByText("No journeys in this project yet")).toBeInTheDocument();
  });
  it("is wide, shows journeys as a rail, and folds unlinked work", async () => {
    const userEvent = fastUserEvent();
    const two: JourneyView[] = [...journeys, { ...journeys[0], contractId: "other", title: "Another journey" }];
    const tasks = [{ key: "t1", taskId: "t1", pr: 40, title: "Loose PR", prTitle: "", journeys: [], step: "claimed", blockedBy: null, reviewers: [] }] as unknown as TaskState[];
    const { container } = render(<MissionView journeys={two} tasks={tasks} lanes={[]} now={0} runFor={() => null}
      frameUrl={() => null} onMarkSafe={vi.fn()} />);
    expect(container.querySelector(".mv")).toHaveAttribute("data-wide", "true");
    const rail = screen.getByRole("navigation", { name: "Journeys" });
    const rows = within(rail).getAllByRole("button");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(rows[1]);
    expect(rows[1]).toHaveAttribute("aria-pressed", "true");
    expect(rows[0]).toHaveAttribute("aria-pressed", "false");
    const fold = screen.getByRole("button", { name: "Unlinked work · 1" });
    expect(fold).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("#40 Loose PR")).toBeNull();
    await userEvent.click(fold);
    expect(fold).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("#40 Loose PR")).toBeInTheDocument();
  });
});
