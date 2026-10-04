import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { KeelChainView } from "./panel";
import type { RuntimeEvent } from "../runtime/types";

function signal(sequence: number, kind: string, severity = "low"): RuntimeEvent {
  return {
    sequence, kind: "signal_recorded", occurredAt: null, actorId: null, actorType: null,
    idempotencyKey: null, eventId: null, evidenceRefs: [],
    payload: { kind, severity, signalId: `s${sequence}`, sourceKind: "node", sourceId: "implementation", executionId: "demo" },
  } as RuntimeEvent;
}

describe("KeelChainView", () => {
  it("shows the chain and the skipped step in the node panel", () => {
    const html = renderToStaticMarkup(<KeelChainView nodeId="implementation" events={[signal(1, "jpd.journey"), signal(2, "keel.card")]} />);
    expect(html).toContain(`aria-label="Journey recorded"`);
    expect(html).toContain(`aria-label="Obligation missing"`);
    expect(html).toContain(`aria-label="Proof missing"`);
    expect(html).toContain("Obligation was skipped.");
    expect(html).toContain("Card has no proof yet.");
  });

  it("renders nothing without Keel signals", () => {
    expect(renderToStaticMarkup(<KeelChainView nodeId="implementation" events={[]} />)).toBe("");
  });
});
