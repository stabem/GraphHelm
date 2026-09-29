import { digestOf } from "./customs";
import type { EvidenceContent, RuntimeEvent } from "./types";

const PROTOCOL = "graphhelm-subagent-v1";
const MAX_ENVELOPE_BYTES = 1024 * 1024;

export function isSubagentLifecycleSignal(event: RuntimeEvent): boolean {
  if (event.kind !== "signal_recorded") return false;
  const kind = record(event.payload)?.kind;
  return kind === "agent_subagent_started" || kind === "agent_subagent_stopped";
}

export type SubagentPhase = "started" | "stopped";

export interface SubagentRelationship {
  executionId: string;
  parentSessionId: string;
  childAgentId: string;
  agentType: string;
  declaredNodeId: string | null;
  sourceId: string;
  sourceActorId: string;
  sourceActorType: string;
  startedSequence: number;
  startedAt: string | null;
  startedEvidenceId: string;
  stoppedSequence: number | null;
  stoppedAt: string | null;
  stoppedEvidenceId: string | null;
  lastChildEvent: { sequence: number; kind: string; occurredAt: string | null } | null;
  phase: SubagentPhase;
}

export interface SubagentReadModel {
  executionId: string;
  relationships: SubagentRelationship[];
  latestByChild: Record<string, SubagentRelationship>;
  rejected: number;
}

