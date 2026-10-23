import { arrangeRequests, requestActivity, requestColor, requestProgress, type Arranged, type OwnerRequest, type RequestProgress } from "../graph/requests";
import type { BoardTask, JourneyFlow } from "../runtime/client";
import type { Agent } from "./board";

/** The colour of one agent's square and chip in WHAT I ASKED (Lanes.dc.html): it asks a question,
 * it has been quiet 30 minutes or more, its slice waits to merge, or it is working. */
export type AskedTone = "asks" | "quiet" | "merge" | "on";

/** One owner request as the WHAT I ASKED column draws it. */
export interface AskedCard {
  requestId: string;
  /** "21 Oct 18:30" from the Runtime's `recordedAt`; `null` when the Runtime served none. */
  when: string | null;
  title: string;
  agents: { lane: string; tone: AskedTone }[];
  /** "N agents", plus " · needs a reply" when one of them asks. */
  summary: string;
  /** #1922: not started / in progress N of M merged / delivered / proven. */
  chip: RequestProgress["chip"];
  progress: RequestProgress;
  /** Newest activity in ms; orders the column. */
  at: number;
  /** The request's own stripe colour. */
  color: string;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** `21 Oct 18:30` in local time, or `null` for an absent or unreadable time. */
export function askedWhen(at: string | null | undefined): string | null {
  if (!at) return null;
  const date = new Date(at);
  if (Number.isNaN(date.getTime())) return null;
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getDate()} ${MONTHS[date.getMonth()]} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function tone(agent: Agent): AskedTone {
  if (agent.questions.length > 0) return "asks";
  if (agent.idleMinutes === null || agent.idleMinutes >= 30) return "quiet";
  return agent.column === "merge" ? "merge" : "on";
}

/** The agents working one request: the lanes holding a slice (not released) on one of its issues,
 * whichever slice their card shows. */
export function agentsFor(request: OwnerRequest, all: readonly Agent[], tasks: readonly BoardTask[]): Agent[] {
  const issues = new Set(request.issues.map((row) => row.issue));
  const lanes = new Set(tasks.filter((task) => task.releasedBy === undefined && task.issue !== null && issues.has(task.issue)).map((task) => task.lane));
  return all.filter((agent) => lanes.has(agent.lane));
}

/** The request each lane serves, by title (#1904, Lanes.dc.html "for:"): the newest request whose
 * issues hold one of that lane's open slices. */
export function requestTitles(requests: readonly OwnerRequest[], all: readonly Agent[], tasks: readonly BoardTask[]): Map<string, string> {
  const titles = new Map<string, string>();
  for (const request of [...requests].reverse()) {
    for (const agent of agentsFor(request, all, tasks)) {
      if (!titles.has(agent.lane)) titles.set(agent.lane, request.title ?? request.ask);
    }
  }
  return titles;
}

/** The stripe colour of the request each lane serves (same pick as `requestTitles`). */
export function requestColors(requests: readonly OwnerRequest[], all: readonly Agent[], tasks: readonly BoardTask[]): Map<string, string> {
  const colors = new Map<string, string>();
  for (const request of [...requests].reverse()) {
    for (const agent of agentsFor(request, all, tasks)) {
      if (!colors.has(agent.lane)) colors.set(agent.lane, requestColor(request.requestId));
    }
  }
  return colors;
}

/** Every request, newest first (the Runtime serves them oldest first), as the column draws it. */
export function askedCards(requests: readonly OwnerRequest[], all: readonly Agent[], tasks: readonly BoardTask[], flows: readonly JourneyFlow[] = [], stalled: ReadonlySet<string> = new Set()): AskedCard[] {
  const cards = [...requests].reverse().map((request) => {
    const working = agentsFor(request, all, tasks);
    const progress = requestProgress(request, tasks, flows, stalled);
    const asks = working.some((agent) => agent.questions.length > 0);
    return {
      requestId: request.requestId,
      when: askedWhen(request.recordedAt),
      title: request.title ?? request.ask,
      agents: working.map((agent) => ({ lane: agent.lane, tone: tone(agent) })),
      summary: working.length === 0 ? "no agent working" : `${working.length} ${working.length === 1 ? "agent" : "agents"}${asks ? " · needs a reply" : ""}`,
      chip: progress.chip,
      progress,
      at: requestActivity(request, tasks),
      color: requestColor(request.requestId),
    };
  });
  return cards;
}

/** The column's order: working first, not started next, delivered apart (collapsed by the view). */
export function arrangeAsked(cards: readonly AskedCard[], now: number): Arranged<AskedCard> {
  return arrangeRequests(cards, (card) => ({ key: card.chip.key, at: card.at }), now);
}
