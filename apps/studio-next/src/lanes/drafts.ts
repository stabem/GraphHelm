/** #1837: one owner request to a lane, built once and resent unchanged. The Runtime recognizes a
 * retry only when the `Idempotency-Key` AND the body match what it recorded, so a resend must
 * carry the first attempt's id, `emittedAt` / `at` and every other byte. A new id is drawn only
 * when the owner asks for something else (another text, lane, target or question). */
export interface Draft {
  /** The `Idempotency-Key` (and, for a message, the signal id). */
  id: string;
  /** What the owner asked for; a different intent is a different request. */
  intent: string;
  body: Record<string, unknown>;
}

/** The pending draft when it carries the same intent, else a new one from `make`. */
export function draftFor(prior: Draft | undefined, intent: string, make: () => { id: string; body: Record<string, unknown> }): Draft {
  if (prior !== undefined && prior.intent === intent) return prior;
  return { intent, ...make() };
}

/** The `POST /v1/executions/{id}/signal` body of an owner message to a lane: an `operator_note`
 * addressed to it, answering `replyTo` when there is one. */
export function messageBody(executionId: string, id: string, lane: string, text: string, replyTo: string | null, now: Date, owner: string): Record<string, unknown> {
  return {
    signal: {
      id, to: lane, ...(replyTo === null ? {} : { replyTo }),
      source: { type: "user", id: owner }, type: "operator_note", severity: "low",
      description: text, evidence: [executionId], emittedAt: now.toISOString(),
    },
  };
}

export type LaneControl = "wake" | "reassign" | "stop";

/** The body of `POST /v1/executions/{id}/lanes/{lane}/{control}` (#1847). `at` is sent so a retry
 * resends the same instant and the Runtime's records stay byte-identical. */
export function controlBody(control: LaneControl, reason: string, now: Date, reassign?: { sliceKey: string; to: string }): Record<string, unknown> {
  const at = now.toISOString().replace(/\.\d{3}Z$/, "Z");
  if (control === "reassign") {
    if (reassign === undefined) throw new Error("reassign needs a slice and a target lane");
    return { sliceKey: reassign.sliceKey, to: reassign.to, reason, at };
  }
  return control === "stop" ? { reason, retired: true, at } : { reason, at };
}

/** A new idempotency key: `randomUUID` where the browser has it, else 128 random bits. */
export function newId(prefix: string): string {
  const uuid = globalThis.crypto?.randomUUID?.();
  if (uuid) return `${prefix}-${uuid}`;
  const bytes = globalThis.crypto.getRandomValues(new Uint8Array(16));
  return `${prefix}-${[...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}
