import { describe, expect, it } from "vitest";

import type { CaptureView, JourneyFlowView, JourneyRunView, JourneyView, RuntimeEvent } from "./types";
import { approvalBlockReason, runProves, stepNeedsFrame, beforeAfterPairs, captureDocuments, journeySummary } from "./journeys";

function capture(over: Partial<CaptureView>): CaptureView {
  return { signalId: "s", sequence: 1, imageEvidenceId: "img", revision: "abc", dirty: false, viewport: { width: 1, height: 1 },
    observer: "kit-1", freshness: "fresh", changedFiles: [], ...over };
}
const step = (id: string, c?: CaptureView) => ({ stepId: id, promises: [], ...(c ? { capture: c } : {}) });

describe("journeySummary", () => {
  it("counts proven, stale and everything else", () => {
    const view: JourneyView = { contractId: "j", title: "J", arrows: [], steps: [
      step("a", capture({})),
      step("b", capture({ freshness: "stale", changedFiles: ["x.ts"] })),
      step("c", capture({ freshness: "unknown", unknownCause: "no_git" })),
      step("d"),
      step("e", capture({ dirty: true })),
    ] };
    expect(journeySummary(view)).toEqual({ proven: 1, stale: 1, other: 3, total: 5 });
  });
});

const T = Date.parse("2026-10-05T08:00:00Z");
function ev(sequence: number, kind: string, payload: unknown, refs: string[] = [], actorId = "kit-1"): RuntimeEvent {
  return { sequence, kind, payload, occurredAt: new Date(T + sequence * 1000).toISOString(), actorId, actorType: "agent",
    idempotencyKey: null, eventId: `e${sequence}`, evidenceRefs: refs };
}
const doc = (over: Record<string, unknown> = {}) => JSON.stringify({ protocol: "graphhelm-screen-capture-v1", contractId: "checkout", stepId: "cart",
  revision: "abc1234", dirty: false, viewport: { width: 10, height: 10 }, observer: "kit-1", ...over });
const cap = (seq: number) => ev(seq, "signal_recorded", { kind: "jpd.screen_captured", signalId: `s${seq}` }, [`signal-s${seq}`, `img-${seq}`]);
const envs = (entries: Record<number, string>) =>
  Object.fromEntries(Object.entries(entries).map(([k, text]) => [k, { to: null, replyTo: null, text }]));

describe("captureDocuments", () => {
  it("parses valid capture envelopes and ignores the rest", () => {
    const events = [
      cap(1), cap(2), cap(3), cap(4), cap(5),
      ev(6, "signal_recorded", { kind: "operator_note" }, ["signal-n", "x"]),
      ev(7, "signal_recorded", { kind: "jpd.screen_captured" }, ["signal-only"]),
    ];
    const docs = captureDocuments(events, envs({
      1: doc({ pr: 7, phase: "before" }), 2: "not json", 3: doc({ protocol: "other" }), 4: doc({ phase: "during" }), 5: doc({ dirty: true }),
      6: doc(), 7: doc(),
    }));
    expect(docs.map((d) => d.sequence)).toEqual([1, 5]);
    expect(docs[0]).toMatchObject({ signalId: "s1", imageEvidenceId: "img-1", contractId: "checkout", stepId: "cart", revision: "abc1234",
      dirty: false, observer: "kit-1", actorId: "kit-1", pr: 7, phase: "before" });
    expect(docs[1].dirty).toBe(true);
  });
  it("falls back to the signal id in the first evidence ref", () => {
    const event = ev(1, "signal_recorded", { kind: "jpd.screen_captured" }, ["signal-abc", "img"]);
    expect(captureDocuments([event], envs({ 1: doc() }))[0].signalId).toBe("abc");
  });
});

