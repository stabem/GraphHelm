import { describe, expect, it } from "vitest";
import { keelChain } from "./keel-chain";
import type { RuntimeEvent } from "./types";

let sequence = 0;
function signal(kind: string, severity = "low", sourceId = "implementation"): RuntimeEvent {
  sequence += 1;
  return {
    sequence, kind: "signal_recorded", occurredAt: null, actorId: null, actorType: null,
    idempotencyKey: null, eventId: null, evidenceRefs: [],
    payload: { kind, severity, signalId: `s${sequence}`, sourceKind: "node", sourceId, executionId: "demo" },
  } as RuntimeEvent;
}

const states = (events: RuntimeEvent[]) => keelChain(events, "implementation").steps.map((step) => step.state);

describe("keelChain", () => {
  it("shows nothing for a node that did no Keel work", () => {
    expect(keelChain([signal("no_progress")], "implementation").active).toBe(false);
  });

  it("reads the full chain and a later green proof over an earlier red one", () => {
    const events = [signal("jpd.journey"), signal("jpd.obligation"), signal("keel.card"), signal("keel.proof", "high"), signal("keel.proof")];
    expect(states(events)).toEqual(["recorded", "recorded", "recorded", "green"]);
    expect(keelChain(events, "implementation").gaps).toEqual([]);
  });

  it("names a failed latest proof", () => {
    const events = [signal("keel.card"), signal("keel.proof", "high")];
    expect(keelChain(events, "implementation").gaps).toEqual(["Latest proof failed."]);
  });

  it("names a card with no proof, and accepts a card with no journey", () => {
    expect(keelChain([signal("keel.card")], "implementation").gaps).toEqual(["Card has no proof yet."]);
  });

  it("names a skipped obligation and a proof with no card", () => {
    const events = [signal("jpd.journey"), signal("keel.proof")];
    expect(keelChain(events, "implementation").gaps).toEqual(["Obligation was skipped.", "Card was skipped."]);
  });

  it("ignores another node's signals", () => {
    expect(keelChain([signal("keel.card", "low", "deploy")], "implementation").active).toBe(false);
  });
});

describe("keelChain lock refusals", () => {
  it("shows a lock refusal even before any card exists", () => {
    const chain = keelChain([signal("keel.blocked", "medium"), signal("keel.blocked", "medium")], "implementation");
    expect(chain.active).toBe(true);
    expect(chain.blocked).toBe(2);
    expect(chain.gaps).toEqual(["A Keel lock stopped this agent 2 times."]);
  });
});
