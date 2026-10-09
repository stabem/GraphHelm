import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { MissionGraph } from "./mission-graph";
import type { Mission } from "../runtime/mission";
const userEvent = fastUserEvent();

const mission: Mission = {
  contractId: "watch", title: "Watch plays inside the Studio",
  steps: [
    { stepId: "open", index: 0, title: "Open a journey", status: "not_run", reason: null },
    { stepId: "mark", index: 1, title: "Mark a skipped step safe", status: "needs_you", reason: "data-changing" },
  ],
  tasks: [{ key: "k564", pr: 564, issue: 519, title: "Owner marks a skipped step safe", lane: "gh-claude-8", reviewers: ["gh-claude-2"], step: "merged", blocked: false, trust: 3 }],
  summary: { proven: 0, total: 2, inFlight: 0, needYou: 1 },
};

function setup(over: Partial<Parameters<typeof MissionGraph>[0]> = {}) {
  const props = { mission, selectedStepId: null, selectedTaskKey: null, onSelectStep: vi.fn(), onSelectTask: vi.fn(), onOpenTest: vi.fn(), ...over };
  render(<MissionGraph {...props} />);
  return props;
}

describe("MissionGraph", () => {
  it("shows the quiet summary line", () => {
    setup();
    expect(screen.getByText("0/2 proven · 0 in flight · 1 need you")).toBeInTheDocument();
  });

  it("step rail names each step with its status, and a click selects it", async () => {
    const p = setup();
    await userEvent.click(screen.getByRole("button", { name: "Step 2: Mark a skipped step safe, Needs you" }));
    expect(p.onSelectStep).toHaveBeenCalledWith("mark");
  });

  it("task node click selects the task", async () => {
    const p = setup();
    await userEvent.click(screen.getByRole("button", { name: /#564/ }));
    expect(p.onSelectTask).toHaveBeenCalledWith("k564");
  });

  it("inspector shows the ladder lit to the task's trust", () => {
    setup({ selectedTaskKey: "k564" });
    const ladder = screen.getByRole("list", { name: "How far it got" });
    expect(ladder.querySelectorAll('[data-lit="true"]').length).toBe(3);
    expect(ladder).toHaveTextContent("WrittenReviewedMergedProvenSeen by you");
  });

  it("open its test uses the selected step", async () => {
    const p = setup({ selectedTaskKey: "k564", selectedStepId: "mark" });
    await userEvent.click(screen.getByRole("button", { name: "Open its test" }));
    expect(p.onOpenTest).toHaveBeenCalledWith("mark");
  });

  it("a journey with no tasks still draws its steps", () => {
    setup({ mission: { ...mission, tasks: [] } });
    expect(screen.getAllByRole("button", { name: /^Step / })).toHaveLength(2);
    expect(screen.getByText("No work linked to this journey yet")).toBeInTheDocument();
  });
});
