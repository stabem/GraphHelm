import type { RunTeamReadModel } from "./run-team";
import type { RuntimeEvent } from "./types";

export interface WorkMessage {
  id: string;
  sequence: number;
  sender: string;
  to: string | null;
  replyTo: string | null;
  text: string;
  at: string | null;
  provenance: "stored" | "verified-team";
  acknowledged: boolean;
}

/** Opened operator notes and verified team messages, in journal order. Transport and lifecycle
 * events never become conversation turns, and a write alone is not delivery proof. */
export function workConversation(
  executionId: string,
  events: RuntimeEvent[],
  envelopes: Record<number, { to: string | null; replyTo: string | null; text: string }>,
  team: RunTeamReadModel | null,
): WorkMessage[] {
  const messages: WorkMessage[] = [];
  for (const event of events) {
    const payload = event.payload !== null && typeof event.payload === "object" && !Array.isArray(event.payload)
      ? event.payload as Record<string, unknown> : null;
    if (event.kind !== "signal_recorded" || payload?.kind !== "operator_note"
        || (event.actorType !== "agent" && event.actorType !== "owner") || event.actorId === null) continue;
    const envelope = envelopes[event.sequence];
    const text = envelope?.text.trim();
    if (!text) continue;
    messages.push({ id: `event-${event.sequence}`, sequence: event.sequence, sender: event.actorId,
      to: envelope.to, replyTo: envelope.replyTo, text, at: event.occurredAt,
      provenance: "stored", acknowledged: false });
  }
  if (team?.executionId === executionId && !team.unavailable) {
    for (const message of team.messages) {
      messages.push({ id: `team-${message.id}`, sequence: message.sequence, sender: message.sender,
        to: message.to, replyTo: message.replyTo, text: message.text, at: message.at,
        provenance: "verified-team", acknowledged: message.acknowledged });
    }
  }
  return messages.sort((a, b) => a.sequence - b.sequence || a.id.localeCompare(b.id));
}
