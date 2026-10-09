/**
 * What needs the owner, in one list (spec §4.3). Dark is a claim that nothing blocks, so a stale
 * connection or a missing status is `unknown` - never dark, and never a count the page cannot back.
 */
import { openQuestions, type EnvelopeRecord } from "../graph/ledger";
import type { ExecutionStatus, NativeChatRequest, NativeChatRequestState, RuntimeEvent } from "./types";

export interface QuestionItem { kind: "question"; key: string; asker: string; text: string; signalId: string | null; recommendations: string[]; at: string | null; sequence: number; task?: string | null }
export interface NativeRequestItem { kind: "native_request"; key: string; requestId: string; threadId: string; nodeId: string; title: string; state: NativeChatRequestState; detail: string | null }
export interface StepItem { kind: "waiting_step" | "blocked_step"; key: string; nodeId: string; name: string; reason: string }
export interface DraftItem { kind: "draft"; key: string; draftId: string }
export type NeedsYouItem = QuestionItem | NativeRequestItem | StepItem | DraftItem;
export type BeaconState = { kind: "lit"; count: number } | { kind: "dark" } | { kind: "unknown"; reason: string };
export const RUNTIME_SILENT = "Can't tell: the Runtime is not answering";

export interface NeedsYouInput {
  status: ExecutionStatus | null;
  stale: boolean;
  events: RuntimeEvent[];
  envelopes: EnvelopeRecord;
  nativeRequests: NativeChatRequest[] | null;
  pendingDraftIds: string[];
  nodeNames: Record<string, string>;
  operatorId: string;
  /** An agent's recorded work stands in this run's log (the same fact the run tag reads). */
  agentWorkRecorded?: boolean;
}

export function needsYou(input: NeedsYouInput): { state: BeaconState; items: NeedsYouItem[] } {
  const items: NeedsYouItem[] = [];
  for (const debt of openQuestions(input.events, input.envelopes, input.operatorId)) {
    items.push({ kind: "question", key: `question:${debt.signalId ?? `seq-${debt.sequence}`}`, asker: debt.asker, text: debt.text,
      signalId: debt.signalId, recommendations: input.envelopes[debt.sequence]?.recommendations ?? [], at: debt.at, sequence: debt.sequence,
      ...(debt.task ? { task: debt.task } : {}) });
  }
  for (const request of input.nativeRequests ?? []) {
    if (request.state !== "unobserved" && request.state !== "blocked") continue;
    items.push({ kind: "native_request", key: `native:${request.requestId}`, requestId: request.requestId, threadId: request.threadId,
      nodeId: request.nodeId, title: request.title, state: request.state, detail: request.detail ?? null });
  }
  for (const draftId of input.pendingDraftIds) items.push({ kind: "draft", key: `draft:${draftId}`, draftId });
  for (const reason of input.status?.attentionReasons ?? []) {
    const node = typeof reason.node === "string" ? reason.node : null;
    const kind = typeof reason.kind === "string" ? reason.kind : "";
    if (node === null) continue;
    const step = kind === "waiting_input_node" ? "waiting_step" : kind === "blocked_node" || kind === "untriaged_interruption" ? "blocked_step" : null;
    if (step === null) continue;
    items.push({ kind: step, key: `${kind}:${node}`, nodeId: node, name: input.nodeNames[node] ?? node, reason: kind });
  }
  // A running graph step that waits while agents record work and nothing else is open is a technical
  // fact, not an owner request (#511): the run tag already reads it as calm, so the beacon does too.
  const graphWaitOnly = input.status?.status === "running" && input.agentWorkRecorded === true
    && items.length > 0 && items.every((item) => item.kind === "waiting_step");
  if (graphWaitOnly) items.length = 0;
  const state: BeaconState = input.status === null || input.stale || input.nativeRequests === null
    ? { kind: "unknown", reason: RUNTIME_SILENT }
    : items.length > 0 ? { kind: "lit", count: items.length } : { kind: "dark" };
  return { state, items };
}
