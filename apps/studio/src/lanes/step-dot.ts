import type { BoardTask, TaskStepName } from "../runtime/client";
import { when } from "../time/when";
import { STEPS, type Block, blocks } from "./board";

/** #2532: what a step dot says. Only states the board shows; never invented text. */
export type DotState = "done" | "in progress" | "stalled" | "not started";

export interface StepDot {
  step: TaskStepName;
  state: DotState;
  /** One line for the tooltip, the aria-label and the screen reader. */
  text: string;
  /** The record the dot names last, as its tooltip shows it; `null` for a dot with no record. */
  record: string | null;
  /** The record or link the dot opens; `null` when the board holds none. */
  href: string | null;
}

/** What the board knows about one step of one slice; each field is `null` when it is not known. */
export interface StepFacts {
  /** The declared cause of a stall or block (#2531, `cause`); `null` until the record carries one. */
  cause: string | null;
  who: string | null;
  when: string | null;
  /** The last record of the slice, as the board names it (its sequence). */
  record: string | null;
  /** When the stalled step went silent (the Runtime's idle time), as the tooltip shows it. */
  silentSince: string | null;
  href: string | null;
}

const NONE: StepFacts = { cause: null, who: null, when: null, record: null, silentSince: null, href: null };

/** A link to a pull request, built from the task's `repoUrl` (`https://github.com/<owner>/<repo>`)
 * and its PR number; anything else is no link. */
export function prLink(repoUrl: string | null | undefined, pr: number | null | undefined): string | null {
  if (repoUrl === null || repoUrl === undefined || pr === null || pr === undefined || !Number.isInteger(pr) || pr < 1) return null;
  if (!/^https:\/\/github\.com\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repoUrl)) return null;
  return `${repoUrl}/pull/${pr}`;
}

/** The tooltip line for one dot. A dot with no record says "not started" and nothing else. */
export function dotText(step: TaskStepName, state: DotState, facts: StepFacts = NONE): string {
  if (state === "not started") return `${step} · not started`;
  const parts: string[] = [step, state];
  if (state === "stalled") parts.push(facts.cause ?? "no reason given", facts.silentSince === null ? "silent time unknown" : `silent since ${facts.silentSince}`);
  if (facts.who !== null) parts.push(facts.who);
  if (state === "in progress" && facts.when !== null) parts.push(`since ${facts.when}`);
  parts.push(facts.record ?? "no record in view");
  return parts.join(" · ");
}

const STATE_OF: Record<Block, DotState> = { done: "done", now: "in progress", stuck: "stalled", todo: "not started" };

/** The dots of a Lanes card: the slice's step, its state from the card's blocks. */
export function lanesDots(task: BoardTask | null, step: TaskStepName | null, stuck: boolean, lastRecordAt: string | null, now: Date): StepDot[] {
  const states = blocks(step, stuck);
  const current = step === null ? -1 : STEPS.indexOf(step);
  return STEPS.map((name, index) => {
    const state = STATE_OF[states[index]!];
    const live = index === current;
    const facts: StepFacts = {
      cause: null,
      who: task === null || state === "not started" ? null : live && task.currentReviewer && name === "review" ? task.currentReviewer : task?.lane ?? null,
      when: live && task?.clock?.since ? when(task.clock.since, now).text : null,
      record: live && task !== null ? `record #${task.lastSequence}` : null,
      silentSince: state === "stalled" && lastRecordAt !== null ? when(lastRecordAt, now).text : null,
      href: state === "not started" ? null : prLink(task?.repoUrl, task?.pr),
    };
    return { step: name, state, text: dotText(name, state, facts), record: state === "not started" ? null : facts.record ?? "no record in view", href: facts.href };
  });
}

/** The dots of a Graph issue card: squares before the stage are done; the stage is in progress,
 * or stalled when the card sits in the Ready column; the rest are not started. */
export function graphDots(task: BoardTask, at: number, stalled: boolean, silentSince: string | null, now: Date): StepDot[] {
  return STEPS.map((name, index) => {
    const state: DotState = index < at ? "done" : index === at ? (stalled ? "stalled" : "in progress") : "not started";
    const live = index === at;
    const facts: StepFacts = {
      cause: null,
      who: state === "not started" ? null : live && task.currentReviewer && name === "review" ? task.currentReviewer : task.lane,
      when: live && task.clock?.since ? when(task.clock.since, now).text : null,
      record: live ? `record #${task.lastSequence}` : null,
      silentSince: state === "stalled" && silentSince !== null ? when(silentSince, now).text : null,
      href: state === "not started" ? null : prLink(task.repoUrl, task.pr),
    };
    return { step: name, state, text: dotText(name, state, facts), record: state === "not started" ? null : facts.record ?? "no record in view", href: facts.href };
  });
}
