import { describe, expect, it } from "vitest";
import { isSubagentLifecycleSignal } from "./subagents";
import type { RuntimeEvent } from "./types";

const event = (sequence: number, kind: "agent_subagent_started" | "agent_subagent_stopped"): RuntimeEvent => ({
  sequence,
  kind: "signal_recorded",
  payload: { kind },
  occurredAt: null,
  actorId: "actor",
  actorType: "agent",
  idempotencyKey: null,
  eventId: `event-${sequence}`,
  evidenceRefs: [],
});

describe("subagent lifecycle signals", () => {
  it("classifies lifecycle telemetry separately from chat signals", () => {
    expect(isSubagentLifecycleSignal(event(1, "agent_subagent_started"))).toBe(true);
    expect(isSubagentLifecycleSignal({ ...event(1, "agent_subagent_started"), payload: { kind: "operator_note" } })).toBe(false);
  });
});
