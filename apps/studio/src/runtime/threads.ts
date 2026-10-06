/**
 * Chat threads for the Chat column (spec §4.4). Pair threads are RECORDED messages: what two
 * agents recorded to each other through the Runtime, not native chats talking directly.
 */
import type { EnvelopeRecord } from "../graph/ledger";
import { SCREEN_CAPTURE_PROTOCOL, TRANSITION_PROTOCOL } from "./journeys";
import { botKeyOf, firstLine, type Bot } from "./team";
import type { RuntimeEvent } from "./types";
import type { WorkMessage } from "./work-conversation";

export type ThreadKind = "everyone" | "pair" | "direct";
export interface ChatThread { key: string; kind: ThreadKind; label: string; participants: string[]; messages: WorkMessage[] }
export const EVERYONE = "everyone";

export function namesOf(bots: Bot[]): Record<string, string> {
  const names: Record<string, string> = {};
  for (const bot of bots) {
    names[bot.key] = bot.name;
    if (bot.actorId !== null) names[bot.actorId] = bot.name;
  }
  return names;
}

export function chatThreads(messages: WorkMessage[], bots: Bot[], operatorId: string): ChatThread[] {
  const names = namesOf(bots);
  const label = (id: string) => names[id] ?? id;
  const everyone: ChatThread = { key: EVERYONE, kind: "everyone", label: "Everyone", participants: [], messages: [] };
  const pairs = new Map<string, ChatThread>();
  const directs = new Map<string, ChatThread>();
  for (const message of messages) {
    if (message.to === null) { everyone.messages.push(message); continue; }
    const fromOwner = message.sender === operatorId;
    const toOwner = message.to === operatorId;
    if (fromOwner || toOwner) {
      const other = fromOwner ? message.to : message.sender;
      const key = botKeyOf(bots, other) ?? other;
      const thread = directs.get(key) ?? { key: `direct:${key}`, kind: "direct" as const, label: label(key), participants: [key], messages: [] };
      thread.messages.push(message);
      directs.set(key, thread);
      continue;
    }
    const a = botKeyOf(bots, message.sender) ?? message.sender;
    const b = botKeyOf(bots, message.to) ?? message.to;
    const [first, second] = [a, b].sort();
    const key = `pair:${first}+${second}`;
    const thread = pairs.get(key) ?? { key, kind: "pair" as const, label: `${label(first)} ↔ ${label(second)}`, participants: [first, second], messages: [] };
    thread.messages.push(message);
    pairs.set(key, thread);
  }
  return [everyone, ...pairs.values(), ...directs.values()];
}

export function unreadCounts(threads: ChatThread[], lastOpened: Record<string, number>): Record<string, number> {
  return Object.fromEntries(threads.map((thread) => [thread.key, thread.messages.filter((message) => message.sequence > (lastOpened[thread.key] ?? 0)).length]));
}

export function parseMention(text: string, bots: Bot[]): { to: string | null; text: string } {
  const lowered = text.toLowerCase();
  for (const bot of [...bots].sort((a, b) => b.name.length - a.name.length)) {
    const prefix = `@${bot.name.toLowerCase()} `;
    if (lowered.startsWith(prefix)) return { to: bot.actorId ?? bot.key, text: text.slice(prefix.length).trim() };
  }
  return { to: null, text };
}

export function sealedNotesPending(events: RuntimeEvent[], envelopes: EnvelopeRecord): number {
  return events.filter((event) => event.kind === "signal_recorded" && event.evidenceRefs.length > 0
    && (event.payload as { kind?: unknown } | null)?.kind === "operator_note" && envelopes[event.sequence] === undefined).length;
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

export function describeActivity(item: ActivityItem, names: Record<string, string>, envelopes: EnvelopeRecord, operatorId: string,
  stepTitle: (contractId: string, stepId: string) => string = (_contractId, stepId) => stepId): ActivityLine {
  const who = item.actorId === operatorId ? "You" : names[item.actorId ?? ""] ?? item.actorId ?? "Someone";
  const journey = journeyWords(item.text ?? "", stepTitle);
  if (journey !== null) return { sequence: item.sequence, text: `${who} ${journey}`, at: item.occurredAt };
  const words = firstLine(item.text ?? "", 80);
  const to = envelopes[item.sequence]?.to ?? null;
  const text = words === ""
    ? `${who} recorded a sealed note`
    : to === operatorId ? `${who} asked you “${words}”`
    : to !== null ? `${who} told ${names[to] ?? to} “${words}”`
    : `${who} said “${words}”`;
  return { sequence: item.sequence, text, at: item.occurredAt };
}
