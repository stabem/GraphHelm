/**
 * Chat threads for the Chat column: Everyone, one thread per task (#393, journey-first spec §8) and
 * the owner's direct line with one agent. Every message is RECORDED through the Runtime.
 */
import type { EnvelopeRecord } from "../graph/ledger";
import { SCREEN_CAPTURE_PROTOCOL, TRANSITION_PROTOCOL } from "./journeys";
import { firstLine, type Bot } from "./team";

export const EVERYONE = "everyone";

export function namesOf(bots: Bot[]): Record<string, string> {
  const names: Record<string, string> = {};
  for (const bot of bots) {
    names[bot.key] = bot.name;
    if (bot.actorId !== null) names[bot.actorId] = bot.name;
  }
  return names;
}

export function parseMention(text: string, bots: Bot[]): { to: string | null; text: string } {
  const lowered = text.toLowerCase();
  for (const bot of [...bots].sort((a, b) => b.name.length - a.name.length)) {
    const prefix = `@${bot.name.toLowerCase()} `;
    if (lowered.startsWith(prefix)) return { to: bot.actorId ?? bot.key, text: text.slice(prefix.length).trim() };
  }
  return { to: null, text };
}

export interface ActivityItem { sequence: number; actorId: string | null; occurredAt: string | null; text: string | null }
export interface ActivityLine { sequence: number; text: string; at: string | null }

/** A journey record (spec 6.2) said in words: its envelope text is a JSON document, never prose. */
function journeyWords(text: string, stepTitle: (contractId: string, stepId: string) => string): string | null {
  if (!text.startsWith("{")) return null;
  let doc: unknown;
  try { doc = JSON.parse(text); } catch { return null; }
  if (doc === null || typeof doc !== "object" || Array.isArray(doc)) return null;
  const d = doc as Record<string, unknown>;
  const str = (value: unknown) => typeof value === "string" && value.length > 0 ? value : null;
  const contract = str(d.contractId);
  if (contract === null) return null;
  if (d.protocol === SCREEN_CAPTURE_PROTOCOL && str(d.stepId) !== null) return `captured ${stepTitle(contract, d.stepId as string)}`;
  if (d.protocol === TRANSITION_PROTOCOL && str(d.fromStepId) !== null && str(d.toStepId) !== null) {
    return `walked ${stepTitle(contract, d.fromStepId as string)} → ${stepTitle(contract, d.toStepId as string)}`;
  }
  return null;
}

function documentOf(text: string): Record<string, unknown> | null {
  try {
    const doc: unknown = JSON.parse(text);
    return doc !== null && typeof doc === "object" && !Array.isArray(doc) ? doc as Record<string, unknown> : null;
  } catch { return null; }
}

/** #442: the owner's own records (spec #301 section 5.2, 5.3) said in words. */
function ownerRecordWords(text: string, envelope: EnvelopeRecord[number] | undefined, names: Record<string, string>): string | null {
  const doc = text.trimStart().startsWith("{") ? documentOf(text) : null;
  if (doc === null) return null;
  const to = envelope?.to ?? null;
  if (doc.protocol === "graphhelm-actor-alias-v1" && typeof doc.displayName === "string" && to !== null) {
    return `named ${to} “${firstLine(doc.displayName, 80)}”`;
  }
  if (doc.protocol === "graphhelm-owner-refusal-v1") return to !== null ? `refused a question from ${names[to] ?? to}` : "refused a question";
  return null;
}

function documentKind(text: string): string {
  const doc = documentOf(text);
  const kind = doc === null ? null : [doc.protocol, doc.schema].find((value) => typeof value === "string" && /^[a-z0-9.-]{1,64}$/.test(value));
  return typeof kind === "string" ? `a ${kind} record` : "a record";
}

export function describeActivity(item: ActivityItem, names: Record<string, string>, envelopes: EnvelopeRecord, operatorId: string,
  stepTitle: (contractId: string, stepId: string) => string = (_contractId, stepId) => stepId): ActivityLine {
  const who = item.actorId === operatorId ? "You" : names[item.actorId ?? ""] ?? item.actorId ?? "Someone";
  const journey = journeyWords(item.text ?? "", stepTitle);
  if (journey !== null) return { sequence: item.sequence, text: `${who} ${journey}`, at: item.occurredAt };
  const owner = ownerRecordWords(item.text ?? "", envelopes[item.sequence], names);
  if (owner !== null) return { sequence: item.sequence, text: `${who} ${owner}`, at: item.occurredAt };
  // #442: any other document (or one cut short) is named by its kind, never printed as JSON.
  if ((item.text ?? "").trimStart().startsWith("{")) {
    return { sequence: item.sequence, text: `${who} recorded ${documentKind(item.text ?? "")}`, at: item.occurredAt };
  }
  const words = firstLine(item.text ?? "", 80);
  const to = envelopes[item.sequence]?.to ?? null;
  const text = words === ""
    ? `${who} recorded a sealed note`
    : to === operatorId ? `${who} asked you “${words}”`
    : to !== null ? `${who} told ${names[to] ?? to} “${words}”`
    : `${who} said “${words}”`;
  return { sequence: item.sequence, text, at: item.occurredAt };
}
