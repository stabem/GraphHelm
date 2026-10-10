import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import { ProofRun } from "./proof-run";
import type { JourneyRunSource } from "./journey-flows";
import type { JourneyRunView, JourneyView } from "../runtime/types";

const journey = {
  contractId: "cart",
  title: "Cart",
  arrows: [],
  steps: [
    { stepId: "a", screen: { screenId: "a", title: "Home" } },
    { stepId: "b", screen: { screenId: "b", title: "Cart" } },
  ],
} as unknown as JourneyView;

function source(start: JourneyRunSource["start"], read: JourneyRunSource["read"] = start as unknown as JourneyRunSource["read"]) {
  return {
    start: vi.fn(start),
    read: vi.fn(read),
    screenFrame: vi.fn(async (_f: string, id: string) => ({ blob: new Blob([id]), etag: null })),
    liveFrame: vi.fn(async () => null),
  };
}

function mount(src: JourneyRunSource, onRun = vi.fn()) {
  const onSelect = vi.fn();
  render(<ProofRun journey={journey} source={src} selected={0} onSelect={onSelect} frameUrl={() => null} onMarkSafe={vi.fn()} onRun={onRun} />);
  return { onSelect, onRun };
}

const flush = async () => { await act(async () => { await Promise.resolve(); await Promise.resolve(); }); };

beforeEach(() => {
  vi.useFakeTimers();
  let n = 0;
  vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: vi.fn(() => `blob:${++n}`), revokeObjectURL: vi.fn() }));
});
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("ProofRun (#746)", () => {
  it("opening starts exactly one run, and polling fills the frames", async () => {
    const running: JourneyRunView = { state: "running", kind: "preview", current: "a", screens: {} };
    const midway: JourneyRunView = { state: "running", kind: "preview", current: "b", screens: { a: { frame: true, result: "pass" } } };
    const done: JourneyRunView = { state: "ready", kind: "preview", result: "pass", screens: { a: { frame: true, result: "pass" }, b: { frame: true, result: "pass" } } };
    const reads = [midway, done];
    const src = source(async () => running, async () => reads.shift()!);
    const { onRun } = mount(src);
    await flush();
    expect(src.start).toHaveBeenCalledTimes(1);
    expect(src.start).toHaveBeenCalledWith("cart", false);
    expect(screen.getByRole("status")).toHaveTextContent("Running · step 1 of 2");
    expect(screen.getByText("Preview — not proof")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^1 SEES Home · running/ })).toBeInTheDocument();
    await act(async () => { vi.advanceTimersByTime(1000); });
    await flush();
    expect(screen.getByRole("img", { name: "Frame 1: Home" })).toHaveAttribute("src", expect.stringMatching(/^blob:/));
    await act(async () => { vi.advanceTimersByTime(1000); });
    await flush();
    expect(src.start).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toHaveTextContent("2 passed");
    expect(onRun).toHaveBeenLastCalledWith(done);
  });

  it("a held step shows the confirm, which starts a confirmed run", async () => {
    const held: JourneyRunView = { state: "ready", kind: "replay", result: "pass", screens: { a: { frame: true, result: "pass" } }, edges: { e: { result: "skipped", reason: "confirm_needed" } }, held: { edge: "e", act: "click", base: "http://app" } };
    const src = source(async () => held);
    mount(src);
    await flush();
    const group = screen.getByRole("group", { name: "Step waiting for you" });
    expect(group).toHaveTextContent("The next step changes data at http://app");
    await act(async () => { screen.getByRole("button", { name: "Run it" }).click(); });
    expect(src.start).toHaveBeenLastCalledWith("cart", false, true);
  });

  it("a refused run shows the Runtime's message as an alert", async () => {
    const src = source(async () => { throw new Error("Runtime is down"); });
    mount(src);
    await flush();
    expect(screen.getByRole("alert")).toHaveTextContent("Runtime is down");
  });

  it("Run again starts a new run", async () => {
    const done: JourneyRunView = { state: "ready", kind: "replay", result: "pass", screens: { a: { frame: true, result: "pass" } } };
    const src = source(async () => done);
    mount(src);
    await flush();
    await act(async () => { screen.getByRole("button", { name: "Run again" }).click(); });
    await flush();
    expect(src.start).toHaveBeenCalledTimes(2);
    expect(src.start).toHaveBeenLastCalledWith("cart", true);
  });
});
