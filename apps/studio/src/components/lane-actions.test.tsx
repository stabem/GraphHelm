import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { LaneActions, laneRoster } from "./lane-actions";

const userEvent = fastUserEvent();
const lanes = [{ lane: "gh-claude-7", bars: [], silent: false, lastEventAt: 0 }, { lane: "gh-claude-9", bars: [], silent: true, lastEventAt: 0 }, { lane: "TBD", bars: [], silent: false, lastEventAt: 0 }];

describe("laneRoster", () => {
  it("joins bots and lanes, marks silent lanes and drops TBD", () => {
    expect(laneRoster(["gh-claude-3", "tbd", "gh-claude-7"], lanes)).toEqual([
      { name: "gh-claude-3", silent: false }, { name: "gh-claude-7", silent: false }, { name: "gh-claude-9", silent: true },
    ]);
  });
});

describe("LaneActions", () => {
  const base = { lane: "gh-claude-7", step: "Review", pr: 609, roster: laneRoster(["gh-claude-3"], lanes), lastSeenAt: 1_000, now: 10 * 60_000 };
  it("Nudge posts an operator_note to the owner lane and then reads nudged Xm ago", async () => {
    const send = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(<LaneActions {...base} send={send} />);
    await userEvent.click(screen.getByRole("button", { name: "Nudge gh-claude-7" }));
    expect(send).toHaveBeenCalledWith({ type: "operator_note", to: "gh-claude-7", description: "Owner asks: status of Review on PR #609?" });
    rerender(<LaneActions {...base} send={send} now={base.now + 3 * 60_000} />);
    expect(screen.getByText(/nudged 3m ago/)).toBeInTheDocument();
    // The lane recorded something newer than the nudge: the button comes back.
    rerender(<LaneActions {...base} send={send} now={base.now + 4 * 60_000} lastSeenAt={Date.now() + 60_000} />);
    expect(screen.getByRole("button", { name: "Nudge gh-claude-7" })).toBeInTheDocument();
  });
  it("a failed nudge shows the error as an alert", async () => {
    const send = vi.fn().mockRejectedValue(new Error("Runtime has no keyring"));
    render(<LaneActions {...base} send={send} />);
    await userEvent.click(screen.getByRole("button", { name: "Nudge gh-claude-7" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Runtime has no keyring");
  });
  it("Reassign sends a take-over note to the new lane and a hand-off note to the old one", async () => {
    const send = vi.fn().mockResolvedValue(undefined);
    render(<LaneActions {...base} send={send} />);
    const select = screen.getByRole("combobox", { name: "Reassign to…" });
    expect(Array.from((select as HTMLSelectElement).options).map((o) => o.textContent)).toEqual(["Reassign to…", "gh-claude-3", "gh-claude-9 (silent)"]);
    await userEvent.selectOptions(select, "gh-claude-3");
    await userEvent.click(screen.getByRole("button", { name: "Confirm reassign" }));
    expect(send.mock.calls).toEqual([
      [{ type: "operator_note", to: "gh-claude-3", description: "Take over Review on PR #609 from gh-claude-7" }],
      [{ type: "operator_note", to: "gh-claude-7", description: "Hand Review on PR #609 to gh-claude-3" }],
    ]);
    expect(await screen.findByText("reassign requested to gh-claude-3")).toBeInTheDocument();
  });
});
