import { afterEach, describe, expect, it, vi } from "vitest";

import type { GraphModel } from "../graph/model";
import type { Bot } from "./team";
import type { RuntimeEvent } from "./types";
import { buildHandover, lastSeenKey, readLastSeen, shouldShowHandover, writeLastSeen } from "./handover";

const T0 = Date.parse("2026-10-05T08:00:00Z");
const at = (minutes: number) => new Date(T0 + minutes * 60_000).toISOString();

function ev(sequence: number, minutes: number, kind: string, actorId: string, payload: Record<string, unknown> = {}): RuntimeEvent {
  return { sequence, kind, payload, occurredAt: at(minutes), actorId, actorType: actorId === "system-cli" ? "system" : "agent",
    idempotencyKey: null, eventId: `e${sequence}`, evidenceRefs: [] };
}

function bot(key: string): Bot {
  return { key, actorId: key, name: key.replace("-", " "), hue: 0, role: null, doingNow: "", lastRecordAt: null, lastSequence: 0,
    state: "working", quietMinutes: null, shared: false, native: false, tasks: [] };
}

// seq 1..10 before the owner left (minute 0..9), seq 11..40 while away (minute 20..140).
const before = Array.from({ length: 10 }, (_, i) => ev(i + 1, i, "signal_recorded", "kit-1", { kind: "operator_note" }));
const away: RuntimeEvent[] = [
  ev(11, 20, "node_outcome_recorded", "system-cli", { nodeId: "cart", outcome: "succeeded", nextState: "succeeded" }),
  ev(12, 25, "signal_recorded", "kit-2", { kind: "agent_task_completed" }),
  ev(13, 30, "signal_recorded", "kit-3", { kind: "operator_note", signalId: "sig-q" }),
  ...Array.from({ length: 27 }, (_, i) => ev(14 + i, 40 + i * 4, "signal_recorded", "kit-1", { kind: "operator_note" })),
];
const events = [ev(0, 0, "execution_form_declared", "system-cli", { nodeIds: ["cart", "pay"] }), ev(0.5, 1, "node_ready", "system-cli", { nodeId: "pay" }), ...before, ...away];

describe("shouldShowHandover", () => {
  it("needs a stored position, 15 minutes and 20 events", () => {
    expect(shouldShowHandover(events, 10, 40)).toBe(true);
    expect(shouldShowHandover(events, null, 40)).toBe(false);
    expect(shouldShowHandover(events, 30, 40)).toBe(false);          // 10 events
    expect(shouldShowHandover(before, 1, 10)).toBe(false);           // 9 minutes
  });
});

describe("buildHandover", () => {
  const model = { nodes: [{ id: "cart", declaredName: "Cart page", state: "succeeded" }, { id: "pay", declaredName: "Payment", state: "ready" }] } as unknown as GraphModel;
  const handover = buildHandover({
    events, bots: [bot("kit-1"), bot("kit-2"), bot("kit-3")], model, claudeTasks: null, fromSeq: 10, toSeq: 40,
    openItems: [{ kind: "question", key: "question:sig-q", asker: "kit-3", text: "Merge now?\nmore", signalId: "sig-q", recommendations: [], at: at(30), sequence: 13 }],
  });

  it("lists what shipped, each line citing its sequences", () => {
    expect(handover.shipped).toEqual([
      { text: "Cart page succeeded", sequences: [11] },
      { text: "kit 2 finished a task", sequences: [12] },
    ]);
  });
  it("lists the questions opened while away", () => {
    expect(handover.needsYou).toEqual([{ text: "kit 3 asked: Merge now?", sequences: [13] }]);
  });
  it("also lists every other open item the beacon counts", () => {
    const h = buildHandover({
      events, bots: [], model, claudeTasks: null, fromSeq: 10, toSeq: 40,
      openItems: [
        { kind: "native_request", key: "native:r", requestId: "r", threadId: "t", nodeId: "n", title: "loja kit 2", state: "unobserved", detail: null },
        { kind: "waiting_step", key: "w", nodeId: "pay", name: "Payment", reason: "waiting_input_node" },
        { kind: "draft", key: "draft:d", draftId: "d" },
      ],
    });
    expect(h.needsYou.length).toBe(3);
  });
  it("lists bots with no record for 30 minutes or more inside the gap", () => {
    expect(handover.quiet.map((line) => line.text)).toEqual(["kit 2: no new record for 119 min", "kit 3: no new record for 114 min"]);
  });
  it("lists graph steps that existed before and got no record in the gap", () => {
    expect(handover.untouched).toEqual([{ text: "Payment got no record", sequences: [0.5] }]);
  });
  it("carries the gap's size", () => {
    expect(handover).toMatchObject({ fromSeq: 10, toSeq: 40, eventCount: 30, gapMinutes: 135 });
  });
});

