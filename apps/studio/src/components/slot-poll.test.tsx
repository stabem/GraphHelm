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
  it("clears a known queue when a later read fails", async () => {
    const known = parseSlots({ slots: [{ root: "D:/gh", holder: { lane: "a", heldSeconds: 1 }, waiting: [] }] });
    const client = { workspaceSlots: vi.fn().mockResolvedValueOnce(known).mockRejectedValueOnce(Object.assign(new Error("offline"), { status: 503 })) };
    vi.useFakeTimers();
    render(<SlotPoll client={client}>{(s) => <span>{s.some((slot) => slot.ok && slot.holder?.lane === "a") ? "Building" : "clear"}</span>}</SlotPoll>);
    await flush();
    expect(screen.getByText("Building")).toBeTruthy();
    await act(async () => { vi.advanceTimersByTime(10_000); await Promise.resolve(); await Promise.resolve(); });
    expect(screen.getByText("clear")).toBeTruthy();
    vi.useRealTimers();
  });
  it("clears old data immediately when the client changes and ignores its late reply", async () => {
    let resolveOld!: (slots: ReturnType<typeof parseSlots>) => void;
    const old = { workspaceSlots: vi.fn(() => new Promise<ReturnType<typeof parseSlots>>((resolve) => { resolveOld = resolve; })) };
    const next = { workspaceSlots: vi.fn().mockResolvedValue([]) };
    const view = (client: typeof old | typeof next) => <SlotPoll client={client}>{(s) => <span>{s.some((slot) => slot.ok && slot.holder?.lane === "a") ? "Building" : "clear"}</span>}</SlotPoll>;
    const r = render(view(old));
    await flush();
    r.rerender(view(next));
    expect(screen.getByText("clear")).toBeTruthy();
    await flush();
    resolveOld(parseSlots({ slots: [{ root: "D:/gh", holder: { lane: "a", heldSeconds: 1 }, waiting: [] }] }));
    await flush();
    expect(screen.getByText("clear")).toBeTruthy();
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
    const building = Array.from(container.querySelectorAll(".ab-row")).find((row) => row.querySelector(".ab-name")?.textContent === "a")!;
    expect(building.querySelector(".ab-pill")).toHaveTextContent("Building");
    expect(building.querySelector(".ab-pill")).toHaveTextContent("4m");
    expect(building.querySelector(".ab-tag")).toBeNull();
    expect(building.querySelector(".ab-doing")).toHaveTextContent("D:/gh");
    expect(tags).toContain("Waiting for build · 2nd · 12m");
    expect(screen.getByText("queue unavailable").closest(".ab-slot")?.getAttribute("data-state")).toBe("unavailable");
    expect(container.querySelector('[aria-label="Build slots"]')).toBeTruthy();
  });
  it("no slot roots: no strip", () => {
    const { container } = render(<LanesTimeline lanes={[lane("a")]} now={now} windowMs={3_600_000} slots={[]} />);
    expect(container.querySelector('[aria-label="Build slots"]')).toBeNull();
  });
});
