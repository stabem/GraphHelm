import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { ProofTable } from "./proof-table";
import type { Mission } from "../runtime/mission";

const userEvent = fastUserEvent();

const mission: Mission = {
  contractId: "j", title: "J",
  steps: [
    { stepId: "mark", index: 0, title: "Mark a skipped step safe", status: "needs_you", reason: "data-changing", promise: "The owner can mark a skipped step safe." },
    { stepId: "next", index: 1, title: "Next", status: "not_run", reason: null, promise: null },
  ],
  tasks: [{ key: "k", pr: 564, issue: null, title: "t", lane: "gh-claude-8", reviewers: ["gh-claude-2"], step: "review", blocked: false, trust: 1,
    blockedBy: null, rounds: [], headSha: null, mergeSha: null, repoUrl: null }],
  summary: { proven: 0, total: 2, inFlight: 1, needYou: 1, readyUnclaimed: 0 },
};

describe("ProofTable", () => {
  it("one row per step: promise, status badge, custody chips and the call", async () => {
    const onOpenTest = vi.fn();
    render(<ProofTable mission={mission} onOpenTest={onOpenTest} />);
    expect(screen.getByRole("heading", { name: "Can I trust “J”?" })).toBeInTheDocument();
    const rows = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("The owner can mark a skipped step safe.");
    expect(rows[0]).toHaveTextContent("Needs you");
    expect(rows[0]).toHaveTextContent("data-changing");
    expect(rows[0]).toHaveTextContent("journey replay · step 1 SKIPPED");
    expect(rows[0]).toHaveTextContent("impl gh-claude-8");
    expect(rows[0]).toHaveTextContent("rev gh-claude-2");
    expect(rows[1]).toHaveTextContent("No work linked to this step");
    await userEvent.click(screen.getByRole("button", { name: "Open test for step 1" }));
    expect(onOpenTest).toHaveBeenCalledWith("mark");
  });

  it("the replay thumbnail opens the test and shows the real frame when there is one", async () => {
    const onOpenTest = vi.fn();
    render(<ProofTable mission={mission} onOpenTest={onOpenTest} frameUrl={(id) => (id === "mark" ? "blob:m" : null)} />);
    expect(screen.getByRole("img", { name: "Replay frame of step 1" })).toHaveAttribute("src", "blob:m");
    expect(screen.getAllByRole("img")).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "Open the test canvas for step 2" }));
    expect(onOpenTest).toHaveBeenCalledWith("next");
  });

  it("without a replay handler the replay button is disabled and says where it starts", async () => {
    const { unmount } = render(<ProofTable mission={mission} onOpenTest={vi.fn()} />);
    expect(screen.getByRole("button", { name: /Replay whole journey — start it from the journey panel/ })).toBeDisabled();
    unmount();
    const onReplay = vi.fn();
    render(<ProofTable mission={mission} onOpenTest={vi.fn()} onReplay={onReplay} />);
    await userEvent.click(screen.getByRole("button", { name: "Replay whole journey" }));
    expect(onReplay).toHaveBeenCalled();
  });
});
