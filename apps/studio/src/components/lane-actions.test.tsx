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
  const M = 60_000;
  const base = { lane: "gh-claude-7", step: "Review", pr: 609, roster: laneRoster(["gh-claude-3"], lanes), lastSeenAt: 1_000, now: 10 * M };
  it("Ask posts an operator_note, then reads Asked Xm ago · no answer yet, then Answered once the lane records anything newer", async () => {
    const send = vi.fn().mockResolvedValue(undefined);
    const onAsked = vi.fn();
    const { rerender } = render(<LaneActions {...base} askedAt={null} onAsked={onAsked} send={send} />);
    await userEvent.click(screen.getByRole("button", { name: "Ask gh-claude-7 for status" }));
    expect(send).toHaveBeenCalledWith({ type: "operator_note", to: "gh-claude-7", description: "Owner asks: status of Review on PR #609?" });
    expect(onAsked).toHaveBeenCalledWith(10 * M);
    rerender(<LaneActions {...base} askedAt={10 * M} onAsked={onAsked} send={send} now={12 * M} />);
    expect(screen.getByText("Asked 2 min ago · no answer yet")).toBeInTheDocument();
    rerender(<LaneActions {...base} askedAt={10 * M} onAsked={onAsked} send={send} now={13 * M} lastSeenAt={12 * M} />);
    expect(screen.getByText("Answered 1 min ago")).toBeInTheDocument();
  });
  it("a failed ask shows the error as an alert", async () => {
    const send = vi.fn().mockRejectedValue(new Error("Runtime has no keyring"));
    render(<LaneActions {...base} askedAt={null} onAsked={vi.fn()} send={send} />);
    await userEvent.click(screen.getByRole("button", { name: "Ask gh-claude-7 for status" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Runtime has no keyring");
  });
  it("Hand to… is a Studio button opening an accessible menu; it confirms inline and sends the same two notes", async () => {
    const send = vi.fn().mockResolvedValue(undefined);
    render(<LaneActions {...base} askedAt={null} onAsked={vi.fn()} send={send} />);
    expect(screen.queryByRole("combobox")).toBeNull();
    const hand = screen.getByRole("button", { name: "Hand to…" });
    expect(hand).toHaveClass("mg-secondary");
    expect(hand).toHaveAttribute("aria-haspopup", "menu");
    await userEvent.click(hand);
    const menu = screen.getByRole("menu");
    const items = screen.getAllByRole("menuitem");
    expect(items.map((i) => i.textContent)).toEqual(["gh-claude-3", "gh-claude-9 · silent"]);
    expect(items[0]).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(items[1]).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(items[0]).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(menu).not.toBeInTheDocument();
    expect(hand).toHaveFocus();
    await userEvent.click(hand);
    await userEvent.click(screen.getByRole("menuitem", { name: "gh-claude-3" }));
    expect(screen.getByText("Hand review of PR #609 to gh-claude-3?")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(send.mock.calls).toEqual([
      [{ type: "operator_note", to: "gh-claude-3", description: "Take over Review on PR #609 from gh-claude-7" }],
      [{ type: "operator_note", to: "gh-claude-7", description: "Hand Review on PR #609 to gh-claude-3" }],
    ]);
    expect(await screen.findByText("Hand-off to gh-claude-3 requested")).toBeInTheDocument();
  });
  it("Cancel drops the hand-off without sending", async () => {
    const send = vi.fn();
    render(<LaneActions {...base} askedAt={null} onAsked={vi.fn()} send={send} />);
    await userEvent.click(screen.getByRole("button", { name: "Hand to…" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "gh-claude-3" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByText(/Hand review of/)).toBeNull();
    expect(send).not.toHaveBeenCalled();
  });
});