describe("buildHandover screen captures", () => {
  const cap = (sequence: number, minutes: number): RuntimeEvent => ({
    ...ev(sequence, minutes, "signal_recorded", "kit-1", { kind: "jpd.screen_captured", signalId: `s${sequence}` }),
    evidenceRefs: [`signal-s${sequence}`, `img-${sequence}`],
  });
  const doc = (over: Record<string, unknown>) => ({ to: null, replyTo: null, text: JSON.stringify({ protocol: "graphhelm-screen-capture-v1",
    contractId: "checkout", stepId: "cart", revision: "abc1234", dirty: false, viewport: { width: 1, height: 1 }, observer: "kit-1", ...over }) });
  const run = (extra: RuntimeEvent[], envelopes: Record<number, { to: null; replyTo: null; text: string }>) => buildHandover({
    events: [...events, ...extra], bots: [bot("kit-1")], model: null, claudeTasks: null, openItems: [], fromSeq: 10, toSeq: 50, envelopes,
  }).shipped.filter((line) => line.text.includes("captured"));

  it("counts an after capture as shipped and cites its matching before", () => {
    const lines = run([cap(41, 100), cap(42, 101), cap(43, 102)],
      { 41: doc({ pr: 7, phase: "before" }), 42: doc({ pr: 7, phase: "before" }), 43: doc({ pr: 7, phase: "after" }) });
    expect(lines).toEqual([{ text: "kit 1 captured cart after (PR #7)", sequences: [43, 42] }]);
  });
  it("cites only the after when no before exists, and omits PR when absent", () => {
    expect(run([cap(41, 100)], { 41: doc({ phase: "after" }) })).toEqual([{ text: "kit 1 captured cart after", sequences: [41] }]);
  });
  it("ignores before captures, invalid documents and a missing envelopes input", () => {
    expect(run([cap(41, 100), cap(42, 101)], { 41: doc({ phase: "before" }), 42: { to: null, replyTo: null, text: "nope" } })).toEqual([]);
    const h = buildHandover({ events: [...events, cap(41, 100)], bots: [], model: null, claudeTasks: null, openItems: [], fromSeq: 10, toSeq: 50 });
    expect(h.shipped.some((line) => line.text.includes("captured"))).toBe(false);
  });
});

describe("last seen storage", () => {
  afterEach(() => { localStorage.clear(); vi.restoreAllMocks(); });
  it("round-trips per project and run", () => {
    writeLastSeen("ml-saas", "run-a", 42);
    expect(readLastSeen("ml-saas", "run-a")).toBe(42);
    expect(readLastSeen("ml-saas", "run-b")).toBeNull();
  });
  it("treats junk and unavailable storage as no position", () => {
    localStorage.setItem(lastSeenKey("ml-saas", "run-a"), "not a number");
    expect(readLastSeen("ml-saas", "run-a")).toBeNull();
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("blocked"); });
    expect(readLastSeen("ml-saas", "run-a")).toBeNull();
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("blocked"); });
    expect(() => writeLastSeen("ml-saas", "run-a", 1)).not.toThrow();
  });
});