describe("beforeAfterPairs", () => {
  it("pairs the newest before and after per (contract, step, pr), newest pair first", () => {
    const events = [cap(1), cap(2), cap(3), cap(4), cap(5), cap(6), cap(7), cap(8)];
    const docs = captureDocuments(events, envs({
      1: doc({ pr: 7, phase: "before" }), 2: doc({ pr: 7, phase: "before" }), 3: doc({ pr: 7, phase: "after" }),
      4: doc({ stepId: "pay", pr: 8, phase: "before" }), 5: doc({ stepId: "pay", pr: 8, phase: "after" }),
      6: doc({ stepId: "ship", pr: 9, phase: "after" }), 7: doc({ stepId: "x", phase: "before" }), 8: doc({ stepId: "x", phase: "after" }),
    }));
    const pairs = beforeAfterPairs(docs);
    expect(pairs.map((p) => [p.stepId, p.pr, p.before.sequence, p.after.sequence])).toEqual([["pay", 8, 4, 5], ["cart", 7, 2, 3]]);
    expect(pairs[0]).toMatchObject({ contractId: "checkout", observer: "kit-1", actorId: "kit-1" });
  });
});

/* #409: a capture reader that drops `phase: live` (phase C records one per live visit) loses the
 * live visit's evidence. Cost: pure functions, milliseconds. */
describe("live sessions on the Journey tab", () => {
  it("keeps a capture recorded with phase live (phase C) instead of dropping it", () => {
    const docs = captureDocuments([cap(1)], envs({ 1: doc({ phase: "live" }) }));
    expect(docs.map((d) => d.phase)).toEqual(["live"]);
  });
});

describe("approvalBlockReason (#1448)", () => {
  const flow = (status: "draft" | "approved"): JourneyFlowView => ({ id: "j", title: "J", status,
    approved: status === "approved" ? { revision: "r", digest: "d" } : null, drift: [], findings: [], approvable: true,
    screens: [{ id: "a", url: "/", state: "stable" }, { id: "b", url: "/", state: "stable" }],
    edges: [{ id: "a.b", from: "a", to: "b" }], paths: { main: ["a.b"] } } as JourneyFlowView);
  const contract = { contractId: "j", title: "J", steps: [{ stepId: "a" }, { stepId: "b" }] };
  const run = (kind: "preview" | "replay"): JourneyRunView => ({ state: "ready", kind, result: "pass",
    screens: { a: { result: "pass", frame: true }, b: { result: "pass", frame: true } }, edges: { "a.b": { result: "pass" } } });

  it("a passing preview unlocks a draft, since a draft cannot be replayed before approval", () => {
    expect(approvalBlockReason(flow("draft"), contract, run("preview"))).toBeNull();
  });
  it("an approved flow still needs a replay; its preview is not proof", () => {
    expect(approvalBlockReason(flow("approved"), contract, run("preview"))).toMatch(/Not run yet/);
    expect(approvalBlockReason(flow("approved"), contract, run("replay"))).toBeNull();
  });
  // #1653: the Proof list and Approve share runProves, so they cannot disagree.
  it("a browser step without its frame blocks Approve, as it keeps the list at not run", () => {
    const frameless = { ...run("preview"), screens: { a: { result: "pass" as const, frame: false }, b: { result: "pass" as const, frame: true } } };
    expect(runProves(flow("draft"), contract, frameless)).toBe(false);
    expect(approvalBlockReason(flow("draft"), contract, frameless)).toMatch(/Not run yet/);
  });
  it("a failed edge is not a pass for the list or for Approve", () => {
    const broken = { ...run("preview"), edges: { "a.b": { result: "fail" as const } } };
    expect(runProves(flow("draft"), contract, broken)).toBe(false);
    expect(approvalBlockReason(flow("draft"), contract, broken)).toMatch(/Not run yet/);
  });
  it("a pass record with no step results proves nothing", () => {
    expect(runProves(flow("draft"), contract, { state: "ready", kind: "preview", result: "pass", screens: {}, edges: {} })).toBe(false);
    expect(runProves(flow("draft"), contract, run("preview"))).toBe(true);
  });
  it("a backend step passes on its result alone; a mixed screen still needs a frame", () => {
    const backend = { ...flow("draft"), screens: [{ id: "a", url: "/", state: "stable", expect: [{ role: "exit", name: "0" }] },
      { id: "b", url: "/", state: "stable", expect: [{ role: "exit", name: "0" }, { role: "stdout", name: "ok" }] }] } as JourneyFlowView;
    const noFrames = { ...run("preview"), screens: { a: { result: "pass" as const, frame: false }, b: { result: "pass" as const, frame: false } } };
    expect(runProves(backend, contract, noFrames)).toBe(true);
    expect(stepNeedsFrame({ expect: [{ role: "heading" }, { role: "exit" }] })).toBe(true);
  });
});
