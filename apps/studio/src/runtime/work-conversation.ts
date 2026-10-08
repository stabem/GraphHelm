import type { RunTeamReadModel } from "./run-team";
import type { RuntimeEvent } from "./types";

export interface WorkMessage {
  id: string;
  sequence: number;
  sender: string;
  to: string | null;
  replyTo: string | null;
  /** The task the envelope names (signal 1.2.0, #393); absent when it names none. */
  task?: string | null;
  text: string;
  at: string | null;
  provenance: "stored" | "verified-team";
  acknowledged: boolean;
  /** GraphHelm MCP process nonce from a recognized record key; never a native chat identity. */
  transportSession?: string | null;
}

/** MCP record keys carry a 16-hex process nonce, typed RPC id, and request digest. */
export function mcpTransportFromRecordKey(key: string | null): string | null {
  if (key === null) return null;
  return /^mcp-([0-9a-f]{16})-[sn][a-z0-9-]{1,32}-record-[0-9a-f]{16}$/.exec(key)?.[1] ?? null;
}

/** Opened operator notes and verified team messages, in journal order. Transport and lifecycle
 * events never become conversation turns, and a write alone is not delivery proof. */
export function workConversation(
  executionId: string,
  events: RuntimeEvent[],
  envelopes: Record<number, { to: string | null; replyTo: string | null; task?: string | null; text: string }>,
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
      to: envelope.to, replyTo: envelope.replyTo, task: envelope.task ?? null, text, at: event.occurredAt,
      provenance: "stored", acknowledged: false,
      transportSession: event.actorType === "agent" ? mcpTransportFromRecordKey(event.idempotencyKey) : null });
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

export interface AgentLane {
  actorId: string;
  /** Every operator note this agent recorded in the run, opened or not. */
  count: number;
  latestSequence: number;
  latestAt: string | null;
  /** The newest note whose sealed words have been opened; null while none is readable yet. */
  latestText: string | null;
  latestTextAt: string | null;
}

/** One lane per agent that recorded operator notes, newest activity first (#294). Counts come
 * from the log itself, so an agent appears even before its sealed words are opened. */
export function agentLanes(
  events: RuntimeEvent[],
  envelopes: Record<number, { text: string }>,
): AgentLane[] {
  const lanes = new Map<string, AgentLane>();
  for (const event of events) {
    const payload = event.payload !== null && typeof event.payload === "object" && !Array.isArray(event.payload)
      ? event.payload as Record<string, unknown> : null;
    if (event.kind !== "signal_recorded" || payload?.kind !== "operator_note"
        || event.actorType !== "agent" || event.actorId === null) continue;
    const lane = lanes.get(event.actorId) ?? { actorId: event.actorId, count: 0, latestSequence: 0,
      latestAt: null, latestText: null, latestTextAt: null };
    lane.count += 1;
    if (event.sequence >= lane.latestSequence) {
      lane.latestSequence = event.sequence;
      lane.latestAt = event.occurredAt;
    }
    const text = envelopes[event.sequence]?.text?.trim();
    if (text) {
      lane.latestText = text;
      lane.latestTextAt = event.occurredAt;
    }
    lanes.set(event.actorId, lane);
  }
  return [...lanes.values()].sort((a, b) => b.latestSequence - a.latestSequence);
}
