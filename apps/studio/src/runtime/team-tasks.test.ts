import { describe, expect, it, vi } from "vitest";
import { digestOf } from "./customs";
import { isClaudeTaskSignal, readClaudeTasks } from "./team-tasks";
import type { EvidenceContent, RuntimeEvent } from "./types";

const executionId = "run-task";

async function sealed(phase: "created" | "completed", id: string, teammateName: string | null = "reviewer"): Promise<EvidenceContent> {
  const actor = "claude-session-session-1";
  const kind = `agent_task_${phase}`;
  const detail = { protocol: "graphhelm-native-task-v1", executionId, host: "claude", parentSessionId: "session-1", nativeTaskId: "task-1", taskSubject: "Review the checkout flow", teammateName, phase };
  const content = JSON.stringify({ id: `signal-${phase}`, source: { type: "tool", id: actor }, type: kind, evidence: [executionId], description: JSON.stringify(detail) });
  const hash = await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle);
  return { evidenceId: id, mediaType: "application/json", sensitivity: "internal", contentSha256: hash.slice("sha256:".length), content };
}

function event(seq: number, phase: "created" | "completed", evidence: EvidenceContent): RuntimeEvent {
  const actor = "claude-session-session-1";
  return { sequence: seq, kind: "signal_recorded", payload: { kind: `agent_task_${phase}`, sourceId: actor, sourceKind: "tool", signalId: `signal-${phase}`, envelopeSha256: evidence.contentSha256 }, occurredAt: null, actorId: actor, actorType: "agent", idempotencyKey: null, eventId: `event-${seq}`, evidenceRefs: [evidence.evidenceId] };
}

describe("readClaudeTasks", () => {
  it("verifies sealed create and completion events but labels them as observations", async () => {
    const created = await sealed("created", "ev-created");
    const completed = await sealed("completed", "ev-completed");
    const result = await readClaudeTasks({ executionId, events: [event(2, "completed", completed), event(1, "created", created)], readEvidence: vi.fn(async (_run, id) => id === created.evidenceId ? created : completed) });
    expect(result.rejected).toBe(0);
    expect(result.tasks).toHaveLength(1);
    expect(result.tasks[0]).toMatchObject({ taskSubject: "Review the checkout flow", teammateName: "reviewer", createdSequence: 1, completedSequence: 2 });
  });

  it("keeps a completion without its creation and rejects tampered evidence", async () => {
    const completed = await sealed("completed", "ev-completed");
    const signal = event(2, "completed", completed);
    const result = await readClaudeTasks({ executionId, events: [signal], readEvidence: vi.fn(async () => completed) });
    expect(result.tasks[0]).toMatchObject({ createdSequence: null, completedSequence: 2 });
    expect(isClaudeTaskSignal(signal)).toBe(true);
    const bad = await readClaudeTasks({ executionId, events: [{ ...signal, payload: { ...(signal.payload as Record<string, unknown>), sourceId: "different-actor" } }], readEvidence: vi.fn(async () => completed) });
    expect(bad.tasks).toHaveLength(0);
    expect(bad.rejected).toBe(1);
  });

  it("preserves an unreported teammate as unknown", async () => {
    const created = await sealed("created", "ev-created", null);
    const result = await readClaudeTasks({ executionId, events: [event(1, "created", created)], readEvidence: vi.fn(async () => created) });
    expect(result.tasks[0].teammateName).toBeNull();
  });
});
