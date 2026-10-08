/**
 * Chat threads for the Chat column: Everyone, one thread per task (#393, journey-first spec §8) and
 * the owner's direct line with one agent. Every message is RECORDED through the Runtime.
 */
import type { EnvelopeRecord } from "../graph/ledger";
import { SCREEN_CAPTURE_PROTOCOL, TRANSITION_PROTOCOL } from "./journeys";
import { botKeyOf, firstLine, type Bot } from "./team";
import type { RuntimeEvent } from "./types";
import type { TaskState } from "./team-tasks";
import type { WorkMessage } from "./work-conversation";

export type ThreadKind = "everyone" | "task" | "direct";
export interface ChatThread {
  key: string;
  kind: ThreadKind;
  label: string;
  participants: string[];
  messages: WorkMessage[];
  /** A task thread that is merged or has been quiet past the threshold; shown under "older". */
  older?: boolean;
}
export const EVERYONE = "everyone";
/** How long a task thread may stay quiet before it folds under "older" (spec §8 default). */
export const TASK_THREAD_QUIET_HOURS = 8;

export function namesOf(bots: Bot[]): Record<string, string> {
  const names: Record<string, string> = {};
  for (const bot of bots) {
    names[bot.key] = bot.name;
    if (bot.actorId !== null) names[bot.actorId] = bot.name;
  }
  return names;
}

function taskLabel(taskId: string, task: TaskState | undefined): string {
  if (task?.pr != null && task.issue != null) return `PR #${task.pr} · issue #${task.issue}`;
  if (task?.pr != null) return `PR #${task.pr}`;
  if (task?.issue != null) return `Issue #${task.issue}`;
  return taskId;
}

/** #393 (spec §8): threads follow the tasks of the Team tab. A message naming a `task` lands in
 * that task's thread, which a `task.*` record opens before its first line; a message to nobody
 * in particular, or between two agents with no task, is said to the room (Everyone); only the
 * owner's own line with one agent keeps a direct thread. */
export function chatThreads(messages: WorkMessage[], bots: Bot[], operatorId: string, tasks: TaskState[] = [],
  now: number = Date.now(), quietHours: number = TASK_THREAD_QUIET_HOURS): ChatThread[] {
  const names = namesOf(bots);
  const label = (id: string) => names[id] ?? id;
  const everyone: ChatThread = { key: EVERYONE, kind: "everyone", label: "Everyone", participants: [], messages: [] };
  const states = new Map(tasks.map((task) => [task.taskId, task]));
  const taskThreads = new Map<string, ChatThread>();
  const taskThread = (taskId: string) => {
    const thread = taskThreads.get(taskId) ?? { key: `task:${taskId}`, kind: "task" as const, label: taskLabel(taskId, states.get(taskId)), participants: [], messages: [] };
    taskThreads.set(taskId, thread);
    return thread;
  };
  for (const task of tasks) taskThread(task.taskId);
  const directs = new Map<string, ChatThread>();
  for (const message of messages) {
    if (message.task) { taskThread(message.task).messages.push(message); continue; }
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
    everyone.messages.push(message);
  }
  for (const [taskId, thread] of taskThreads) {
    const last = thread.messages.map((message) => Date.parse(message.at ?? "")).filter(Number.isFinite).reduce((a, b) => Math.max(a, b), Number.NEGATIVE_INFINITY);
    thread.older = states.get(taskId)?.step === "merged" || (Number.isFinite(last) && now - last > quietHours * 3_600_000);
  }
  return [everyone, ...taskThreads.values(), ...directs.values()];
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
