/**
 * The live team, derived from records only (spec §4.1).
 *
 * A bot is a persona, an aliased actor, or an actor with a record in the last two hours; every
 * other recorder is folded into one line so a long run does not read as "98 agents". State and
 * tasks come from records, never from note text, and silence is reported as an age, never as
 * "stuck".
 */
import { hueOf } from "../components/format";
import type { EnvelopeRecord } from "../graph/ledger";
import type { GraphModel } from "../graph/model";
import type { ClaudeTaskReadModel } from "./team-tasks";
import type { NativeChatSummary, RuntimeEvent } from "./types";

export const SHARED_CODEX_ACTOR = "codex";
export const BOT_RECENT_MS = 2 * 60 * 60 * 1000;
export const WORKING_MS = 15 * 60 * 1000;
export const LIVE_LINK_MS = 60 * 1000;
const OPERATOR = "studio-operator";
const SETTLED = new Set(["succeeded", "waived", "skipped"]);
const OPENING = "Opening…";
const COORDINATOR = /coordinat|orchestrat|\blead\b/i;

export type BotState = "working" | "waiting_for_you" | "quiet" | "done";
export interface BotTask { id: string; title: string; done: boolean; source: "task_record" | "graph_node"; nodeId: string | null; sequence: number; doneSequence: number | null }
export interface Bot { key: string; actorId: string | null; name: string; hue: number; role: string | null; doingNow: string; lastRecordAt: string | null; lastSequence: number; state: BotState; quietMinutes: number | null; shared: boolean; native: boolean; tasks: BotTask[] }
export interface OtherRecorder { actorId: string; count: number; lastRecordAt: string | null }
export interface TeamModel { bots: Bot[]; otherRecorders: OtherRecorder[] }
export interface TeamLink { a: string; b: string; count: number; lastAt: string | null; live: boolean }
export interface TeamInput {
  events: RuntimeEvent[];
  envelopes: EnvelopeRecord;
  personas: Record<string, string>;
  nativeLinks: Record<string, { chat: NativeChatSummary; charter: string; nodeId: string }>;
  aliases: Record<string, string>;
  model: GraphModel | null;
  claudeTasks: ClaudeTaskReadModel | null;
  waitingAskers: ReadonlySet<string>;
  now: number;
}

function signalKind(event: RuntimeEvent): string | null {
  const payload = event.payload;
  if (payload === null || typeof payload !== "object" || Array.isArray(payload)) return null;
  const kind = (payload as Record<string, unknown>).kind;
  return typeof kind === "string" ? kind : null;
}

export function timeOf(at: string | null): number | null {
  if (at === null) return null;
  const value = Date.parse(at);
  return Number.isNaN(value) ? null : value;
}

export function firstLine(text: string, max = 120): string {
  const line = text.split(/\r?\n/).find((candidate) => candidate.trim().length > 0)?.trim() ?? "";
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}

interface Seed { actorId: string | null; native: boolean; charter: string | null; title: string | null }
interface Tally { count: number; lastAt: string | null; lastSequence: number }

