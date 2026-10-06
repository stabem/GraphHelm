import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import { RUNTIME_SILENT, type NeedsYouItem, type QuestionItem } from "../runtime/needs-you";
import { Beacon, beaconLabel, QuestionCards } from "./beacon";

const userEvent = fastUserEvent();
afterEach(cleanup);

const question: QuestionItem = { kind: "question", key: "question:sig-q", asker: "kit-3", text: "Merge now?", signalId: "sig-q",
  recommendations: ["Wait", "Merge"], at: null, sequence: 5 };

describe("Beacon", () => {
  it("names lit, dark and unknown in words", () => {
    expect(beaconLabel({ kind: "lit", count: 1 })).toBe("1 decision needs you");
    expect(beaconLabel({ kind: "lit", count: 3 })).toBe("3 decisions need you");
    expect(beaconLabel({ kind: "dark" })).toBe("Nothing needs you");
    expect(beaconLabel({ kind: "unknown", reason: RUNTIME_SILENT })).toBe(RUNTIME_SILENT);
  });

  it("never renders unknown as dark", () => {
    const { container } = render(<Beacon state={{ kind: "unknown", reason: RUNTIME_SILENT }} onOpen={vi.fn()} />);
    expect(container.querySelector(".beacon-dark")).toBeNull();
    expect(screen.getByRole("button", { name: RUNTIME_SILENT })).toHaveClass("beacon-unknown");
  });

  it("opens the cards on click", async () => {
    const onOpen = vi.fn();
    render(<Beacon state={{ kind: "lit", count: 1 }} onOpen={onOpen} />);
    await userEvent.click(screen.getByRole("button", { name: "1 decision needs you" }));
    expect(onOpen).toHaveBeenCalled();
  });
});

describe("QuestionCards", () => {
  const handlers = () => ({ onChoose: vi.fn(), onAnswer: vi.fn(), onCheck: vi.fn(), stepActions: vi.fn(() => <button type="button">approve cart</button>) });

  it("turns a question's recommendations into one row of choice buttons plus Answer, with no Refuse in phase 1", async () => {
    const h = handlers();
    render(<QuestionCards items={[question]} names={{ "kit-3": "loja kit 3" }} busy={false} {...h} />);
    const card = screen.getByRole("article", { name: "loja kit 3 asks" });
    expect(within(card).getAllByRole("button").map((button) => button.textContent)).toEqual(["Wait", "Merge", "Answer"]);
    await userEvent.click(within(card).getByRole("button", { name: "Merge" }));
    expect(h.onChoose).toHaveBeenCalledWith(question, "Merge");
    await userEvent.click(within(card).getByRole("button", { name: "Answer" }));
    expect(h.onAnswer).toHaveBeenCalledWith(question);
    expect(within(card).queryByRole("button", { name: /refuse/i })).toBeNull();
  });

  it("puts Refuse beside Answer and calls onRefuse; disabled without a signal id or while busy", async () => {
    const h = handlers();
    const onRefuse = vi.fn();
    const { rerender } = render(<QuestionCards items={[question]} names={{}} busy={false} onRefuse={onRefuse} {...h} />);
    const card = screen.getByRole("article", { name: "kit-3 asks" });
    expect(within(card).getAllByRole("button").map((button) => button.textContent)).toEqual(["Wait", "Merge", "Answer", "Refuse"]);
    await userEvent.click(within(card).getByRole("button", { name: "Refuse" }));
    expect(onRefuse).toHaveBeenCalledWith(question);
    rerender(<QuestionCards items={[{ ...question, signalId: null }]} names={{}} busy={false} onRefuse={onRefuse} {...h} />);
    expect(screen.getByRole("button", { name: "Refuse" })).toBeDisabled();
    rerender(<QuestionCards items={[question]} names={{}} busy onRefuse={onRefuse} {...h} />);
    expect(screen.getByRole("button", { name: "Refuse" })).toBeDisabled();
  });

  it("says an unconfirmed native request in plain words and keeps transport ids in details", async () => {
    const h = handlers();
    const item: NeedsYouItem = { kind: "native_request", key: "native:r1", requestId: "r1", threadId: "t-1", nodeId: "start", title: "loja kit 2", state: "unobserved", detail: "Send outcome is unobserved." };
    render(<QuestionCards items={[item]} names={{}} busy={false} {...h} />);
    expect(screen.getByText("Your message to loja kit 2 was not confirmed. It may not have arrived.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Check it" }));
    expect(h.onCheck).toHaveBeenCalledWith(item);
    expect(screen.getByText("r1").closest("details")).not.toBeNull();
  });

  it("delegates step and draft actions to the page", () => {
    const h = handlers();
    render(<QuestionCards items={[{ kind: "blocked_step", key: "blocked_node:cart", nodeId: "cart", name: "Cart page", reason: "blocked_node" }]} names={{}} busy={false} {...h} />);
    expect(screen.getByText("Cart page is blocked.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "approve cart" })).toBeInTheDocument();
  });

  it("renders nothing when nothing needs you", () => {
    const { container } = render(<QuestionCards items={[]} names={{}} busy={false} {...handlers()} />);
    expect(container).toBeEmptyDOMElement();
  });
});
