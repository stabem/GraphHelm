import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import type { BeforeAfterPair, CaptureDocument } from "../runtime/journeys";
import type { CaptureView, JourneysView, RuntimeEvent } from "../runtime/types";
import { JourneyCanvas, type JourneyCanvasProps } from "./journey-canvas";
import studioStyles from "../styles.css?raw";

const userEvent = fastUserEvent();
let counter = 0;
const create = vi.fn(() => `blob:img-${++counter}`);
const revoke = vi.fn();
beforeEach(() => { counter = 0; create.mockClear(); revoke.mockClear(); Object.assign(URL, { createObjectURL: create, revokeObjectURL: revoke }); });
afterEach(cleanup);

const capture = (sequence: number, extra: Partial<CaptureView>): CaptureView => ({ signalId: `s${sequence}`, sequence, imageEvidenceId: `img-${sequence}`,
  revision: "abcdef1234567", dirty: false, viewport: { width: 800, height: 600 }, observer: "kit-1", freshness: "fresh", changedFiles: [], ...extra });

const view: JourneysView = { head: "abc", journeys: [{ contractId: "checkout", title: "Checkout", steps: [
  { stepId: "cart", screen: { screenId: "cart", title: "Cart", scopePaths: ["src/cart"] }, capture: capture(10, {}), promises: ["The cart shows the total"] },
  { stepId: "pay", screen: { screenId: "pay", title: "Pay", scopePaths: ["src/pay"] }, capture: capture(11, { freshness: "stale", changedFiles: ["src/pay/form.tsx", "src/pay/a.ts", "src/pay/b.ts"] }), promises: [] },
  { stepId: "confirm", screen: null, capture: capture(12, { freshness: "unknown", unknownCause: "dirty", dirty: true }), promises: [] },
  { stepId: "receipt", screen: { screenId: "receipt", title: "Receipt", scopePaths: [] }, capture: null, promises: [] },
], arrows: [
  { fromStepId: "cart", toStepId: "pay", state: "walked" },
  { fromStepId: "pay", toStepId: "confirm", state: "never_walked" },
  { fromStepId: "confirm", toStepId: "receipt", state: "stale" },
] }] };

const doc = (sequence: number, phase: "before" | "after"): CaptureDocument => ({ sequence, signalId: null, imageEvidenceId: `img-${phase}`, contractId: "checkout",
  stepId: "cart", revision: "r", dirty: false, observer: "kit-1", actorId: "kit-1", pr: 7, phase, occurredAt: null });
const pair: BeforeAfterPair = { contractId: "checkout", stepId: "cart", pr: 7, before: doc(20, "before"), after: doc(21, "after"), observer: "kit-1", actorId: "kit-1" };
const events = [{ sequence: 10, kind: "signal_recorded", occurredAt: new Date(Date.now() - 5 * 60000).toISOString() } as RuntimeEvent];

function props(overrides: Partial<JourneyCanvasProps> = {}): JourneyCanvasProps {
  return { view, contractId: null, onSelectContract: vi.fn(), loadImage: vi.fn((id: string) => Promise.resolve(new Blob([id], { type: "image/png" }))),
    events, botName: (id) => `Bot ${id}`, beforeAfter: [pair], onOpenRecords: vi.fn(), ...overrides };
}

