/**
 * "While you were away" (spec §4.5): what shipped, what needs you, what went quiet and what
 * nobody touched between the last sequence the owner saw and now. Every line cites the event
 * sequences it came from. Missing or unreadable storage means no card - never a wrong one.
 */
import type { GraphModel } from "../graph/model";
import type { NeedsYouItem } from "./needs-you";
import { firstLine, timeOf, type Bot } from "./team";
import type { ClaudeTaskReadModel } from "./team-tasks";
import type { RuntimeEvent } from "./types";

export const HANDOVER_MIN_GAP_MS = 15 * 60 * 1000;
export const HANDOVER_MIN_EVENTS = 20;
export const QUIET_IN_GAP_MS = 30 * 60 * 1000;
const SETTLED = new Set(["succeeded", "waived", "skipped"]);

export interface HandoverLine { text: string; sequences: number[] }
export interface Handover { fromSeq: number; toSeq: number; eventCount: number; gapMinutes: number; shipped: HandoverLine[]; needsYou: HandoverLine[]; quiet: HandoverLine[]; untouched: HandoverLine[] }
export interface HandoverInput { events: RuntimeEvent[]; bots: Bot[]; model: GraphModel | null; claudeTasks: ClaudeTaskReadModel | null; openItems: NeedsYouItem[]; fromSeq: number; toSeq: number }

function payloadOf(event: RuntimeEvent): Record<string, unknown> {
  return event.payload !== null && typeof event.payload === "object" && !Array.isArray(event.payload) ? event.payload as Record<string, unknown> : {};
}

function gapBounds(events: RuntimeEvent[], fromSeq: number, toSeq: number): { inGap: RuntimeEvent[]; fromTime: number | null; toTime: number | null } {
  const inGap = events.filter((event) => event.sequence > fromSeq && event.sequence <= toSeq);
  const seen = events.filter((event) => event.sequence <= fromSeq).at(-1);
  const fromTime = timeOf(seen?.occurredAt ?? inGap[0]?.occurredAt ?? null);
  const toTime = timeOf(inGap.at(-1)?.occurredAt ?? null);
  return { inGap, fromTime, toTime };
}

export function shouldShowHandover(events: RuntimeEvent[], fromSeq: number | null, toSeq: number): boolean {
  if (fromSeq === null || toSeq <= fromSeq) return false;
  const { inGap, fromTime, toTime } = gapBounds(events, fromSeq, toSeq);
  return inGap.length >= HANDOVER_MIN_EVENTS && fromTime !== null && toTime !== null && toTime - fromTime >= HANDOVER_MIN_GAP_MS;
}

