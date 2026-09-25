import { describe, expect, it } from "vitest";

import { claimVerdict, clearVerdict, digestOf, openWaitSequence } from "./customs";
import type { ExecutionStatus, MutationEvidence, RuntimeEvent } from "./types";

function status(customs: unknown): ExecutionStatus {
  return {
    executionId: "exec-1",
    mode: "supervised",
    status: "running",
    attention: "needs_you",
    attentionReasons: [],
    nodeStateCounts: {},
    untriagedInterruptions: [],
    silenceUnevaluated: [],
    startedAt: null,
    lastEventAt: null,
    nodeLastEventAt: {},
    headSequence: 9,
    customs,
  } as unknown as ExecutionStatus;
}

function event(kind: string, sequence: number, payload: unknown = null): RuntimeEvent {
  return {
    sequence,
    kind,
    payload,
    occurredAt: null,
    actorId: "owner-local",
    actorType: "owner",
    idempotencyKey: null,
  } as unknown as RuntimeEvent;
}

function evidence(over: Partial<MutationEvidence>): MutationEvidence {
  return {
    action: "claim",
    executionId: "exec-1",
    node: "implementation",
    actor: { id: "owner-local", type: "owner" },
    idempotencyKey: "key",
    headBefore: 8,
    headAfter: 9,
    result: "succeeded",
    statusAfter: null,
    newEvents: [],
    diagnostics: [],
    ...over,
  } as unknown as MutationEvidence;
}

describe("openWaitSequence", () => {
  it("reads the sequence the fold recorded for this node", () => {
    const payload = status({ nodes: { implementation: { openWait: { atSequence: 9 } } } });
    expect(openWaitSequence(payload, "implementation")).toBe(9);
  });

  // ZERO IS A SEQUENCE. A guard written as a truthiness test drops it, and the caller then omits
  // `waitSeq` and asks the Runtime to pick a wait — which is the ambiguity the field removes.
  it("keeps a wait at sequence zero", () => {
    const payload = status({ nodes: { implementation: { openWait: { atSequence: 0 } } } });
    expect(openWaitSequence(payload, "implementation")).toBe(0);
  });

  it("answers null for a node with no open wait, and for an absent status", () => {
    const payload = status({ nodes: { implementation: { scans: [] } } });
    expect(openWaitSequence(payload, "implementation")).toBeNull();
    expect(openWaitSequence(payload, "release_notes")).toBeNull();
    expect(openWaitSequence(null, "implementation")).toBeNull();
  });

  it("answers null rather than guessing when the shape is not the one it expects", () => {
    expect(openWaitSequence(status({ nodes: "nope" }), "implementation")).toBeNull();
    expect(
      openWaitSequence(status({ nodes: { implementation: { openWait: { atSequence: "9" } } } }), "implementation"),
    ).toBeNull();
    expect(
      openWaitSequence(status({ nodes: { implementation: { openWait: { atSequence: -1 } } } }), "implementation"),
    ).toBeNull();
  });
});

describe("claimVerdict", () => {
  it("takes the claim sequence from the envelope, not from a field", () => {
    const verdict = claimVerdict(evidence({ newEvents: [event("completion_claimed", 9)] }));
    expect(verdict).toEqual({ outcome: "claimed", claimSeq: 9 });
  });

  // THE TRAP THIS FILE EXISTS FOR: the mutation succeeded and the claim did not. A caller reading
  // `result` would tell a person their claim went through.
  it("reads a refusal that arrived as a 200 with result succeeded", () => {
    const verdict = claimVerdict(
      evidence({
        result: "succeeded",
        newEvents: [event("completion_refused", 9, { reasonCode: "evidence_budget_unmet" })],
      }),
    );
    expect(verdict).toEqual({ outcome: "refused", reasonCode: "evidence_budget_unmet" });
  });

  it("is unknown when no decision event is present, and never defaults to either verdict", () => {
    expect(claimVerdict(evidence({ newEvents: [] })).outcome).toBe("unknown");
    expect(claimVerdict(evidence({ newEvents: [event("node_outcome_recorded", 9)] })).outcome).toBe("unknown");
  });

  it("carries a null reason rather than inventing one when the payload has none", () => {
    expect(claimVerdict(evidence({ newEvents: [event("completion_refused", 9, {})] }))).toEqual({
      outcome: "refused",
      reasonCode: null,
    });
  });
});

describe("clearVerdict", () => {
  it("reads the fold's verdict for this claim", () => {
    const after = status({ clearances: { "9": { type: "cleared" } } });
    expect(clearVerdict(evidence({ statusAfter: after }), 9)).toEqual({ outcome: "cleared" });
  });

  // The event is appended whichever way the verification went, so a surface keyed on the event
  // would call a rejected clearance a success.
  it("reads a rejection although the clearance event was appended", () => {
    const after = status({ clearances: { "9": { type: "refused", reasonCode: "hash_mismatch" } } });
    const verdict = clearVerdict(
      evidence({ action: "clear", statusAfter: after, newEvents: [event("completion_cleared", 10)] }),
      9,
    );
    expect(verdict).toEqual({ outcome: "refused", reasonCode: "hash_mismatch" });
  });

  // A verdict for SOME claim is not a verdict for THIS one. Reading the newest entry would report
  // another claim's fate under this claim's name.
  it("does not answer from another claim's verdict", () => {
    const after = status({ clearances: { "7": { type: "cleared" } } });
    expect(clearVerdict(evidence({ statusAfter: after }), 9).outcome).toBe("unknown");
  });

  it("is unknown when the status could not be re-read", () => {
    expect(clearVerdict(evidence({ statusAfter: null }), 9).outcome).toBe("unknown");
  });
});

describe("digestOf", () => {
  it("produces the wire's own sha256 form", async () => {
    const bytes = new TextEncoder().encode("abc");
    const digest = await digestOf(bytes.buffer as ArrayBuffer, globalThis.crypto.subtle);
    // The published SHA-256 of "abc" — a value this file does not compute, so a change in the
    // hashing or the hex formatting cannot agree with itself.
    expect(digest).toBe("sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
  });

  it("is lowercase hex of exactly 64 characters", async () => {
    const digest = await digestOf(new ArrayBuffer(0), globalThis.crypto.subtle);
    expect(digest).toMatch(/^sha256:[0-9a-f]{64}$/);
  });
});
