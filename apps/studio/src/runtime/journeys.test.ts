import { describe, expect, it } from "vitest";

import type { CaptureView, JourneyView, LiveSession, RuntimeEvent } from "./types";
import { beforeAfterPairs, captureAge, captureDocuments, journeySummary, liveChipFor } from "./journeys";

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

describe("captureAge", () => {
  it("reads the time of the event with that sequence, else null", () => {
    expect(captureAge(3, [ev(3, "signal_recorded", {})])).toBe(T + 3000);
    expect(captureAge(9, [ev(3, "signal_recorded", {})])).toBeNull();
  });
});

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

/* #409 (journey-first spec §5 point 5, §9 row D): the Journey tab's live chip is read from the
 * Runtime's session list, never from the Open live click. These cells catch a chip that trusts the
 * click, one that shows another step's session, or a capture reader that drops `phase: live`
 * (phase C records one per live visit). Cost: pure functions, milliseconds. */
describe("live sessions on the Journey tab", () => {
  const session = (over: Partial<LiveSession>): LiveSession => ({ sessionId: "s-1", contractId: "checkout", flowId: "checkout", path: "main",
    stepId: "pay", state: "pass", code: null, at: "pay", screen: null, since: "2026-10-08T03:00:00Z", lastActAt: null, expiresAt: null, ...over });

  it("finds the newest session at a step and names its state, code and place", () => {
    const sessions = [session({ sessionId: "old", since: "2026-10-08T02:00:00Z", state: "fail", code: "driver.expectation_failed" }),
      session({ sessionId: "new", state: "drift", code: "drift.locator_missing", at: "cart.checkout/0" })];
    expect(liveChipFor("checkout", "pay", sessions)).toEqual({ sessionId: "new", state: "drift", label: "drift at cart.checkout/0", code: "drift.locator_missing" });
    expect(liveChipFor("checkout", "cart", sessions)).toBeNull();
    expect(liveChipFor("other", "pay", sessions)).toBeNull();
  });

  it("labels pass, fail and unknown as the owner reads them", () => {
    expect(liveChipFor("checkout", "pay", [session({ state: "pass" })])?.label).toBe("at step · pass");
    expect(liveChipFor("checkout", "pay", [session({ state: "fail", code: "driver.expectation_failed" })])?.label).toBe("at step · fail · driver.expectation_failed");
    expect(liveChipFor("checkout", "pay", [session({ state: "unknown", code: null })])?.label).toBe("at step · unknown");
  });

  it("keeps a capture recorded with phase live (phase C) instead of dropping it", () => {
    const docs = captureDocuments([cap(1)], envs({ 1: doc({ phase: "live" }) }));
    expect(docs.map((d) => d.phase)).toEqual(["live"]);
  });
});
