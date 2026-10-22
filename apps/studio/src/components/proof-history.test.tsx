import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { ProofRun } from "./proof-run";
import { diffPixels, pastVerdict } from "./proof-history";
import type { JourneyRunSource } from "./journey-flows";
import type { JourneyPastRun, JourneyRunView, JourneyView } from "../runtime/types";

const journey = {
  contractId: "cart", title: "Cart", arrows: [],
  steps: [{ stepId: "a", screen: { screenId: "a", title: "Home" } }, { stepId: "b", screen: { screenId: "b", title: "Cart" } }],
} as unknown as JourneyView;

const done: JourneyRunView = { state: "ready", kind: "replay", result: "pass", startedAt: "2026-10-14T12:00:00Z",
  screens: { a: { frame: true, result: "pass" }, b: { frame: true, result: "pass" } } };
const past = (run: string, startedAt: string, b: "pass" | "fail"): JourneyPastRun => ({ ...done, run, startedAt,
  screens: { a: { frame: true, result: "pass" }, b: { frame: true, result: b } } });

beforeEach(() => {
  let n = 0;
  vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: vi.fn(() => `blob:${++n}`), revokeObjectURL: vi.fn() }));
});
afterEach(() => { vi.unstubAllGlobals(); });

describe("diffPixels (#history)", () => {
  it("counts only pixels that moved past the threshold and paints them red", () => {
    const now = new Uint8ClampedArray([10, 10, 10, 255, 200, 0, 0, 255]);
    const before = new Uint8ClampedArray([12, 10, 10, 255, 0, 0, 0, 255]);
    const { ratio, mask } = diffPixels(now, before);
    expect(ratio).toBe(0.5);
    expect([...mask.slice(4, 7)]).toEqual([230, 40, 60]);
    expect(mask[0]).toBe(mask[1]);
  });
  it("reads a past run's verdict from its steps", () => {
    expect(pastVerdict(past("1", "x", "pass"))).toBe("passed");
    expect(pastVerdict(past("1", "x", "fail"))).toBe("failed");
    expect(pastVerdict({ ...done, state: "failed" })).toBe("failed");
  });
});

describe("Proof history (#history)", () => {
  it("lists earlier runs and compares the selected step with the picked run", async () => {
    const runs = [past("00000001760400000000", "2026-10-13T09:05:00Z", "fail"), past("00000001760300000000", "2026-10-12T08:00:00Z", "pass")];
    const src: JourneyRunSource = {
      start: vi.fn(async () => done), read: vi.fn(async () => done),
      screenFrame: vi.fn(async (_f: string, id: string) => ({ blob: new Blob([id]), etag: null })),
      liveFrame: vi.fn(async () => null),
      history: vi.fn(async () => ({ id: "cart", runs })),
      historyFrame: vi.fn(async (_f: string, run: string, id: string) => ({ blob: new Blob([run + id]), etag: null })),
    };
    render(<ProofRun journey={journey} source={src} selected={1} onSelect={vi.fn()} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    await act(async () => { for (let i = 0; i < 6; i++) await Promise.resolve(); });
    const strip = screen.getByRole("group", { name: "Earlier runs" });
    const buttons = strip.querySelectorAll("button");
    expect(buttons).toHaveLength(2);
    expect(buttons[0]).toHaveAttribute("aria-pressed", "true");
    expect(buttons[0]).toHaveTextContent("✕");
    const compare = screen.getByRole("group", { name: "Compare with an earlier run" });
    expect(compare).toHaveTextContent("This step failed then, passed now.");
    expect(src.historyFrame).toHaveBeenCalledWith("cart", runs[0]!.run, "b");
    fireEvent.click(buttons[1]!);
    await act(async () => { for (let i = 0; i < 6; i++) await Promise.resolve(); });
    expect(buttons[1]).toHaveAttribute("aria-pressed", "true");
    expect(src.historyFrame).toHaveBeenCalledWith("cart", runs[1]!.run, "b");
    expect(screen.queryByText(/failed then/)).not.toBeInTheDocument();
  });

  function history(frameFor: (run: string, id: string) => Blob | null): JourneyRunSource {
    const runs = [past("00000001760400000000", "2026-10-13T09:05:00Z", "pass")];
    return {
      start: vi.fn(async () => done), read: vi.fn(async () => done),
      screenFrame: vi.fn(async (_f: string, id: string) => ({ blob: new Blob([id]), etag: null })),
      liveFrame: vi.fn(async () => null),
      history: vi.fn(async () => ({ id: "cart", runs })),
      historyFrame: vi.fn(async (_f: string, run: string, id: string) => { const blob = frameFor(run, id); return blob ? { blob, etag: null } : null; }),
    };
  }

  it("opens Before and Now in the main frame and Escape returns to the frame (#1776)", async () => {
    render(<ProofRun journey={journey} source={history((run, id) => new Blob([run + id]))} selected={1} onSelect={vi.fn()} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    await act(async () => { for (let i = 0; i < 6; i++) await Promise.resolve(); });
    const browser = screen.getByRole("region", { name: "Emulated browser" });
    expect(browser.querySelector(".fcv")).toBeNull();
    const user = fastUserEvent();
    await user.click(screen.getByRole("button", { name: "Compare Before and Now in the frame" }));
    const view = screen.getByRole("group", { name: "Compare Before and Now" });
    expect(browser).toContainElement(view);
    expect(screen.getByRole("button", { name: "Overlay" })).toHaveAttribute("aria-pressed", "true");
    // The mode control sits in the frame bar, before the frame count (approved Proof design).
    const bar = browser.querySelector(".tc-chrome")!;
    expect(bar).toContainElement(screen.getByRole("group", { name: "Compare mode" }));
    expect(bar.lastElementChild).toHaveClass("tc-count");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("group", { name: "Compare Before and Now" })).toBeNull();
    expect(screen.getByRole("img", { name: "Frame 2: Cart" })).toBeInTheDocument();
  });

  it("offers no compare when the earlier run kept no frame for the step (#1776)", async () => {
    render(<ProofRun journey={journey} source={history(() => null)} selected={1} onSelect={vi.fn()} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    await act(async () => { for (let i = 0; i < 6; i++) await Promise.resolve(); });
    expect(screen.getByRole("group", { name: "Compare with an earlier run" })).toHaveTextContent("That run kept no frame for this step.");
    expect(screen.queryByRole("button", { name: "Compare Before and Now in the frame" })).toBeNull();
  });
});
