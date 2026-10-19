/**
 * Pure derivations over the journeys route and the event tail (phase 5, rulings 1, 2, 4).
 * Nothing here reads a file or the clock: a missing event means "unknown", never a guess.
 */
import type { EnvelopeRecord } from "../graph/ledger";
import type { JourneyFlowView, JourneyRunView, JourneyView, RuntimeEvent } from "./types";

/**
 * A flow approval must be backed by the current compiled contract and a complete run. An approved
 * flow needs a replay. A draft cannot be replayed before it is approved, so its proof is a passing
 * preview (#1448 decision); the Runtime returns a run only for the current draft digest, so a draft
 * change shows `none` until the preview runs again.
 */
export function approvalBlockReason(flow: JourneyFlowView, contract: { contractId: string; title: string; steps: readonly { stepId: string }[] }, run: JourneyRunView | null): string | null {
  const path = contract.contractId === flow.id ? "main" : contract.contractId.slice(flow.id.length + 1);
  const edgeIds = flow.paths[path];
  const edges = edgeIds?.map((id) => flow.edges.find((edge) => edge.id === id));
  const stepIds = edges?.length && edges.every((edge) => edge !== undefined)
    ? [edges[0]!.from, ...edges.map((edge) => edge!.to)] : flow.screens.map((screen) => screen.id);
  if (flow.title !== contract.title || edgeIds === undefined || stepIds.length !== contract.steps.length
    || stepIds.some((id, index) => id !== contract.steps[index]?.stepId)
    || flow.findings.some((finding) => finding.code === "flow.contract_stale" && finding.pointer === `/journeys/${contract.contractId}.json`)) {
    return "Contract out of date, regenerate first";
  }
  const proof = flow.status === "approved" ? "replay" : run?.kind === "replay" ? "replay" : "preview";
  if (run?.kind !== proof || !runProves(flow, contract, run)) return `Not run yet — complete every step in a ${proof} first`;
  return null;
}

/** #1653: a screen whose expectations are all process output (exit, stdout, stderr) is a backend
 * step: its run captures no frame. Every other screen is a browser screen and needs one. */
const PROCESS_ROLES = new Set(["exit", "stdout", "stderr"]);
export function stepNeedsFrame(screen: { expect?: ReadonlyArray<{ role: string }> } | undefined): boolean {
  return !(screen?.expect?.length && screen.expect.every((item) => PROCESS_ROLES.has(item.role)));
}

/** #1653: the one check the Proof list and Approve share. A run proves a contract when it is ready
 * and passing, every step passed (with a frame for each browser step), and every edge of the
 * contract's path passed. A record that says `pass` with no step results proves nothing. */
export function runProves(flow: JourneyFlowView, contract: { contractId: string; steps: readonly { stepId: string }[] }, run: JourneyRunView | null): boolean {
  const path = contract.contractId === flow.id ? "main" : contract.contractId.slice(flow.id.length + 1);
  const edgeIds = flow.paths[path] ?? [];
  return run?.state === "ready" && run.result === "pass" && contract.steps.length > 0
    && contract.steps.every((step) => run.screens?.[step.stepId]?.result === "pass"
      && (!stepNeedsFrame(flow.screens.find((screen) => screen.id === step.stepId)) || run.screens[step.stepId]?.frame === true))
    && edgeIds.every((id) => run.edges?.[id]?.result === "pass");
}

export const SCREEN_CAPTURE_KIND = "jpd.screen_captured";
export const SCREEN_CAPTURE_PROTOCOL = "graphhelm-screen-capture-v1";
export const TRANSITION_PROTOCOL = "graphhelm-transition-walked-v1";

export interface JourneySummary { proven: number; stale: number; other: number; total: number }

/** Proven = capture fresh and not dirty; stale = capture stale; everything else is other. */
export function journeySummary(view: JourneyView): JourneySummary {
  let proven = 0;
  let stale = 0;
  let other = 0;
  for (const step of view.steps) {
    const capture = step.capture;
    if (capture?.freshness === "fresh" && !capture.dirty) proven += 1;
    else if (capture?.freshness === "stale") stale += 1;
    else other += 1;
  }
  return { proven, stale, other, total: view.steps.length };
}

export interface CaptureDocument {
  sequence: number;
  signalId: string | null;
  imageEvidenceId: string;
  contractId: string;
  stepId: string;
  revision: string;
  dirty: boolean;
  observer: string;
  actorId: string | null;
  pr?: number;
  phase?: "before" | "after" | "live";
  occurredAt: string | null;
}

