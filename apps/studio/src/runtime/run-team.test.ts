import { describe, expect, it } from "vitest";
import { digestOf } from "./customs";
import { readRunTeam } from "./run-team";
import type { EvidenceContent, RuntimeEvent } from "./types";

const RUN = "run-team-1";
const codex = (id: string) => `codex-session-${id}`;
const claude = (id: string) => `claude-session-${id}`;

function log() {
  const events: RuntimeEvent[] = [];
  const evidence = new Map<string, EvidenceContent>();
  async function add(kind: string, actor: string, details: Record<string, unknown>, to?: string, replyTo?: string) {
    const sequence = events.length + 1;
    const id = `signal-${sequence}`;
    const envelope = { id, source: { type: "tool", id: actor }, type: kind, severity: "low",
      description: JSON.stringify({ protocol: "graphhelm-run-team-v1", executionId: RUN, ...details }),
      evidence: [RUN], ...(to === undefined ? {} : { to }), ...(replyTo === undefined ? {} : { replyTo }) };
    const content = JSON.stringify(envelope);
    const contentSha256 = (await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle)).slice(7);
    const evidenceId = `evidence-${sequence}`;
    evidence.set(evidenceId, { evidenceId, mediaType: "application/json", sensitivity: "internal", content, contentSha256 });
    events.push({ sequence, kind: "signal_recorded", payload: { kind, sourceKind: "tool", sourceId: actor,
      signalId: id, envelopeSha256: contentSha256 }, occurredAt: `2026-10-01T12:${String(sequence).padStart(2, "0")}:00Z`,
      actorId: actor, actorType: "agent", idempotencyKey: null, eventId: `event-${sequence}`, evidenceRefs: [evidenceId] });
    return id;
  }
  return { events, evidence, add, read: () => readRunTeam(RUN, events, async (_run, id) => {
    const found = evidence.get(id);
    if (!found) throw new Error("missing evidence");
    return found;
  }) };
}

describe("issue 86 run team projection", () => {
  it("C1/C2 reconstructs four native members, their reports, a room message, and an addressed receipt", async () => {
    const fixture = log();
    for (const session of ["c1", "c2", "c3"]) await fixture.add("run_team_joined", codex(session),
      { actorId: codex(session), host: "codex", sessionId: session });
    await fixture.add("run_team_joined", claude("a1"), { actorId: claude("a1"), host: "claude", sessionId: "a1" });
    await fixture.add("run_team_reported", codex("c1"), { actorId: codex("c1"), task: "Review API",
      activity: "Reading route contract", state: "working" });
    await fixture.add("run_team_reported", claude("a1"), { actorId: claude("a1"), task: "Check UI",
      activity: "Waiting for browser proof", state: "waiting" });
    const direct = await fixture.add("run_team_message", codex("c1"), { messageId: "signal-7", sender: codex("c1"),
      recipient: claude("a1"), text: "Please check the view." }, claude("a1"));
    await fixture.add("run_team_acknowledged", claude("a1"), { messageId: direct, recipient: claude("a1"),
      sender: codex("c1") }, codex("c1"), direct);
    await fixture.add("run_team_message", claude("a1"), { messageId: "signal-9", sender: claude("a1"),
      recipient: codex("c1"), text: "I will check it." }, codex("c1"), direct);
    await fixture.add("run_team_message", codex("c3"), { messageId: "signal-10", sender: codex("c3"),
      recipient: null, text: "Build passed." });
    const model = await fixture.read();
    expect(model.members).toHaveLength(4);
    expect(model.members.map((member) => member.actorId)).toEqual([codex("c1"), codex("c2"), codex("c3"), claude("a1")]);
    expect(model.members[0]).toMatchObject({ task: "Review API", activity: "Reading route contract", reportedState: "working" });
    expect(model.messages).toHaveLength(3);
    expect(model.messages[0]).toMatchObject({ to: claude("a1"), acknowledged: true });
    expect(model.messages[1].replyTo).toBe(direct);
    expect(model.messages[2]).toMatchObject({ to: null, acknowledged: false });
    expect(model.rejected).toBe(0);
  });

  it("C3 excludes wrong-run evidence and forged recipient receipts without inventing acknowledgement", async () => {
    const fixture = log();
    await fixture.add("run_team_joined", codex("c1"), { actorId: codex("c1"), host: "codex", sessionId: "c1" });
    await fixture.add("run_team_joined", codex("c2"), { actorId: codex("c2"), host: "codex", sessionId: "c2" });
    await fixture.add("run_team_joined", claude("a1"), { actorId: claude("a1"), host: "claude", sessionId: "a1" });
    const message = await fixture.add("run_team_message", codex("c1"), { messageId: "signal-4", sender: codex("c1"),
      recipient: claude("a1"), text: "Review this" }, claude("a1"));
    await fixture.add("run_team_acknowledged", codex("c2"), { messageId: message, recipient: codex("c2"),
      sender: codex("c1") }, codex("c1"), message);
    await fixture.add("run_team_reported", codex("c2"), { executionId: "other-run", actorId: codex("c2"),
      task: "False task", activity: "False progress", state: "working" });
    const model = await fixture.read();
    expect(model.messages[0].acknowledged).toBe(false);
    expect(model.members[1].task).toBeNull();
    expect(model.rejected).toBe(2);
  });

  it("C4 keeps historical actor activity separate from explicit run membership", async () => {
    const fixture = log();
    fixture.events.push({ sequence: 1, kind: "signal_recorded", payload: { kind: "operator_note" },
      occurredAt: null, actorId: "agent-chat", actorType: "agent", idempotencyKey: null,
      eventId: "historical", evidenceRefs: [] });
    const model = await fixture.read();
    expect(model.members).toEqual([]);
    expect(model.messages).toEqual([]);
    expect(model.rejected).toBe(0);
  });
});
