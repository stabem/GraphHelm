/**
 * Reading the customs pipeline's answers off the wire, as pure functions.
 *
 * THE VERDICT IS NOT THE HTTP STATUS, and that is the whole reason this file exists. A claim the
 * Runtime refuses is a 200 whose refusal is a JOURNAL EVENT with a registry code; a clearance the
 * fold rejects is a 200 whose rejection is an entry in the projection. A caller that read
 * `MutationEvidence.result` would be told "succeeded" for both — correctly, because the mutation
 * DID land — and would then show a person that their claim went through when the journal says it
 * did not. Every function here reads the decision from where the decision actually lives.
 *
 * Nothing here fetches, and nothing here throws: a shape that does not match answers `unknown`,
 * which a surface must render as "we could not tell" and never as either verdict.
 */
import type { ExecutionStatus, MutationEvidence } from "./types";

/** The verdict on a completion claim. `claimSeq` is the CLAIM's own envelope sequence, which is
 * what a later clearance names — not the wait's. */
export type ClaimVerdict =
  | { outcome: "claimed"; claimSeq: number }
  | { outcome: "refused"; reasonCode: string | null }
  | { outcome: "unknown" };

/** The verdict on a clearance. */
export type ClearVerdict =
  | { outcome: "cleared" }
  | { outcome: "refused"; reasonCode: string | null }
  | { outcome: "unknown" };

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function stringField(source: Record<string, unknown> | null, field: string): string | null {
  const value = source?.[field];
  return typeof value === "string" && value.length > 0 ? value : null;
}

/**
 * The sequence of the wait this node is holding open, or `null` when it holds none.
 *
 * THE SEQUENCE IS THE WAIT'S IDENTITY. A node re-parks, so its NAME identifies the node and never
 * the wait; a claim that names a superseded sequence is refused as a stale rendezvous instead of
 * silently answering whichever wait is open now. A caller that has a status in hand always has
 * this number, so there is no honest reason to omit it and let the Runtime choose.
 *
 * Read from `customs.nodes.<node>.openWait.atSequence` — the map the fold clears the instant the
 * wait closes, so an absent entry means "not waiting" rather than "we did not look".
 */
export function openWaitSequence(status: ExecutionStatus | null, node: string): number | null {
  const nodes = record(record(status?.customs)?.nodes);
  const wait = record(record(nodes?.[node])?.openWait);
  const sequence = wait?.atSequence;
  return typeof sequence === "number" && Number.isSafeInteger(sequence) && sequence >= 0
    ? sequence
    : null;
}

/**
 * What the Runtime decided about a claim, read from the event it appended.
 *
 * ONE event per request by construction — the route appends `completion_claimed` OR
 * `completion_refused` and never both — so finding either is the decision. The claim's sequence is
 * the ENVELOPE's, not a field: the journal assigns it, which is exactly why a caller cannot mint
 * or predict it.
 */
export function claimVerdict(evidence: MutationEvidence): ClaimVerdict {
  for (const event of evidence.newEvents) {
    if (event.kind === "completion_claimed") {
      return { outcome: "claimed", claimSeq: event.sequence };
    }
    if (event.kind === "completion_refused") {
      return { outcome: "refused", reasonCode: stringField(record(event.payload), "reasonCode") };
    }
  }
  return { outcome: "unknown" };
}

/**
 * What the FOLD decided about a clearance, read from the projection it produced.
 *
 * NOT FROM THE EVENT, and the difference is load-bearing: `completion_cleared` is appended
 * whichever way the verification went — the countersignature is checked during the replay, not by
 * the door — so the event's presence says a clearance was ATTEMPTED and nothing more. The verdict
 * lands in `customs.clearances`, keyed by the claim's sequence, which is why this function needs
 * the claim sequence the caller is asking about and cannot infer it from the newest entry.
 */
export function clearVerdict(evidence: MutationEvidence, claimSeq: number): ClearVerdict {
  const clearances = record(record(evidence.statusAfter?.customs)?.clearances);
  const verdict = record(clearances?.[String(claimSeq)]);
  const type = stringField(verdict, "type");
  if (type === "cleared") return { outcome: "cleared" };
  if (type === "refused") {
    return { outcome: "refused", reasonCode: stringField(verdict, "reasonCode") };
  }
  return { outcome: "unknown" };
}

/**
 * `sha256:<64 lowercase hex>` for one artefact, computed where the artefact already is.
 *
 * THE BYTES DO NOT LEAVE. A claim presents a digest and a size, never the file, so hashing in the
 * browser is not an optimisation — it is what lets a person point at a test report on their own
 * machine without uploading it anywhere.
 */
export async function digestOf(bytes: ArrayBuffer, subtle: SubtleCrypto): Promise<string> {
  const hashed = await subtle.digest("SHA-256", bytes);
  const hex = Array.from(new Uint8Array(hashed))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  return `sha256:${hex}`;
}
