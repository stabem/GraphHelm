import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { MissionGraph } from "./mission-graph";
import type { Mission, MissionTask } from "../runtime/mission";
const userEvent = fastUserEvent();

const merged: MissionTask = {
  key: "k564", pr: 564, issue: 519, title: "Owner marks a skipped step safe", lane: "gh-claude-8", reviewers: ["gh-claude-2"], step: "merged",
  blocked: false, trust: 3, blockedBy: null, rounds: [], headSha: "4f1ead4c00", mergeSha: "9a9a9a9a11", repoUrl: "https://github.com/o/r",
};
const blocked: MissionTask = {
  ...merged, key: "k560", pr: 560, title: "Opening a journey runs it", lane: "gh-claude-6", reviewers: ["gh-claude-7"], step: "review", trust: 1,
  blocked: true, blockedBy: { reviewer: "gh-claude-7", headSha: "bf76a762ff" }, mergeSha: null, repoUrl: null,
  rounds: [{ reviewer: "gh-claude-7", headSha: "bf76a762ff", fixHead: null }],
};

const mission: Mission = {
  contractId: "watch", title: "Watch plays inside the Studio",
  steps: [
    { stepId: "open", index: 0, title: "Open a journey", status: "not_run", reason: null, promise: null },
    { stepId: "mark", index: 1, title: "Mark a skipped step safe", status: "needs_you", reason: "data-changing", promise: "The owner can mark it safe" },
  ],
  tasks: [merged],
  summary: { proven: 0, total: 2, inFlight: 0, needYou: 1, readyUnclaimed: 0 },
};

function setup(over: Partial<Parameters<typeof MissionGraph>[0]> = {}) {
  const props = { mission, selectedStepId: null, selectedTaskKey: null, onSelectStep: vi.fn(), onSelectTask: vi.fn(), onOpenTest: vi.fn(), ...over };
  const view = render(<MissionGraph {...props} />);
  return { ...props, ...view };
}

describe("MissionGraph", () => {
  it("shows the quiet summary line", () => {
    const { container } = setup();
    expect(container.querySelector(".mg-summary")).toHaveTextContent("0/2 proven · 0 in flight · 0 ready, unclaimed · 1 need you");
  });

  it("a column head click selects its step; the selected column is marked", async () => {
    const p = setup({ selectedStepId: "mark" });
    await userEvent.click(screen.getByRole("button", { name: "Column 1: Open a journey" }));
    expect(p.onSelectStep).toHaveBeenCalledWith("open");
    expect(screen.getByRole("button", { name: "Column 2: Mark a skipped step safe" })).toHaveAttribute("data-selected", "true");
  });

  it("task node click selects the task; merged work sits past the first unproven step", async () => {
    const p = setup();
    const node = screen.getByRole("button", { name: /#564/ });
    expect(node.style.left).toBe("168px");
    await userEvent.click(node);
    expect(p.onSelectTask).toHaveBeenCalledWith("k564");
  });

  it("inspector shows the ladder lit to the task's trust, the note, custody and a PR link", () => {
    setup({ selectedTaskKey: "k564" });
    const ins = screen.getByRole("complementary", { name: "Selected work" });
    const ladder = within(ins).getByRole("list", { name: "How far it got" });
    expect(ladder.querySelectorAll('[data-lit="true"]').length).toBe(3);
    expect(ladder).toHaveTextContent("WrittenReviewedMergedProvenSeen by you");
    expect(ins).toHaveTextContent("Merged, not proven yet");
    expect(ins).toHaveTextContent("Proves step 2");
    const rows = within(within(ins).getByRole("list", { name: "Who touched it" })).getAllByRole("listitem").map((li) => li.textContent);
    expect(rows).toEqual(["Implementgh-claude-8done", "Reviewgh-claude-2APPROVE", "Merge9a9a9a9amerged"]);
    expect(within(ins).getByRole("link", { name: "Open PR" })).toHaveAttribute("href", "https://github.com/o/r/pull/564");
    expect(ins).toHaveTextContent("Journey replay · step 2Needs you · data-changing");
  });

  it("a blocked task names the BLOCK, its reviewer and head", () => {
    setup({ mission: { ...mission, tasks: [blocked] }, selectedTaskKey: "k560" });
    const ins = screen.getByRole("complementary", { name: "Selected work" });
    expect(ins).toHaveTextContent("BLOCK by gh-claude-7 at bf76a762");
    expect(within(ins).queryByRole("link", { name: "Open PR" })).toBeNull();
    expect(ins).toHaveTextContent("No evidence recorded on this head");
  });

  it("open its test uses the task's step", async () => {
    const p = setup({ selectedTaskKey: "k564" });
    await userEvent.click(screen.getByRole("button", { name: "Open its test" }));
    expect(p.onOpenTest).toHaveBeenCalledWith("mark");
  });

  it("with no task selected, the inspector shows the selected step", async () => {
    const p = setup({ selectedStepId: "mark" });
    const ins = screen.getByRole("complementary", { name: "Selected step" });
    expect(ins).toHaveTextContent("STEP 2 · Needs you");
    expect(ins).toHaveTextContent("data-changing");
    await userEvent.click(within(ins).getByRole("button", { name: "Open test" }));
    expect(p.onOpenTest).toHaveBeenCalledWith("mark");
  });

  it("draws edges between consecutive tasks with an arrow", () => {
    const { container } = setup({ mission: { ...mission, tasks: [merged, { ...blocked, pr: 600, key: "k600" }] } });
    expect(container.querySelectorAll(".mg-seg").length).toBeGreaterThan(1);
    expect(container.querySelector('.mg-seg[data-done="true"]')).not.toBeNull();
  });

  it("a journey with no tasks still draws its steps", () => {
    setup({ mission: { ...mission, tasks: [] } });
    expect(screen.getAllByRole("button", { name: /^Column / })).toHaveLength(2);
    expect(screen.getByText("No work linked to this journey yet")).toBeInTheDocument();
    expect(screen.getByText("Merged is not done. Done = the replay proves the step.")).toBeInTheDocument();
  });
});
