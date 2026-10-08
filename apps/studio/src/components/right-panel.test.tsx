import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import { RightPanel } from "./right-panel";
import type { BeforeAfterPair, CaptureDocument } from "../runtime/journeys";
import type { JourneyView } from "../runtime/types";

const userEvent = fastUserEvent();
afterEach(cleanup);

describe("RightPanel", () => {
  it("shows honest empty states for journeys and before/after until phase 4 data exists", () => {
    render(<RightPanel activity={[]} onOpenActivity={vi.fn()} />);
    expect(screen.getByRole("region", { name: "Journeys" })).toHaveTextContent("No journeys mapped yet");
    expect(screen.getByRole("region", { name: "Before and after" })).toHaveTextContent("No before/after screenshots yet");
    expect(screen.getByRole("region", { name: "What just happened" })).toHaveTextContent("Nothing recorded yet");
  });

  // #446: while the journeys read or the capture envelopes are in flight, the panel says so; the
  // #301 observer saw "No journeys mapped yet" for seconds on a run that had journeys.
  it("says it is loading instead of claiming there is nothing", () => {
    render(<RightPanel activity={[]} onOpenActivity={vi.fn()} journeysLoading pairsLoading />);
    expect(screen.getByRole("region", { name: "Journeys" })).toHaveTextContent("Loading journeys…");
    expect(screen.getByRole("region", { name: "Journeys" })).not.toHaveTextContent("No journeys mapped yet");
    expect(screen.getByRole("region", { name: "Before and after" })).toHaveTextContent("Opening screenshots…");
  });

  // #447: a flow with a second path compiles to `<flow>.<path>`; the panel showed both as two rows
  // with the same title. The branch is a sub-row named by its path, and each still opens its contract.
  it("shows a flow's branch path as a named sub-row, never as a second identical row", async () => {
    const j = (contractId: string, title: string): JourneyView => ({ contractId, title, arrows: [], steps: [{ stepId: "a", capture: null, promises: [] }] });
    const onOpenJourney = vi.fn();
    render(<RightPanel activity={[]} onOpenActivity={vi.fn()} onOpenJourney={onOpenJourney}
      journeys={[j("run-actions", "Owner pauses a run"), j("run-actions.cancel", "Owner pauses a run"), j("other.thing", "Dotted but alone")]} />);
    const rows = screen.getAllByRole("button").map((button) => button.textContent ?? "");
    expect(rows.filter((text) => text.startsWith("Owner pauses a run"))).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: /cancel path/ }));
    expect(onOpenJourney).toHaveBeenCalledWith("run-actions.cancel");
    expect(screen.getByRole("button", { name: /Dotted but alone/ })).toBeInTheDocument();
  });

  it("lists what just happened as bot verb object, each opening its record", async () => {
    const onOpen = vi.fn();
    render(<RightPanel activity={[{ sequence: 7, text: "loja kit 1 asked you “Merge now?”", at: null }]} onOpenActivity={onOpen} />);
    await userEvent.click(screen.getByRole("button", { name: /loja kit 1 asked you/ }));
    expect(onOpen).toHaveBeenCalledWith(7);
  });
  it("lists journeys with a proven bar and before/after pairs, each opening its target", async () => {
    const cap = (freshness: "fresh" | "stale" | "unknown") => ({ signalId: "s", sequence: 1, imageEvidenceId: "i", revision: "r", dirty: false,
      viewport: { width: 1, height: 1 }, observer: "kit-1", freshness, changedFiles: [] });
    const journey: JourneyView = { contractId: "checkout", title: "Checkout", arrows: [], steps: [
      { stepId: "cart", screen: { screenId: "cart", title: "Cart", scopePaths: [] }, capture: cap("fresh"), promises: [] },
      { stepId: "pay", capture: cap("stale"), promises: [] },
      { stepId: "done", capture: null, promises: [] },
    ] };
    const doc = (sequence: number, phase: "before" | "after"): CaptureDocument => ({ sequence, signalId: null, imageEvidenceId: "x", contractId: "checkout",
      stepId: "cart", revision: "r", dirty: false, observer: "kit-1", actorId: "kit-1", pr: 9, phase, occurredAt: null });
    const pair: BeforeAfterPair = { contractId: "checkout", stepId: "cart", pr: 9, before: doc(1, "before"), after: doc(2, "after"), observer: "kit-1", actorId: "kit-1" };
    const onOpenJourney = vi.fn();
    const onOpenPair = vi.fn();
    render(<RightPanel activity={[]} onOpenActivity={vi.fn()} journeys={[journey]} beforeAfter={[pair]}
      onOpenJourney={onOpenJourney} onOpenPair={onOpenPair} botName={(id) => `Bot ${id}`} />);
    expect(screen.getByRole("img", { name: "1 of 3 steps proven" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Checkout/ }));
    expect(onOpenJourney).toHaveBeenCalledWith("checkout");
    await userEvent.click(screen.getByRole("button", { name: "PR #9 · Cart · Bot kit-1" }));
    expect(onOpenPair).toHaveBeenCalledWith(pair);
  });
});
