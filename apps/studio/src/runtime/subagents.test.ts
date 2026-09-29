import { describe, expect, it, vi } from "vitest";
import { digestOf } from "./customs";
import { isSubagentLifecycleSignal, readSubagentRelationships } from "./subagents";
import type { EvidenceContent, RuntimeEvent } from "./types";

const executionId = "run-new";
const childAgentId = "child-1";

async function evidenceFor(overrides: Record<string, unknown> = {}, evidenceId = "ev-1"): Promise<EvidenceContent> {
  const envelope = {
    id: typeof overrides.signalId === "string" ? overrides.signalId : "sig-envelope",
    source: { type: "tool", id: `codex-session-${typeof overrides.parentSessionId === "string" ? overrides.parentSessionId : "parent-1"}` },
    type: `agent_subagent_${overrides.phase === "stopped" ? "stopped" : "started"}`,
    evidence: [executionId],
    description: JSON.stringify({ protocol: "graphhelm-subagent-v1", executionId, host: "codex", parentSessionId: "parent-1", childAgentId, agentType: "worker", phase: "started", declaredNodeId: null, ...overrides }),
  };
  const content = JSON.stringify(envelope);
  return {
    evidenceId,
    mediaType: "application/json",
    sensitivity: "internal",
    contentSha256: (await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle)).slice("sha256:".length),
    content,
  };
}

function event(sequence: number, kind: "agent_subagent_started" | "agent_subagent_stopped", hash: string, overrides: Record<string, unknown> = {}, evidenceId = "ev-1"): RuntimeEvent {
  return {
    sequence,
    kind: "signal_recorded",
    payload: { kind, sourceId: "codex-session-parent-1", sourceKind: "tool", signalId: "sig-envelope", envelopeSha256: hash, ...overrides },
    occurredAt: null,
    actorId: typeof overrides.actorId === "string" ? overrides.actorId : "codex-session-parent-1",
    actorType: typeof overrides.actorType === "string" ? overrides.actorType : "agent",
    idempotencyKey: null,
    eventId: `event-${sequence}`,
    evidenceRefs: [evidenceId],
  };
}

describe("readSubagentRelationships", () => {
  it("classifies lifecycle telemetry separately from chat signals", () => {
    expect(isSubagentLifecycleSignal(event(1, "agent_subagent_started", "hash"))).toBe(true);
    expect(isSubagentLifecycleSignal({ ...event(1, "agent_subagent_started", "hash"), payload: { kind: "operator_note" } })).toBe(false);
  });
  it("correlates a valid start and stop and makes the latest state explicit", async () => {
    const started = await evidenceFor({}, "ev-start");
    const stopped = await evidenceFor({ phase: "stopped" }, "ev-stop");
    const readEvidence = vi.fn(async (_run: string, id: string) => id === "ev-start" ? started : stopped);
    const result = await readSubagentRelationships({
      executionId,
      events: [event(2, "agent_subagent_stopped", stopped.contentSha256, {}, "ev-stop"), event(1, "agent_subagent_started", started.contentSha256, {}, "ev-start")],
      readEvidence,
    });
    expect(result.relationships).toHaveLength(1);
    expect(result.latestByChild[childAgentId]).toMatchObject({ phase: "stopped", startedSequence: 1, stoppedSequence: 2 });
  });

  it.each([
    ["actor", { actorId: "host-session-other" }],
    ["protocol", { protocol: "wrong" }],
    ["identity", { signalId: "other-signal" }],
  ])("rejects evidence with a wrong %s", async (_label, change) => {
    const evidence = await evidenceFor(change);
    const result = await readSubagentRelationships({
      executionId,
      events: [event(1, "agent_subagent_started", evidence.contentSha256, "actorId" in change ? { actorId: change.actorId } : {})],
      readEvidence: async () => evidence,
    });
    expect(result.relationships).toHaveLength(0);
    expect(result.rejected).toBe(1);
  });

  it("rejects a hash mismatch and keeps a prior execution out of the model", async () => {
    const evidence = await evidenceFor();
    const result = await readSubagentRelationships({
      executionId,
      events: [event(1, "agent_subagent_started", "sha256:wrong")],
      readEvidence: async () => ({ ...evidence, content: JSON.stringify({ ...JSON.parse(evidence.content), executionId: "run-old" }) }),
    });
    expect(result.relationships).toHaveLength(0);
    expect(result.latestByChild).toEqual({});
    expect(result.rejected).toBe(1);
  });
  it("does not pair a stop from another host session with a child start", async () => {
    const started = await evidenceFor({}, "ev-start");
    const foreignStop = await evidenceFor({ parentSessionId: "parent-2", phase: "stopped" }, "ev-foreign");
    const result = await readSubagentRelationships({
      executionId,
      events: [
        event(1, "agent_subagent_started", started.contentSha256, {}, "ev-start"),
        event(2, "agent_subagent_stopped", foreignStop.contentSha256, { sourceId: "codex-session-parent-2", actorId: "codex-session-parent-2" }, "ev-foreign"),
      ],
      readEvidence: async (_run, id) => id === "ev-start" ? started : foreignStop,
    });
    expect(result.relationships).toHaveLength(1);
    expect(result.relationships[0].phase).toBe("started");
    expect(result.rejected).toBe(1);
  });
});