function nonEmpty(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

/** Capture signals whose opened envelope is a valid screen-capture document; the rest are ignored. */
export function captureDocuments(events: RuntimeEvent[], envelopes: EnvelopeRecord): CaptureDocument[] {
  const out: CaptureDocument[] = [];
  for (const event of events) {
    if (event.kind !== "signal_recorded") continue;
    const payload = event.payload !== null && typeof event.payload === "object" ? event.payload as Record<string, unknown> : {};
    if (payload.kind !== SCREEN_CAPTURE_KIND) continue;
    const text = envelopes[event.sequence]?.text;
    const image = event.evidenceRefs[1];
    if (typeof text !== "string" || !nonEmpty(image)) continue;
    let doc: unknown;
    try { doc = JSON.parse(text); } catch { continue; }
    if (doc === null || typeof doc !== "object" || Array.isArray(doc)) continue;
    const d = doc as Record<string, unknown>;
    if (d.protocol !== SCREEN_CAPTURE_PROTOCOL || !nonEmpty(d.contractId) || !nonEmpty(d.stepId) || !nonEmpty(d.revision)
      || typeof d.dirty !== "boolean" || !nonEmpty(d.observer)) continue;
    if (d.pr !== undefined && !(typeof d.pr === "number" && Number.isSafeInteger(d.pr) && d.pr >= 1)) continue;
    // `live` (#409): a live session's capture at a step (phase C); kept, never a before/after pair.
    if (d.phase !== undefined && d.phase !== "before" && d.phase !== "after" && d.phase !== "live") continue;
    const ref = event.evidenceRefs[0];
    const signalId = nonEmpty(payload.signalId) ? payload.signalId : nonEmpty(ref) && ref.startsWith("signal-") ? ref.slice("signal-".length) : null;
    out.push({
      sequence: event.sequence, signalId, imageEvidenceId: image, contractId: d.contractId, stepId: d.stepId,
      revision: d.revision, dirty: d.dirty, observer: d.observer, actorId: event.actorId,
      ...(d.pr === undefined ? {} : { pr: d.pr }), ...(d.phase === undefined ? {} : { phase: d.phase }),
      occurredAt: event.occurredAt,
    });
  }
  return out;
}

export interface BeforeAfterPair {
  contractId: string;
  stepId: string;
  pr: number;
  before: CaptureDocument;
  after: CaptureDocument;
  observer: string;
  actorId: string | null;
}

/** Newest before and newest after per (contractId, stepId, pr); pr required. Newest pair first. */
export function beforeAfterPairs(captures: CaptureDocument[]): BeforeAfterPair[] {
  const newest = new Map<string, { before?: CaptureDocument; after?: CaptureDocument; contractId: string; stepId: string; pr: number }>();
  for (const capture of captures) {
    if (capture.pr === undefined || (capture.phase !== "before" && capture.phase !== "after")) continue;
    const key = JSON.stringify([capture.contractId, capture.stepId, capture.pr]);
    const slot = newest.get(key) ?? { contractId: capture.contractId, stepId: capture.stepId, pr: capture.pr };
    const held = slot[capture.phase];
    if (held === undefined || capture.sequence > held.sequence) slot[capture.phase] = capture;
    newest.set(key, slot);
  }
  const pairs: BeforeAfterPair[] = [];
  for (const slot of newest.values()) {
    if (slot.before === undefined || slot.after === undefined) continue;
    pairs.push({ contractId: slot.contractId, stepId: slot.stepId, pr: slot.pr, before: slot.before, after: slot.after,
      observer: slot.after.observer, actorId: slot.after.actorId });
  }
  return pairs.sort((a, b) => Math.max(b.before.sequence, b.after.sequence) - Math.max(a.before.sequence, a.after.sequence));
}

/** #447: a flow with a second path compiles to `<flow>.<path>`. When `<flow>` is itself a journey
 * here, the contract is that flow's `path` path; anything else (a dotted id alone) is its own row. */
export function journeyPath(contractIds: readonly string[], contractId: string): { flow: string; path: string } | null {
  const dot = contractId.lastIndexOf(".");
  if (dot <= 0 || dot === contractId.length - 1) return null;
  const flow = contractId.slice(0, dot);
  return contractIds.includes(flow) ? { flow, path: contractId.slice(dot + 1) } : null;
}
