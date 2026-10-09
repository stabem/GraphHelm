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
    expect(document.querySelector(".mg-summary")).toHaveTextContent("0/1 proven · 0 in flight · 0 ready, unclaimed · 1 need you");
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
    const rows = Array.from(rail.querySelectorAll<HTMLButtonElement>("button.mv-journey"));
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

  it("clicking a step chip selects its column, and a task node opens its custody", async () => {
    const userEvent = fastUserEvent();
    const two: JourneyView[] = [{ ...journeys[0]!, steps: [...journeys[0]!.steps, { stepId: "next", screen: { screenId: "next", title: "Next step", scopePaths: [] }, promises: [] }] }];
    const tasks = [{ key: "t9", taskId: "t9", pr: 9, issue: 519, lane: "gh-claude-8", title: "Work", prTitle: "", journeys: ["watch"], step: "review",
      blockedBy: { reviewer: "gh-claude-2", headSha: "abcdef0123", commentUrl: "" }, reviewers: ["gh-claude-2"], mergeSha: null, headSha: "abcdef0123", repoUrl: null,
      rounds: [{ reviewer: "gh-claude-2", headSha: "abcdef0123", commentUrl: "", fixHead: null, blockedAt: null, fixedAt: null }] }] as unknown as TaskState[];
    render(<MissionView journeys={two} tasks={tasks} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    expect(screen.getByText("issue #519 · 1 task")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Step 2: Next step, Not run" }));
    expect(screen.getByRole("button", { name: "Column 2: Next step" })).toHaveAttribute("data-selected", "true");
    expect(screen.getByRole("button", { name: "Column 1: Mark a skipped step safe" })).toHaveAttribute("data-selected", "false");
    expect(screen.getByRole("complementary", { name: "Selected step" })).toHaveTextContent("STEP 2 · Not run");
    await userEvent.click(within(screen.getByRole("region", { name: "Work graph" })).getByRole("button", { name: /#9/ }));
    const ins = screen.getByRole("complementary", { name: "Selected work" });
    expect(ins).toHaveTextContent("BLOCK by gh-claude-2 at abcdef01");
    expect(screen.getByRole("button", { name: "Column 1: Mark a skipped step safe" })).toHaveAttribute("data-selected", "true");
  });

  it("lanes tab shows the lane cards", async () => {
    const userEvent = fastUserEvent();
    const lanes = [{ lane: "gh-claude-3", silent: false, lastEventAt: 0, bars: [] }];
    render(<MissionView journeys={journeys} tasks={[]} lanes={lanes} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    await userEvent.click(screen.getByRole("tab", { name: "Lanes" }));
    expect(screen.getByText("FREE HANDS").parentElement).toHaveTextContent("gh-claude-3");
    expect(screen.queryByRole("navigation", { name: "Journeys" })).toBeNull();
  });
});