export function teamModel(input: TeamInput): TeamModel {
  const tallies = new Map<string, Tally>();
  const notes = new Map<string, string>();
  for (const event of input.events) {
    if (event.kind !== "signal_recorded" || event.actorType !== "agent" || event.actorId === null) continue;
    const tally = tallies.get(event.actorId) ?? { count: 0, lastAt: null, lastSequence: 0 };
    tally.count += 1;
    if (event.sequence >= tally.lastSequence) {
      tally.lastSequence = event.sequence;
      tally.lastAt = event.occurredAt;
    }
    tallies.set(event.actorId, tally);
    if (signalKind(event) === "operator_note") {
      // #446: a sealed note not opened yet exists; it reads as opening, never as "no note".
      const envelope = input.envelopes[event.sequence];
      const text = envelope?.text?.trim();
      if (text) notes.set(event.actorId, firstLine(text));
      else if (envelope === undefined && event.evidenceRefs.length > 0) notes.set(event.actorId, OPENING);
    }
  }

  const seeds = new Map<string, Seed>();
  for (const [id, charter] of Object.entries(input.personas)) {
    if (id !== OPERATOR) seeds.set(id, { actorId: id, native: false, charter: charter || null, title: null });
  }
  for (const [thread, link] of Object.entries(input.nativeLinks)) {
    if (!seeds.has(thread)) seeds.set(thread, { actorId: null, native: true, charter: link.charter || null, title: link.chat.title });
  }
  for (const id of Object.keys(input.aliases)) {
    if (!seeds.has(id) && id !== SHARED_CODEX_ACTOR && id !== OPERATOR) seeds.set(id, { actorId: id, native: false, charter: null, title: null });
  }
  const otherRecorders: OtherRecorder[] = [];
  for (const [id, tally] of tallies) {
    if (seeds.has(id)) continue;
    const at = timeOf(tally.lastAt);
    if (at !== null && input.now - at <= BOT_RECENT_MS) {
      seeds.set(id, { actorId: id, native: false, charter: null, title: null });
    } else {
      otherRecorders.push({ actorId: id, count: tally.count, lastRecordAt: tally.lastAt });
    }
  }

  const nodes = input.model?.nodes ?? [];
  const bots: Bot[] = [...seeds.entries()].map(([key, seed]) => {
    const tally = seed.actorId === null ? undefined : tallies.get(seed.actorId);
    const tasks: BotTask[] = [];
    for (const task of input.claudeTasks?.tasks ?? []) {
      if (seed.actorId === null || task.sourceId !== seed.actorId) continue;
      tasks.push({ id: task.nativeTaskId, title: task.taskSubject, done: task.completedSequence !== null, source: "task_record",
        nodeId: null, sequence: task.createdSequence ?? task.completedSequence ?? 0, doneSequence: task.completedSequence });
    }
    const nativeNode = seed.native ? input.nativeLinks[key]?.nodeId ?? null : null;
    for (const node of nodes) {
      const mine = (seed.actorId !== null && node.assignedActor?.id === seed.actorId) || node.id === nativeNode;
      if (!mine) continue;
      const last = node.history.at(-1)?.sequence ?? 0;
      const done = SETTLED.has(node.state);
      tasks.push({ id: `node:${node.id}`, title: node.declaredName ?? node.id, done, source: "graph_node", nodeId: node.id, sequence: last, doneSequence: done ? last : null });
    }
    tasks.sort((a, b) => a.sequence - b.sequence);

    const lastSequence = tally?.lastSequence ?? 0;
    const lastAt = timeOf(tally?.lastAt ?? null);
    const quietMinutes = lastAt === null ? null : Math.floor((input.now - lastAt) / 60_000);
    const waitingNode = tasks.some((task) => task.nodeId !== null && nodes.find((node) => node.id === task.nodeId)?.state === "waiting_input");
    const newest = tasks.at(-1);
    let state: BotState;
    if ((seed.actorId !== null && input.waitingAskers.has(seed.actorId)) || waitingNode) state = "waiting_for_you";
    else if (newest?.done === true && newest.doneSequence !== null && lastSequence <= newest.doneSequence) state = "done";
    else if (lastAt !== null && input.now - lastAt < WORKING_MS) state = "working";
    else state = "quiet";

    const shared = seed.actorId === SHARED_CODEX_ACTOR;
    const name = input.aliases[key] ?? seed.title ?? (shared ? "Codex (shared)" : key);
    return {
      key, actorId: seed.actorId, name, hue: hueOf(key), role: seed.charter === null ? null : firstLine(seed.charter) || null,
      doingNow: seed.actorId !== null ? notes.get(seed.actorId) ?? "No note yet" : "No note yet",
      lastRecordAt: tally?.lastAt ?? null, lastSequence, state, quietMinutes, shared, native: seed.native, tasks,
    };
  });

  bots.sort((a, b) => {
    const lead = Number(COORDINATOR.test(`${b.name} ${b.role ?? ""}`)) - Number(COORDINATOR.test(`${a.name} ${a.role ?? ""}`));
    return lead !== 0 ? lead : a.name.localeCompare(b.name);
  });
  otherRecorders.sort((a, b) => a.actorId.localeCompare(b.actorId, undefined, { numeric: true }));
  return { bots, otherRecorders };
}

/** The bot an actor id or thread id belongs to, or null when it belongs to none. */
export function botKeyOf(bots: Bot[], id: string | null): string | null {
  if (id === null) return null;
  return bots.find((bot) => bot.key === id || bot.actorId === id)?.key ?? null;
}

/** One line per pair of bots that addressed each other (the `talks` pair rule in App.tsx). */
export function teamLinks(events: RuntimeEvent[], envelopes: EnvelopeRecord, bots: Bot[], now: number): TeamLink[] {
  const links = new Map<string, TeamLink>();
  for (const event of events) {
    if (event.kind !== "signal_recorded" || (event.actorType !== "agent" && event.actorType !== "owner")) continue;
    const to = envelopes[event.sequence]?.to ?? null;
    if (event.actorId === OPERATOR || to === OPERATOR) continue;
    const from = botKeyOf(bots, event.actorId);
    const target = botKeyOf(bots, to);
    if (from === null || target === null || from === target) continue;
    const [a, b] = [from, target].sort();
    const link = links.get(`${a}\u0000${b}`) ?? { a, b, count: 0, lastAt: null, live: false };
    link.count += 1;
    if (link.lastAt === null || (timeOf(event.occurredAt) ?? 0) >= (timeOf(link.lastAt) ?? 0)) link.lastAt = event.occurredAt;
    links.set(`${a}\u0000${b}`, link);
  }
  return [...links.values()]
    .map((link) => {
      const at = timeOf(link.lastAt);
      return { ...link, live: at !== null && now - at <= LIVE_LINK_MS };
    })
    .sort((x, y) => x.a.localeCompare(y.a) || x.b.localeCompare(y.b));
}
