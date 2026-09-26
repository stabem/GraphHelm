/**
 * The ledger of unanswered questions, extracted so BOTH surfaces read the same debt.
 *
 * The banner quoting "start asks: which region?" and an agent calling `graphhelm_get_attention`
 * must never disagree about who is owed what - that is the same class of drift the adapter's
 * header forbids between buttons and tools. So the derivation lives here once, pure over the
 * event page and the opened envelopes, and the two callers (`App`'s attention block, the WebMCP
 * adapter's attention tool) both call THIS.
 *
 * WHAT A DEBT IS. An agent's `signal_recorded` addressed to the operator by envelope. Derived,
 * never guessed - an unaddressed room message is not a question, because quoting the WRONG
 * message is worse than quoting none. Two settlement rules, both about honesty:
 * - A debt to the OPERATOR dies only when the OPERATOR answers. An author-blind answered-set let
 *   another agent's side-reply erase a question the thread still showed unanswered.
 * - A reply to the operator's OWN signal is a return receipt, not a new question: quoting a
 *   delivered report as "X asked you" would invert a settled exchange into a fresh demand.
 */

import type { RuntimeEvent } from "../runtime/types";

/**
 * The sentence inside sealed content, when there is one to read.
 *
 * A signal's sealed evidence is the full envelope document - id, source, severity, `emittedAt`
 * and all - so rendering the sealed bytes verbatim puts a wall of JSON where somebody's sentence
 * should be. Anything this cannot read a sentence out of is returned UNCHANGED: a model's reply
 * and a tool's output are sealed here too. A ModelReply has the public `text` plus
 * `usage` shape, so its words can be shown without guessing from arbitrary JSON.
 */
export function readable_content(text: string, mediaType: string): string {
  if (!mediaType.includes("json")) return text;
  try {
    const parsed: unknown = JSON.parse(text);
    if (parsed === null || typeof parsed !== "object") return text;
    const modelReply = parsed as { text?: unknown; usage?: unknown };
    if (typeof modelReply.text === "string" && modelReply.usage !== null
        && typeof modelReply.usage === "object" && !Array.isArray(modelReply.usage)) {
      return modelReply.text;
    }
    const description = (parsed as { description?: unknown }).description;
    const type = (parsed as { type?: unknown }).type;
    if (typeof description === "string" && type === "node_delivery") {
      const delivery: unknown = JSON.parse(description);
      if (delivery && typeof delivery === "object" && "summary" in delivery && "reason" in delivery
          && typeof delivery.summary === "string" && typeof delivery.reason === "string") {
        return `${delivery.summary}\n\n${delivery.reason}`;
      }
    }
    if (typeof description === "string" && ["owner_document_changed", "owner_document_edit_saved", "owner_document_edit_intent"].includes(String(type))) {
      const record = JSON.parse(description) as {path?: unknown; reason?: unknown; edit?: {path?: unknown; reason?: unknown}};
      const edit = record.edit ?? record;
      if (typeof edit.path === "string" && typeof edit.reason === "string") {
        return `${type === "owner_document_edit_intent" ? "Owner requested a project edit" : "Owner changed a project document"}: ${edit.path}\n\n${edit.reason}`;
      }
    }
    return typeof description === "string" && description.length > 0 ? description : text;
  } catch {
    // Sealed content that claims to be JSON and is not still has to reach the reader.
    return text;
  }
}

/** The envelope's address, when it carries one (schema 1.1.0). A group chat where the addressing
 * is invisible is unthreadable by eye - the fields are already in the sealed envelope; this only
 * reads them out. Non-JSON or address-free content yields nothing, never a guess. */
export function address_of(text: string, mediaType: string): { to: string | null; replyTo: string | null } {
  const none = { to: null, replyTo: null };
  if (!mediaType.includes("json")) return none;
  try {
    const parsed: unknown = JSON.parse(text);
    if (parsed === null || typeof parsed !== "object") return none;
    const envelope = parsed as { to?: unknown; replyTo?: unknown };
    return {
      to: typeof envelope.to === "string" && envelope.to.length > 0 ? envelope.to : null,
      replyTo:
        typeof envelope.replyTo === "string" && envelope.replyTo.length > 0
          ? envelope.replyTo
          : null,
    };
  } catch {
    return none;
  }
}

/** The envelope a signal's sealed evidence carried, keyed by the signal event's sequence. */
export type EnvelopeRecord = Record<number, { to: string | null; replyTo: string | null; text: string }>;

export interface OpenQuestion {
  /** The agent that asked. */
  asker: string;
  at: string | null;
  /** The question itself, verbatim from the sealed envelope. */
  text: string;
  /** The signal id an answer's `replyTo` must cite to settle this debt. */
  signalId: string | null;
}

function signalIdOf(event: RuntimeEvent): string | null {
  const payload =
    event.payload !== null && typeof event.payload === "object"
      ? (event.payload as { signalId?: unknown })
      : {};
  return typeof payload.signalId === "string" ? payload.signalId : null;
}

/** Every question still owed to `operatorId`, newest first. */
export function openQuestions(
  events: RuntimeEvent[],
  envelopes: EnvelopeRecord,
  operatorId: string,
): OpenQuestion[] {
  const operatorSignalIds = new Set<string>();
  const answeredIds = new Set<string>();
  for (const event of events) {
    if (event.kind !== "signal_recorded" || event.actorType !== "owner") continue;
    const signalId = signalIdOf(event);
    if (signalId !== null) operatorSignalIds.add(signalId);
    const replyTo = envelopes[event.sequence]?.replyTo ?? null;
    if (replyTo !== null) answeredIds.add(replyTo);
  }
  const owed: OpenQuestion[] = [];
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (event.kind !== "signal_recorded") continue;
    if (event.actorType !== "agent" || event.actorId === null) continue;
    const envelope = envelopes[event.sequence];
    if (envelope === undefined || envelope.to !== operatorId) continue;
    if (envelope.replyTo !== null && operatorSignalIds.has(envelope.replyTo)) continue;
    const signalId = signalIdOf(event);
    if (signalId !== null && answeredIds.has(signalId)) continue;
    owed.push({ asker: event.actorId, at: event.occurredAt, text: envelope.text, signalId });
  }
  return owed;
}
