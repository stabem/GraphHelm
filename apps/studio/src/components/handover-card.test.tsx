import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import type { Handover } from "../runtime/handover";
import { HandoverCard } from "./handover-card";

const userEvent = fastUserEvent();
afterEach(cleanup);

const HANDOVER: Handover = {
  fromSeq: 10, toSeq: 40, eventCount: 30, gapMinutes: 135,
  shipped: [{ text: "Cart page succeeded", sequences: [11] }],
  needsYou: [{ text: "kit 3 asked: Merge now?", sequences: [13] }],
  quiet: [], untouched: [{ text: "Payment got no record", sequences: [2] }],
  goal: { text: "Observe issue graph and work kanban", sequences: [12] },
};

describe("HandoverCard", () => {
  // #1041 B2: the `studio-handover` flow's details screen expects the task's goal (heading "Goal"),
  // one cited evidence record as a link ("Cited evidence") and the open duties (heading "Open duties").
  it("names the goal, links one cited evidence record and lists the open duties", async () => {
    const onOpen = vi.fn();
    render(<HandoverCard handover={HANDOVER} onOpen={onOpen} onDismiss={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: "Show details" }));
    expect(screen.getByRole("heading", { name: "Goal" })).toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "Goal" })).getByText("Observe issue graph and work kanban")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Open duties" })).toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "Open duties" })).getByRole("button", { name: "kit 3 asked: Merge now?" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("link", { name: "Cited evidence" }));
    expect(onOpen).toHaveBeenCalledWith([11]);
  });

  it("summarises the gap in four groups and says Nothing for an empty one", async () => {
    render(<HandoverCard handover={HANDOVER} onOpen={vi.fn()} onDismiss={vi.fn()} />);
    const card = screen.getByRole("region", { name: "While you were away" });
    expect(card).toHaveTextContent("30 records over 2 h 15 min");
    await userEvent.click(screen.getByRole("button", { name: "Show details" }));
    expect(within(screen.getByRole("region", { name: "Went quiet" })).getByText("Nothing.")).toBeInTheDocument();
  });

  it("opens the records a line cites and advances only on Got it", async () => {
    const onOpen = vi.fn();
    const onDismiss = vi.fn();
    render(<HandoverCard handover={HANDOVER} onOpen={onOpen} onDismiss={onDismiss} />);
    await userEvent.click(screen.getByRole("button", { name: "Show details" }));
    await userEvent.click(screen.getByRole("button", { name: "Cart page succeeded" }));
    expect(onOpen).toHaveBeenCalledWith([11]);
    expect(onDismiss).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(onDismiss).toHaveBeenCalled();
  });

  /* The owner, at 800-1440 px: the card opened over the Journey tab on every load and covered the
   * flow list and the Watch/Approve buttons. It is now a bar in the page flow, one line until
   * asked, never a dialog over the canvas; Hide puts it away for this browser session. */
  it("is a one-line bar, collapsed until asked, with Hide beside Got it", async () => {
    const onHide = vi.fn();
    render(<HandoverCard handover={HANDOVER} onOpen={vi.fn()} onDismiss={vi.fn()} onHide={onHide} />);
    expect(screen.queryByRole("dialog")).toBeNull();
    const bar = screen.getByRole("region", { name: "While you were away" });
    expect(bar).toHaveTextContent("Shipped 1 · Needs you 1");
    expect(screen.queryByRole("region", { name: "Shipped" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Show details" }));
    expect(screen.getByRole("region", { name: "Shipped" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Hide for now" }));
    expect(onHide).toHaveBeenCalled();
  });
});
