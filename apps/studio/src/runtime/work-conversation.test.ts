import { describe, expect, it } from "vitest";

import { agentLanes, mcpTransportFromRecordKey, workConversation } from "./work-conversation";
import type { RunTeamReadModel } from "./run-team";
import type { RuntimeEvent } from "./types";

const signal = (sequence: number, kind: string, actorId = "agent-a"): RuntimeEvent => ({
  sequence, kind: "signal_recorded", payload: { kind }, actorId,
  actorType: actorId === "studio-operator" ? "owner" : "agent", occurredAt: null,
  idempotencyKey: null, eventId: `event-${sequence}`, evidenceRefs: [`evidence-${sequence}`],
});

describe("work conversation", () => {
  it("accepts only the exact MCP record-key shape as transport provenance", () => {
    expect(mcpTransportFromRecordKey("mcp-aaaaaaaaaaaaaaaa-sroute-1-record-1111111111111111")).toBe("aaaaaaaaaaaaaaaa");
    expect(mcpTransportFromRecordKey("mcp-bbbbbbbbbbbbbbbb-n7-record-2222222222222222")).toBe("bbbbbbbbbbbbbbbb");
    for (const key of [null, "mcp-short-s1-record-1111111111111111",
      "mcp-AAAAAAAAAAAAAAAA-s1-record-1111111111111111",
      "mcp-aaaaaaaaaaaaaaaa-s1-signal-1111111111111111",
      "mcp-aaaaaaaaaaaaaaaa-x1-record-1111111111111111", "custom-operator-note"]) {
      expect(mcpTransportFromRecordKey(key)).toBeNull();
    }
  });

  it("keeps interleaved notes on distinct MCP transports without promoting bookkeeping", () => {
    const a = "aaaaaaaaaaaaaaaa";
    const b = "bbbbbbbbbbbbbbbb";
    const events = [
      { ...signal(1, "operator_note", "codex"), idempotencyKey: `mcp-${a}-s1-record-1111111111111111` },
      { ...signal(2, "operator_note", "codex"), idempotencyKey: `mcp-${b}-s2-record-2222222222222222` },
      { ...signal(3, "wake_lease", "codex"), idempotencyKey: `mcp-${a}-s3-record-3333333333333333` },
      { ...signal(4, "operator_note", "codex"), idempotencyKey: `mcp-${a}-s4-record-4444444444444444` },
      { ...signal(5, "operator_note", "claude"), idempotencyKey: "direct-http-note" },
    ];
    const envelopes = Object.fromEntries(events.map((event) => [event.sequence,
      { text: `Work ${event.sequence}`, to: null, replyTo: null }]));
    expect(workConversation("run-a", events, envelopes, null).map((message) =>
      [message.sender, message.transportSession ?? null, message.text])).toEqual([
      ["codex", a, "Work 1"], ["codex", b, "Work 2"], ["codex", a, "Work 4"],
      ["claude", null, "Work 5"],
    ]);
  });

  it("merges opened notes and verified team messages in event order without bookkeeping", () => {
    const team: RunTeamReadModel = { executionId: "run-a", members: [], rejected: 0, unavailable: false,
      messages: [
        { id: "team-1", sender: "agent-b", to: "agent-a", replyTo: null, text: "Please review this.",
          at: null, sequence: 6, acknowledged: true, acknowledgedAt: null },
        { id: "team-2", sender: "agent-a", to: "agent-b", replyTo: "team-1", text: "Review recorded.",
          at: null, sequence: 9, acknowledged: false, acknowledgedAt: null },
      ] };
    const events = [signal(3, "operator_note"), signal(4, "wake_lease"),
      signal(5, "persona_created"), signal(8, "operator_note", "studio-operator")];
    const envelopes = { 3: { text: "Work started.", to: null, replyTo: null },
      4: { text: "Wake lease armed.", to: null, replyTo: null },
      5: { text: "Persona created.", to: null, replyTo: null },
      8: { text: "Please continue.", to: "agent-a", replyTo: null } };
    expect(workConversation("run-a", events, envelopes, team)).toEqual([
      expect.objectContaining({ sequence: 3, sender: "agent-a", text: "Work started.", provenance: "stored" }),
      expect.objectContaining({ sequence: 6, sender: "agent-b", text: "Please review this.", provenance: "verified-team", acknowledged: true }),
      expect.objectContaining({ sequence: 8, sender: "studio-operator", to: "agent-a", text: "Please continue.", provenance: "stored" }),
      expect.objectContaining({ sequence: 9, sender: "agent-a", replyTo: "team-1", text: "Review recorded.", provenance: "verified-team", acknowledged: false }),
    ]);
    expect(workConversation("run-b", events, envelopes, team).map((item) => item.sequence)).toEqual([3, 8]);
    expect(workConversation("run-a", events, {}, null)).toEqual([]);
  });
});

describe("agentLanes (#294)", () => {
  const note = (sequence: number, actorId: string, actorType = "agent") => ({
    sequence, kind: "signal_recorded", payload: { kind: "operator_note" }, occurredAt: `2026-10-01T00:00:${String(sequence).padStart(2, "0")}Z`,
    actorId, actorType, idempotencyKey: null, eventId: `e${sequence}`, evidenceRefs: [],
  }) as unknown as Parameters<typeof agentLanes>[0][number];
  it("groups notes per agent, newest lane first, counting sealed notes too", () => {
    const lanes = agentLanes([note(1, "codex-lojakit-1"), note(2, "codex-lojakit-3"), note(3, "codex-lojakit-1"),
      note(4, "studio-operator", "owner")], { 1: { text: "first" }, 2: { text: "  checkout done \n more" } });
    expect(lanes.map((lane) => [lane.actorId, lane.count])).toEqual([["codex-lojakit-1", 2], ["codex-lojakit-3", 1]]);
    expect(lanes[0]).toMatchObject({ latestSequence: 3, latestText: "first" });
    expect(lanes[1].latestText).toBe("checkout done \n more");
  });
});
