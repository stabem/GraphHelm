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
};

describe("HandoverCard", () => {
  it("summarises the gap in four groups and says Nothing for an empty one", () => {
    render(<HandoverCard handover={HANDOVER} onOpen={vi.fn()} onDismiss={vi.fn()} />);
    const card = screen.getByRole("dialog", { name: "While you were away" });
    expect(card).toHaveTextContent("30 records over 2 h 15 min");
    expect(within(screen.getByRole("region", { name: "Went quiet" })).getByText("Nothing.")).toBeInTheDocument();
  });

  it("opens the records a line cites and advances only on Got it", async () => {
    const onOpen = vi.fn();
    const onDismiss = vi.fn();
    render(<HandoverCard handover={HANDOVER} onOpen={onOpen} onDismiss={onDismiss} />);
    await userEvent.click(screen.getByRole("button", { name: "Cart page succeeded" }));
    expect(onOpen).toHaveBeenCalledWith([11]);
    expect(onDismiss).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(onDismiss).toHaveBeenCalled();
  });
});
