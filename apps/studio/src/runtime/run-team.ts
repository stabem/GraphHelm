import { digestOf } from "./customs";
import type { EvidenceContent, RuntimeEvent } from "./types";

const PROTOCOL = "graphhelm-run-team-v1";
const KINDS = new Set(["run_team_joined", "run_team_reported", "run_team_message", "run_team_acknowledged"]);
const STATES = new Set(["working", "waiting", "blocked", "completed"]);
const MAX_SIGNALS = 2048;
const MAX_EVIDENCE = 16 * 1024;

export function isRunTeamSignal(event: RuntimeEvent): boolean {
  return event.kind === "signal_recorded" && KINDS.has(string(object(event.payload)?.kind) ?? "");
}

export interface RunTeamReadModel {
  executionId: string;
  members: Array<{ actorId: string; host: string; sessionId: string; joinedAt: string | null;
    lastAt: string | null; task: string | null; activity: string | null; reportedState: string | null;
    endedAt: string | null }>;
  messages: Array<{ id: string; sender: string; to: string | null; replyTo: string | null;
    text: string; at: string | null; sequence: number; acknowledged: boolean; acknowledgedAt: string | null }>;
  rejected: number;
  unavailable: boolean;
}

function object(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown> : null;
}

function string(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function rawHash(value: string): string {
  return value.startsWith("sha256:") ? value.slice(7) : value;
}

async function actorFor(host: string, session: string): Promise<string> {
  const direct = `${host}-session-${session}`;
  if (direct.length <= 128) return direct;
  const hash = rawHash(await digestOf(new TextEncoder().encode(`${host}\u0000${session}`).buffer, globalThis.crypto.subtle));
  return `agent-session-${hash.slice(0, 48)}`;
}

/** Rebuilds run membership and chat from verified sealed signals, never from historical actor counts. */
export async function readRunTeam(executionId: string, events: RuntimeEvent[],
  readEvidence: (executionId: string, evidenceId: string) => Promise<EvidenceContent>): Promise<RunTeamReadModel> {
  const result: RunTeamReadModel = { executionId, members: [], messages: [], rejected: 0, unavailable: false };
  const carriers = events.filter(isRunTeamSignal);
  if (carriers.length > MAX_SIGNALS) return { ...result, unavailable: true };
  const members = new Map<string, RunTeamReadModel["members"][number]>();
  const messages = new Map<string, RunTeamReadModel["messages"][number]>();
  const lastTeamSequence = new Map<string, number>();
  const seen = new Set<string>();

  for (const event of [...carriers].sort((a, b) => a.sequence - b.sequence)) {
    const payload = object(event.payload);
    const kind = string(payload?.kind);
    const id = string(payload?.signalId);
    const actor = string(payload?.sourceId);
    const hash = string(payload?.envelopeSha256);
    if (!kind || !id || !actor || !hash || seen.has(id) || event.actorType !== "agent"
        || event.actorId !== actor || payload?.sourceKind !== "tool" || event.evidenceRefs.length !== 1) {
      result.rejected++; continue;
    }
    const evidenceId = event.evidenceRefs[0];
    let evidence: EvidenceContent;
    try { evidence = await readEvidence(executionId, evidenceId); }
    catch { result.rejected++; continue; }
    if (evidence.evidenceId !== evidenceId || evidence.mediaType !== "application/json"
        || evidence.content.length > MAX_EVIDENCE || rawHash(evidence.contentSha256) !== rawHash(hash)
        || rawHash(await digestOf(new TextEncoder().encode(evidence.content).buffer, globalThis.crypto.subtle)) !== rawHash(hash)) {
      result.rejected++; continue;
    }
    let envelope: Record<string, unknown> | null = null;
    let details: Record<string, unknown> | null = null;
    try {
      envelope = object(JSON.parse(evidence.content));
      details = object(JSON.parse(String(envelope?.description ?? "")));
    } catch { /* A damaged sealed envelope cannot create a member or message. */ }
    const source = object(envelope?.source);
    if (!envelope || !details || source?.type !== "tool" || source.id !== actor
        || envelope.id !== id || envelope.type !== kind || details.protocol !== PROTOCOL
        || details.executionId !== executionId || !Array.isArray(envelope.evidence)
        || envelope.evidence.length !== 1 || envelope.evidence[0] !== executionId) {
      result.rejected++; continue;
    }
    seen.add(id);
    if (kind === "run_team_joined") {
      const host = string(details.host);
      const session = string(details.sessionId);
      if (!host || !session || !["codex", "claude"].includes(host) || details.actorId !== actor
          || await actorFor(host, session) !== actor || members.has(actor)
          || envelope.to !== undefined || envelope.replyTo !== undefined) {
        result.rejected++; continue;
      }
      members.set(actor, { actorId: actor, host, sessionId: session, joinedAt: event.occurredAt,
        lastAt: event.occurredAt, task: null, activity: null, reportedState: null, endedAt: null });
      lastTeamSequence.set(actor, event.sequence);
      continue;
    }
    const member = members.get(actor);
    if (!member) { result.rejected++; continue; }
    if (kind === "run_team_reported") {
      const task = string(details.task);
      const activity = string(details.activity);
      if (details.actorId !== actor || !task || !activity || task.length > 500 || activity.length > 2000
          || !STATES.has(String(details.state)) || envelope.to !== undefined || envelope.replyTo !== undefined) {
        result.rejected++; continue;
      }
      member.task = task; member.activity = activity; member.reportedState = String(details.state);
      member.lastAt = event.occurredAt; member.endedAt = null;
      lastTeamSequence.set(actor, event.sequence);
      continue;
    }
    if (kind === "run_team_message") {
      const to = string(envelope.to);
      const replyTo = string(envelope.replyTo);
      const body = string(details.text);
      if (details.messageId !== id || details.sender !== actor || details.recipient !== to
          || !body || body.length > 2000 || (to !== null && (!members.has(to) || to === actor))
          || (replyTo !== null && (messages.get(replyTo)?.to !== actor || to !== messages.get(replyTo)?.sender))) {
        result.rejected++; continue;
      }
      messages.set(id, { id, sender: actor, to, replyTo, text: body, at: event.occurredAt,
        sequence: event.sequence, acknowledged: false, acknowledgedAt: null });
      member.lastAt = event.occurredAt;
      lastTeamSequence.set(actor, event.sequence);
      continue;
    }
    if (kind === "run_team_acknowledged") {
      const replyTo = string(envelope.replyTo);
      const message = replyTo === null ? undefined : messages.get(replyTo);
      if (!message || message.to !== actor || envelope.to !== message.sender
          || details.messageId !== replyTo || details.recipient !== actor
          || details.sender !== message.sender || message.acknowledged) {
        result.rejected++; continue;
      }
      message.acknowledged = true; message.acknowledgedAt = event.occurredAt;
      member.lastAt = event.occurredAt;
      lastTeamSequence.set(actor, event.sequence);
    }
  }
  // An ended session is an observed hook event; silence alone never becomes a disconnect.
  for (const event of events) {
    if (event.kind === "signal_recorded" && object(event.payload)?.kind === "agent_session_ended"
        && event.actorId && members.has(event.actorId)) {
      const member = members.get(event.actorId)!;
      if (event.sequence > (lastTeamSequence.get(member.actorId) ?? 0)) {
        member.endedAt = event.occurredAt;
      }
    }
  }
  result.members = [...members.values()];
  result.messages = [...messages.values()];
  return result;
}