export function buildHandover(input: HandoverInput): Handover {
  const { inGap, fromTime, toTime } = gapBounds(input.events, input.fromSeq, input.toSeq);
  const botName = (id: string | null) => input.bots.find((bot) => bot.key === id || bot.actorId === id)?.name ?? id ?? "Someone";
  const nodeName = (id: unknown) => {
    if (typeof id !== "string") return "A step";
    return input.model?.nodes.find((node) => node.id === id)?.declaredName ?? id;
  };

  const shipped: HandoverLine[] = [];
  for (const event of inGap) {
    const payload = payloadOf(event);
    if (event.kind === "node_outcome_recorded" && payload.outcome === "succeeded") shipped.push({ text: `${nodeName(payload.nodeId)} succeeded`, sequences: [event.sequence] });
    else if (event.kind === "completion_cleared") shipped.push({ text: `${nodeName(payload.nodeId)} was cleared as complete`, sequences: [event.sequence] });
    else if (event.kind === "signal_recorded" && payload.kind === "agent_task_completed") {
      const task = input.claudeTasks?.tasks.find((candidate) => candidate.completedSequence === event.sequence);
      shipped.push({ text: task ? `${botName(event.actorId)} finished “${task.taskSubject}”` : `${botName(event.actorId)} finished a task`, sequences: [event.sequence] });
    }
  }

  // Everything the beacon counts. Questions are placed in the gap by sequence; native requests,
  // steps and drafts carry none, so they are listed as open now (no citation) - the card and the
  // beacon must never disagree.
  const needsYou: HandoverLine[] = input.openItems.flatMap((item): HandoverLine[] => {
    switch (item.kind) {
      case "question":
        return item.sequence > input.fromSeq && item.sequence <= input.toSeq
          ? [{ text: `${botName(item.asker)} asked: ${firstLine(item.text)}`, sequences: [item.sequence] }] : [];
      case "native_request": return [{ text: `${item.title}: request ${item.state === "blocked" ? "blocked" : "not confirmed"}`, sequences: [] }];
      case "waiting_step": return [{ text: `${item.name} is waiting for you`, sequences: [] }];
      case "blocked_step": return [{ text: `${item.name} is blocked`, sequences: [] }];
      case "draft": return [{ text: "A draft is waiting to be sent", sequences: [] }];
    }
  });

  const quiet: HandoverLine[] = [];
  if (toTime !== null) {
    for (const bot of input.bots) {
      if (bot.actorId === null) continue;
      const last = input.events.filter((event) => event.actorId === bot.actorId && event.sequence <= input.toSeq).at(-1);
      const lastTime = timeOf(last?.occurredAt ?? null);
      if (last === undefined || lastTime === null || toTime - lastTime < QUIET_IN_GAP_MS) continue;
      quiet.push({ text: `${bot.name}: no new record for ${Math.floor((toTime - lastTime) / 60_000)} min`, sequences: [last.sequence] });
    }
  }

  const touchedInGap = new Set(inGap.flatMap((event) => { const id = payloadOf(event).nodeId; return typeof id === "string" ? [id] : []; }));
  const untouched: HandoverLine[] = [];
  const firstNamed = new Map<string, number>();
  for (const event of input.events) {
    if (event.sequence > input.fromSeq) break;
    const id = payloadOf(event).nodeId;
    if (typeof id === "string" && !firstNamed.has(id)) firstNamed.set(id, event.sequence);
  }
  for (const [id, sequence] of firstNamed) {
    const node = input.model?.nodes.find((candidate) => candidate.id === id);
    if (touchedInGap.has(id) || (node !== undefined && SETTLED.has(node.state))) continue;
    untouched.push({ text: `${nodeName(id)} got no record`, sequences: [sequence] });
  }
  const recordedInGap = new Set(inGap.map((event) => event.actorId));
  for (const task of input.claudeTasks?.tasks ?? []) {
    if (task.createdSequence === null || task.createdSequence > input.fromSeq) continue;
    if (task.completedSequence !== null && task.completedSequence <= input.toSeq) continue;
    if (recordedInGap.has(task.sourceId)) continue;
    untouched.push({ text: `${task.taskSubject} (${botName(task.sourceId)}) got no record`, sequences: [task.createdSequence] });
  }

  return {
    fromSeq: input.fromSeq, toSeq: input.toSeq, eventCount: inGap.length,
    gapMinutes: fromTime === null || toTime === null ? 0 : Math.floor((toTime - fromTime) / 60_000),
    shipped, needsYou, quiet, untouched,
  };
}

export function lastSeenKey(project: string, executionId: string): string {
  return `graphhelm.handover.last-seen:${project}:${executionId}`;
}

export function readLastSeen(project: string, executionId: string): number | null {
  try {
    const raw = globalThis.localStorage.getItem(lastSeenKey(project, executionId));
    if (raw === null || !/^\d{1,15}$/.test(raw)) return null;
    return Number(raw);
  } catch {
    return null;
  }
}

export function writeLastSeen(project: string, executionId: string, sequence: number): void {
  try {
    globalThis.localStorage.setItem(lastSeenKey(project, executionId), String(Math.max(0, Math.floor(sequence))));
  } catch {
    // A convenience: without storage the next visit shows no card, which is correct.
  }
}
