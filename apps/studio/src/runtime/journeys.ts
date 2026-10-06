/**
 * Pure derivations over the journeys route and the event tail (phase 5, rulings 1, 2, 4).
 * Nothing here reads a file or the clock: a missing event means "unknown", never a guess.
 */
import type { EnvelopeRecord } from "../graph/ledger";
import { timeOf } from "./team";
import type { JourneyView, RuntimeEvent } from "./types";

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

/** The capture's time in ms from the event at that sequence, or null when the tail lacks it. */
export function captureAge(sequence: number, events: RuntimeEvent[]): number | null {
  const event = events.find((candidate) => candidate.sequence === sequence);
  return event === undefined ? null : timeOf(event.occurredAt);
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
  phase?: "before" | "after";
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
    if (d.phase !== undefined && d.phase !== "before" && d.phase !== "after") continue;
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
