import { describe, expect, it, vi } from "vitest";

import envelopeSchema from "../../../../schemas/event-envelope.schema.json";
import { ACTOR_TYPES, MAX_NODE_TIMEOUT_SECONDS, RuntimeClient, RuntimeError, TIMESTAMP_PATTERN, isPersistedTimestamp } from "./client";
import type { WireActorType } from "./types";

/**
 * Invalid input is a refusal at THIS client, never a different operation and never the store's
 * refusal one hop later (PR #662 review, client.ts:726, client.ts:83 and the sweep sibling).
 */
function clientCounting() {
  const fetchImpl = vi.fn(async () =>
    new Response(JSON.stringify({ ok: true, data: { executionId: "demo", status: "running", headSequence: 5, attention: "can_sleep", attentionReasons: [], untriagedInterruptions: [], silenceUnevaluated: [], nodeStateCounts: {}, startedAt: null, lastEventAt: null, nodeLastEventAt: {} } }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    }),
  );
  return { client: new RuntimeClient("token", { baseUrl: "http://runtime.test", fetch: fetchImpl as never }), fetchImpl };
}

describe("the amend-budget bound", () => {
  it("is the envelope schema's own maximum, imported rather than copied", () => {
    expect(MAX_NODE_TIMEOUT_SECONDS).toBe(315576000);
  });

  it("refuses one second over the bound locally, with zero requests", async () => {
    const { client, fetchImpl } = clientCounting();
    await expect(
      client.amendBudget("demo", { node: "judge", seconds: MAX_NODE_TIMEOUT_SECONDS + 1, computedAtSequence: 3 }),
    ).rejects.toBeInstanceOf(RuntimeError);
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("accepts exactly the bound and goes to the Runtime", async () => {
    const { client, fetchImpl } = clientCounting();
    await client.amendBudget("demo", { node: "judge", seconds: MAX_NODE_TIMEOUT_SECONDS, computedAtSequence: 3 });
    expect(fetchImpl).toHaveBeenCalled();
  });
});

describe("the recorded actor vocabulary", () => {
  /** `WireActorType` restates the schema's enum as literals (a JSON import cannot); this pins
   * the two together so a schema move without a type move is a red, not a silent drift. */
  it("is the envelope schema's own enum, and the literal union matches it exactly", () => {
    const fromSchema = (envelopeSchema as { $defs: { actor: { properties: { type: { enum: string[] } } } } }).$defs.actor.properties.type.enum;
    expect([...ACTOR_TYPES]).toEqual(fromSchema);
    const literals: WireActorType[] = ["owner", "human", "agent", "system"];
    expect([...literals].sort()).toEqual([...fromSchema].sort());
  });
});

describe("the sweep instant", () => {
  it("derives its pattern from the envelope schema's own timestamp definition", () => {
    expect(TIMESTAMP_PATTERN).toBe((envelopeSchema as { $defs: { timestamp: { pattern: string } } }).$defs.timestamp.pattern);
  });

  /** Exactly the wire contract: UTC and Z-terminated, at most nine fraction digits, a real
   * calendar day, and the leap second admitted - no looser (offsets, Feb 30), no stricter. */
  it("refuses what the contract refuses, with zero requests, and accepts what it permits", async () => {
    const refused = [
      "2026-09-02T10:00:00+01:00",   // an offset - the contract is UTC only
      "2026-02-30T10:00:00Z",        // an impossible calendar day (Date.parse would normalize it)
      "2026-09-02T10:00:00.0123456789Z", // ten fraction digits
      "2026-09-02T99:99:99Z",        // impossible time
      "",
      "now",
    ];
    for (const asOf of refused) {
      expect(isPersistedTimestamp(asOf), asOf).toBe(false);
      const { client, fetchImpl } = clientCounting();
      await expect(client.sweep("demo", { asOf })).rejects.toBeInstanceOf(RuntimeError);
      expect(fetchImpl, asOf).not.toHaveBeenCalled();
    }
    for (const asOf of ["2026-09-02T10:00:00Z", "2026-12-31T23:59:60Z", "2024-02-29T00:00:00.123456789Z"]) {
      expect(isPersistedTimestamp(asOf), asOf).toBe(true);
    }
    const { client, fetchImpl } = clientCounting();
    await client.sweep("demo", { asOf: "2026-12-31T23:59:60Z" });
    expect(fetchImpl).toHaveBeenCalled();
  });
});
