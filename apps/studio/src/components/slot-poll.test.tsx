import { describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import { SlotPoll } from "./slot-poll";
import { LanesTimeline } from "./lanes-timeline";
import { parseSlots } from "../runtime/slots";

const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });

describe("SlotPoll", () => {
  it("polls every 10s and stops when unmounted", async () => {
    vi.useFakeTimers();
    const client = { workspaceSlots: vi.fn().mockResolvedValue(parseSlots({ slots: [{ root: "D:/gh", holder: null, waiting: [] }] })) };
    const r = render(<SlotPoll client={client}>{(s) => <span>{`n=${s.length}`}</span>}</SlotPoll>);
    await flush();
    expect(screen.getByText("n=1")).toBeTruthy();
    await act(async () => { vi.advanceTimersByTime(10_000); });
    expect(client.workspaceSlots).toHaveBeenCalledTimes(2);
    r.unmount();
    await act(async () => { vi.advanceTimersByTime(30_000); });
    expect(client.workspaceSlots).toHaveBeenCalledTimes(2);
    vi.useRealTimers();
  });
  it("a 403 is quiet: no slots", async () => {
    const client = { workspaceSlots: vi.fn().mockRejectedValue(Object.assign(new Error("no"), { status: 403 })) };
    render(<SlotPoll client={client}>{(s) => <span>{`n=${s.length}`}</span>}</SlotPoll>);
    await flush();
    expect(screen.getByText("n=0")).toBeTruthy();
  });
});

describe("Lanes build slots", () => {
  const now = Date.parse("2026-10-09T12:00:00Z");
  const lane = (name: string) => ({ lane: name, silent: false, lastEventAt: now, bars: [] });
  it("shows Building and Waiting Nth, and an unavailable root in amber", () => {
    const slots = parseSlots({ slots: [
      { root: "D:/gh", holder: { lane: "a", heldSeconds: 240 }, waiting: [{ lane: "x", waitedSeconds: 1 }, { lane: "b", waitedSeconds: 720 }] },
      { root: "E:/gh", error: [{ code: "workspace.slot_root_missing" }] },
    ] });
    const { container } = render(<LanesTimeline lanes={[lane("a"), lane("b")]} now={now} windowMs={3_600_000} slots={slots} />);
    const tags = Array.from(container.querySelectorAll(".ab-tag")).map((t) => t.textContent);
    expect(tags).toContain("Building · 4m");
    expect(tags).toContain("Waiting for build · 2nd · 12m");
    expect(screen.getByText("queue unavailable").closest(".ab-slot")?.getAttribute("data-state")).toBe("unavailable");
    expect(container.querySelector('[aria-label="Build slots"]')).toBeTruthy();
  });
  it("no slot roots: no strip", () => {
    const { container } = render(<LanesTimeline lanes={[lane("a")]} now={now} windowMs={3_600_000} slots={[]} />);
    expect(container.querySelector('[aria-label="Build slots"]')).toBeNull();
  });
});
