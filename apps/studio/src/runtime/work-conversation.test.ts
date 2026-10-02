import { describe, expect, it } from "vitest";

import { workConversation } from "./work-conversation";
import type { RunTeamReadModel } from "./run-team";
import type { RuntimeEvent } from "./types";

const signal = (sequence: number, kind: string, actorId = "agent-a"): RuntimeEvent => ({
  sequence, kind: "signal_recorded", payload: { kind }, actorId,
  actorType: actorId === "studio-operator" ? "owner" : "agent", occurredAt: null,
  idempotencyKey: null, eventId: `event-${sequence}`, evidenceRefs: [`evidence-${sequence}`],
});

describe("work conversation", () => {
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
