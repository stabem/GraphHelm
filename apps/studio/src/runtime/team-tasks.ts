import { digestOf } from "./customs";
import type { EvidenceContent, RuntimeEvent } from "./types";

const PROTOCOL = "graphhelm-native-task-v1";
const MAX_ENVELOPE_BYTES = 1024 * 1024;

export function isClaudeTaskSignal(event: RuntimeEvent): boolean {
  if (event.kind !== "signal_recorded") return false;
  const kind = record(event.payload)?.kind;
  return kind === "agent_task_created" || kind === "agent_task_completed";
}

export interface ClaudeTaskObservation {
  executionId: string;
  nativeTaskId: string;
  taskSubject: string;
  teammateName: string | null;
  parentSessionId: string;
  createdSequence: number | null;
  createdAt: string | null;
  createdEvidenceId: string | null;
  completedSequence: number | null;
  completedAt: string | null;
  completedEvidenceId: string | null;
}

export interface ClaudeTaskReadModel {
  executionId: string;
  tasks: ClaudeTaskObservation[];
  rejected: number;
}

export interface ReadClaudeTasksOptions {
  executionId: string;
  events: RuntimeEvent[];
  readEvidence: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
}

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function text(value: unknown, maximum: number): string | null {
  return typeof value === "string" && value.length > 0 && value.length <= maximum && !/[\u0000-\u001f\u007f]/.test(value)
    ? value
    : null;
}

function sequence(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function taskIdentity(value: unknown): string | null {
  return typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/.test(value) ? value : null;
}

function json(content: EvidenceContent): Record<string, unknown> | null {
  if (content.mediaType !== "application/json" || typeof content.content !== "string") return null;
  if (new TextEncoder().encode(content.content).byteLength > MAX_ENVELOPE_BYTES) return null;
  try {
    return record(JSON.parse(content.content));
  } catch {
    return null;
  }
}

function rawHash(value: string): string {
  return value.startsWith("sha256:") ? value.slice("sha256:".length) : value;
}

/** Reads only sealed Claude task lifecycle envelopes. Completion means Claude marked a task
 * complete; it does not mean its output was reviewed or accepted. */
export async function readClaudeTasks({ executionId, events, readEvidence }: ReadClaudeTasksOptions): Promise<ClaudeTaskReadModel> {
  const ordered = [...events].sort((a, b) => a.sequence - b.sequence);
  const tasks = new Map<string, ClaudeTaskObservation>();
  let rejected = 0;
  for (const event of ordered) {
    if (!isClaudeTaskSignal(event)) continue;
    const payload = record(event.payload);
    const kind = payload?.kind;
    const phase = kind === "agent_task_created" ? "created" : "completed";
    const seq = sequence(event.sequence);
    const evidenceId = event.evidenceRefs.length === 1 ? event.evidenceRefs[0] : null;
    const sourceId = text(payload?.sourceId, 128);
    const signalId = text(payload?.signalId, 256);
    const envelopeHash = text(payload?.envelopeSha256, 128);
    if (seq === null || evidenceId === null || sourceId === null || signalId === null || envelopeHash === null) {
      rejected++;
      continue;
    }
    let evidence: EvidenceContent;
    try { evidence = await readEvidence(executionId, evidenceId); } catch { rejected++; continue; }
    if (evidence.evidenceId !== evidenceId || evidence.content.length > MAX_ENVELOPE_BYTES) { rejected++; continue; }
    const envelope = json(evidence);
    const detailText = typeof envelope?.description === "string" && envelope.description.length <= MAX_ENVELOPE_BYTES
      ? envelope.description : null;
    const detail = detailText === null ? null : (() => {
      try { return record(JSON.parse(detailText)); } catch { return null; }
    })();
    const source = record(envelope?.source);
    const host = detail?.host;
    const parent = taskIdentity(detail?.parentSessionId);
    const taskId = taskIdentity(detail?.nativeTaskId);
    const subject = text(detail?.taskSubject, 256);
    const teammate = detail?.teammateName === null ? null : text(detail?.teammateName, 128);
    let actor = typeof host === "string" && parent !== null ? `${host}-session-${parent}` : null;
    if (actor !== null && actor.length > 128) {
      const identityHash = rawHash(await digestOf(new TextEncoder().encode(`${host}\u0000${parent}`).buffer, globalThis.crypto.subtle));
      actor = `agent-session-${identityHash.slice(0, 48)}`;
    }
    const actualHash = rawHash(evidence.contentSha256);
    const computedHash = rawHash(await digestOf(new TextEncoder().encode(evidence.content).buffer, globalThis.crypto.subtle));
    if (
      detail?.protocol !== PROTOCOL || detail?.executionId !== executionId || detail?.phase !== phase || host !== "claude" ||
      parent === null || taskId === null || subject === null || (detail?.teammateName !== null && teammate === null) ||
      actor === null || actor.length > 128 || source?.type !== "tool" || source?.id !== actor ||
      sourceId !== actor || payload?.sourceKind !== "tool" || event.actorId !== actor || event.actorType !== "agent" ||
      envelope?.id !== signalId || envelope?.type !== kind || envelope?.evidence === undefined ||
      !Array.isArray(envelope.evidence) || envelope.evidence.length !== 1 || envelope.evidence[0] !== executionId ||
      rawHash(envelopeHash) !== actualHash || actualHash !== computedHash
    ) { rejected++; continue; }
    const key = `${actor}\u0000${taskId}`;
    const current = tasks.get(key);
    if (phase === "created") {
      if (current?.createdSequence !== null && current !== undefined) { rejected++; continue; }
      tasks.set(key, { executionId, nativeTaskId: taskId, taskSubject: subject, teammateName: teammate,
        parentSessionId: parent, createdSequence: seq, createdAt: event.occurredAt, createdEvidenceId: evidenceId,
        completedSequence: current?.completedSequence ?? null, completedAt: current?.completedAt ?? null,
        completedEvidenceId: current?.completedEvidenceId ?? null });
    } else {
      if (current?.completedSequence !== null && current !== undefined) { rejected++; continue; }
      tasks.set(key, current ? { ...current, completedSequence: seq, completedAt: event.occurredAt, completedEvidenceId: evidenceId }
        : { executionId, nativeTaskId: taskId, taskSubject: subject, teammateName: teammate,
          parentSessionId: parent, createdSequence: null, createdAt: null, createdEvidenceId: null,
          completedSequence: seq, completedAt: event.occurredAt, completedEvidenceId: evidenceId });
    }
  }
  return { executionId, tasks: [...tasks.values()].sort((a, b) => (a.createdSequence ?? a.completedSequence ?? 0) - (b.createdSequence ?? b.completedSequence ?? 0)), rejected };
}