export interface ReadSubagentRelationshipsOptions {
  executionId: string;
  events: RuntimeEvent[];
  readEvidence: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
}

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function nonEmptyString(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function safeSequence(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function jsonEnvelope(content: EvidenceContent): Record<string, unknown> | null {
  if (content.mediaType !== "application/json" || typeof content.content !== "string") return null;
  const bytes = new TextEncoder().encode(content.content);
  if (bytes.byteLength > MAX_ENVELOPE_BYTES) return null;
  try {
    return record(JSON.parse(content.content));
  } catch {
    return null;
  }
}

function rawSha256(value: string): string {
  return value.startsWith("sha256:") ? value.slice("sha256:".length) : value;
}

/**
 * Reads the sealed, typed lifecycle signals for one execution. Invalid evidence is dropped so a
 * malformed signal cannot invent a child or make an unrelated run appear active.
 */
export async function readSubagentRelationships({
  executionId,
  events,
  readEvidence,
}: ReadSubagentRelationshipsOptions): Promise<SubagentReadModel> {
  const ordered = [...events].sort((a, b) => a.sequence - b.sequence);
  const relationships = new Map<string, SubagentRelationship>();
  let rejected = 0;

  for (const event of ordered) {
    if (event.kind !== "signal_recorded") continue;
    const payload = record(event.payload);
    const signalKind = payload?.kind;
    if (signalKind !== "agent_subagent_started" && signalKind !== "agent_subagent_stopped") continue;
    const sequence = safeSequence(event.sequence);
    const sourceId = nonEmptyString(payload?.sourceId);
    const envelopeSha256 = nonEmptyString(payload?.envelopeSha256);
    const signalId = nonEmptyString(payload?.signalId);
    if (sequence === null || sourceId === null || signalId === null || envelopeSha256 === null || event.evidenceRefs.length !== 1) {
      rejected++;
      continue;
    }
    const evidenceId = event.evidenceRefs[0];
    let evidence: EvidenceContent;
    try {
      evidence = await readEvidence(executionId, evidenceId);
    } catch {
      rejected++;
      continue;
    }
    if (evidence.evidenceId !== evidenceId || evidence.content.length > MAX_ENVELOPE_BYTES) {
      rejected++;
      continue;
    }
    const signal = jsonEnvelope(evidence);
    if (signal === null) {
      rejected++;
      continue;
    }
    const source = record(signal.source);
    const description = typeof signal.description === "string" && signal.description.length <= MAX_ENVELOPE_BYTES
      ? jsonEnvelope({ ...evidence, content: signal.description })
      : null;
    const host = nonEmptyString(description?.host);
    const parentSessionId = nonEmptyString(description?.parentSessionId);
    const childAgentId = nonEmptyString(description?.childAgentId);
    const agentType = nonEmptyString(description?.agentType);
    const phase = description?.phase;
    const envelopeExecutionId = nonEmptyString(description?.executionId);
    let expectedActorId = host !== null && parentSessionId !== null ? `${host}-session-${parentSessionId}` : null;
    if (expectedActorId !== null && expectedActorId.length > 128) {
      const identityHash = rawSha256(await digestOf(new TextEncoder().encode(`${host}\u0000${parentSessionId}`).buffer, globalThis.crypto.subtle));
      expectedActorId = `agent-session-${identityHash.slice(0, 48)}`;
    }
    const computedHash = rawSha256(await digestOf(new TextEncoder().encode(evidence.content).buffer, globalThis.crypto.subtle));
    if (
      description?.protocol !== PROTOCOL ||
      envelopeExecutionId !== executionId ||
      parentSessionId === null ||
      childAgentId === null ||
      agentType === null ||
      (phase !== "started" && phase !== "stopped") ||
      signalKind !== `agent_subagent_${phase}` ||
      expectedActorId === null ||
      source?.type !== "tool" ||
      source?.id !== expectedActorId ||
      sourceId !== expectedActorId ||
      payload?.sourceKind !== "tool" ||
      signal.id !== signalId ||
      signal.type !== signalKind ||
      !Array.isArray(signal.evidence) || signal.evidence.length !== 1 || signal.evidence[0] !== executionId ||
      event.actorId !== expectedActorId ||
      event.actorType !== "agent" ||
      rawSha256(evidence.contentSha256) !== envelopeSha256 ||
      rawSha256(evidence.contentSha256) !== computedHash
    ) {
      rejected++;
      continue;
    }
    const declaredNodeId = nonEmptyString(description?.declaredNodeId);
    const key = `${expectedActorId}\u0000${childAgentId}`;
    const current = relationships.get(key);
    if (phase === "started") {
      if (current !== undefined && current.phase === "started") {
        rejected++;
        continue;
      }
      relationships.set(key, {
        executionId,
        parentSessionId,
        childAgentId,
        agentType,
        declaredNodeId,
        sourceId,
        sourceActorId: event.actorId as string,
        sourceActorType: event.actorType as string,
        startedSequence: sequence,
        startedAt: event.occurredAt,
        startedEvidenceId: evidenceId,
        stoppedSequence: null,
        stoppedAt: null,
        stoppedEvidenceId: null,
        lastChildEvent: null,
        phase: "started",
      });
    } else if (current !== undefined && current.phase === "started" && current.stoppedSequence === null) {
      relationships.set(key, { ...current, stoppedSequence: sequence, stoppedAt: event.occurredAt, stoppedEvidenceId: evidenceId, phase: "stopped" });
    } else {
      rejected++;
    }
  }

  const childCounts = new Map<string, number>();
  for (const relationship of relationships.values()) childCounts.set(relationship.childAgentId, (childCounts.get(relationship.childAgentId) ?? 0) + 1);
  for (const [key, relationship] of relationships) {
    if (childCounts.get(relationship.childAgentId) !== 1) continue;
    const last = ordered.filter((event) => event.actorId === relationship.childAgentId && event.sequence >= relationship.startedSequence).at(-1);
    if (last) relationships.set(key, {
      ...relationship, lastChildEvent: { sequence: last.sequence, kind: last.kind, occurredAt: last.occurredAt },
    });
  }

  const latestByChild: Record<string, SubagentRelationship> = {};
  for (const relationship of relationships.values()) {
    if (childCounts.get(relationship.childAgentId) === 1) latestByChild[relationship.childAgentId] = relationship;
  }
  return { executionId, relationships: [...relationships.values()], latestByChild, rejected };
}