describe("JourneyCanvas", () => {
  it("draws each step's state and each arrow's state", async () => {
    const p = props();
    render(<JourneyCanvas {...p} />);
    const cart = screen.getByRole("button", { name: "Cart, open detail" });
    expect(cart).toHaveAttribute("data-state", "fresh");
    expect(cart).toHaveTextContent("Bot kit-1 · abcdef1 · 5m");
    const pay = screen.getByRole("button", { name: "Pay, open detail" });
    expect(pay).toHaveAttribute("data-state", "stale");
    expect(pay).toHaveTextContent("Code changed after this shot · src/pay/form.tsx +2 more");
    expect(pay.querySelector(".journey-crack")).not.toBeNull();
    const confirm = screen.getByRole("button", { name: "confirm, open detail" });
    expect(confirm).toHaveAttribute("data-state", "unknown");
    expect(confirm).toHaveTextContent("Freshness unknown — taken from uncommitted code");
    expect(confirm).toHaveTextContent("age unknown");
    expect(screen.getByRole("button", { name: "Receipt, open detail" })).toHaveTextContent("Not captured yet");
    expect(screen.getByLabelText("Arrow: walked")).toHaveAttribute("data-state", "walked");
    expect(screen.getByLabelText("Arrow: never walked")).toHaveTextContent("never walked");
    expect(screen.getByLabelText("Arrow: stale")).toHaveTextContent("stale");
    await waitFor(() => expect(screen.getByAltText("Cart screenshot").getAttribute("src")).toMatch(/^blob:/));
    expect(p.loadImage).toHaveBeenCalledWith("img-10");
  });

  it("opens a detail with the full image, before/after, records and promise", async () => {
    const p = props();
    render(<JourneyCanvas {...p} />);
    await userEvent.click(screen.getByRole("button", { name: "Cart, open detail" }));
    const dialog = screen.getByRole("dialog", { name: "Cart detail" });
    expect(dialog).toHaveTextContent("The cart shows the total");
    await waitFor(() => expect(within(dialog).getByAltText("After screenshot").getAttribute("src")).toMatch(/^blob:/));
    expect(within(dialog).getByAltText("Before screenshot")).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Records" }));
    expect(p.onOpenRecords).toHaveBeenCalledWith([10, 20, 21]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    await userEvent.click(screen.getByRole("button", { name: "Pay, open detail" }));
    expect(screen.getByRole("dialog", { name: "Pay detail" })).toHaveTextContent("No promise text in the contract");
  });

  it("revokes its image URLs on unmount and shows empty states", async () => {
    const { unmount } = render(<JourneyCanvas {...props()} />);
    await waitFor(() => expect(create).toHaveBeenCalledTimes(3));
    unmount();
    expect(revoke).toHaveBeenCalledTimes(3);
    render(<JourneyCanvas {...props({ view: null })} />);
    expect(screen.getByLabelText("Journey")).toHaveTextContent("Loading journeys…");
    cleanup();
    render(<JourneyCanvas {...props({ view: { head: null, journeys: [] } })} />);
    expect(screen.getByLabelText("Journey")).toHaveTextContent("No journeys mapped yet");
  });

  it("repeats the stale state and every changed file in the detail of a stale capture", async () => {
    render(<JourneyCanvas {...props()} />);
    await userEvent.click(screen.getByRole("button", { name: "Pay, open detail" }));
    const dialog = screen.getByRole("dialog", { name: "Pay detail" });
    expect(dialog).toHaveTextContent("Code changed after this shot");
    const files = within(dialog).getByRole("list", { name: "Changed files" });
    expect(within(files).getAllByRole("listitem").map((item) => item.textContent)).toEqual(["src/pay/form.tsx", "src/pay/a.ts", "src/pay/b.ts"]);
  });
});

/** jsdom lays nothing out, so the fit at 1440px is held on the rules that decide it: the step row
 * wraps instead of scrolling sideways, a card shrinks with the canvas, and its text breaks. */
describe("journey tab stylesheet fits the canvas", () => {
  const rule = (selector: string): string => {
    const css = studioStyles.replace(/\/\*[\s\S]*?\*\//g, "");
    const line = css.split(/\r?\n/).find((candidate) => candidate.startsWith(`${selector} {`));
    if (line === undefined) throw new Error(`no rule for ${selector}`);
    return line.slice(line.indexOf("{") + 1, line.lastIndexOf("}"));
  };
  it("wraps the steps and never scrolls the canvas sideways", () => {
    expect(rule(".journey-steps")).toMatch(/flex-wrap:\s*wrap/);
    expect(rule(".journey-steps")).not.toMatch(/overflow-x:\s*auto/);
    expect(rule(".journey-canvas")).toMatch(/overflow-x:\s*hidden/);
  });
  it("lets a card shrink and its labels break instead of clipping", () => {
    expect(rule(".journey-card")).not.toMatch(/(?:^|;|\s)width:\s*\d+px/);
    expect(rule(".journey-card")).toMatch(/min-width:\s*0/);
    expect(rule(".journey-card")).toMatch(/overflow-wrap:\s*anywhere/);
  });
});

describe("a journeys read that failed", () => {
  it("shows the Runtime's own message under the empty state", () => {
    render(<JourneyCanvas {...props({ view: null, failed: true, failure: "journeys require an explicit --project on this Runtime" })} />);
    expect(screen.getByText("Journeys could not be read.")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("explicit --project");
  });
});
