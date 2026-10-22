import type { OwnerRequest } from "../graph/requests";
import type { BoardTask } from "../runtime/client";
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

/** Every request, newest first (the Runtime serves them oldest first), as the column draws it. */
export function askedCards(requests: readonly OwnerRequest[], all: readonly Agent[], tasks: readonly BoardTask[]): AskedCard[] {
  return [...requests].reverse().map((request) => {
    const working = agentsFor(request, all, tasks);
    const asks = working.some((agent) => agent.questions.length > 0);
    return {
      requestId: request.requestId,
      when: askedWhen(request.recordedAt),
      title: request.title ?? request.ask,
      agents: working.map((agent) => ({ lane: agent.lane, tone: tone(agent) })),
      summary: `${working.length} ${working.length === 1 ? "agent" : "agents"}${asks ? " · needs a reply" : ""}`,
    };
  });
}
