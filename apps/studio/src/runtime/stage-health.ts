/* #591 (owner): each PR card on the Graph says how long the agent has been in its stage and, in
 * plain words, whether it needs help. Pure: the view passes the lanes and the clock it already has. */
import type { TaskState } from "./team-tasks";
import { LIVENESS_MS, type Lane } from "./lane-bars";
import { openBlock } from "./mission";
export { LIVENESS_MS };
import type { TimedStep } from "./step-timing";
import { slotStatus, slotText, type SlotView } from "./slots";

export type HealthFlag = "needs_you" | "building" | "waiting_build" | "stalled" | "blocked" | "slow" | "moving";
export interface StageHealth { flag: HealthFlag; text: string; tone: "red" | "orange" | "amber" | "green" | "blue"; elapsedMs: number | null; elapsed: string | null }

/** Slow: elapsed more than this many times the stage's expected time (`expectedTime`). */
export const SLOW_FACTOR = 2;
/** Fewer finished samples than this is no median: the expected time is the stage allowance alone,
 * and the card says Moving rather than call a step Slow against no history. */
export const SLOW_MIN_SAMPLES = 3;

/** "45s", "17m", "1h 17m", "2d 4h". */
export function stageDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return m % 60 ? `${h}h ${m % 60}m` : `${h}h`;
  return h % 24 ? `${Math.floor(h / 24)}d ${h % 24}h` : `${Math.floor(h / 24)}d`;
}

const timed = (t: TaskState): TimedStep[] => (t.step === "plan" || t.step === "critic" ? ["plan", "critic"] : t.step === "merged" ? [] : [t.step]);
const openBar = (t: TaskState, lanes: Lane[]) => {
  for (const l of lanes) for (const b of l.bars) if (b.open && b.taskId === t.taskId) return { lane: l, bar: b };
  return null;
};

/** The fix pushed after the last BLOCK, while the re-review has not answered (#591). */
const pushedFix = (t: TaskState) => {
  const last = (t.rounds ?? []).at(-1);
  return t.step === "review" && !openBlock(t) && last?.fixHead ? last : null;
};

/** #591: whose silence stalls the step: the author for Implement/Fix/Merge (and Plan), the reviewer
 * for Review and Re-review (the BLOCKing reviewer re-reviews). `null` when no reviewer is named yet. */
export type OwnerRole = "author" | "reviewer";
/** #591: the role of the owner lane: the reviewer on a Review/Re-review, else the author. */
export const ownerRole = (t: TaskState): OwnerRole => (t.step === "review" && !openBlock(t) ? "reviewer" : "author");

export function ownerLane(t: TaskState): string | null {
  if (t.step !== "review" || openBlock(t)) return t.lane;
  return pushedFix(t)?.reviewer || t.reviewers[t.reviewers.length - 1] || null;
}

export interface Liveness { live: boolean; sinceMs: number | null }
/** #591: a lane is live while its newest record of any kind, on any task, is under LIVENESS_MS old:
 * the lane bars' last event (claims, PRs, assignments, verdicts, merges) and the times the task
 * records carry for it (a pushed fix, a BLOCK). No record at all says nothing: live, `sinceMs` null. */
export function laneLiveness(name: string | null | undefined, lanes: Lane[], records: TaskState[], now: number, slots: SlotView[] = []): Liveness {
  if (!name) return { live: true, sinceMs: null };
  let last = lanes.find((l) => l.lane === name)?.lastEventAt ?? 0;
  const at = (iso: string | null | undefined) => { const ms = iso ? Date.parse(iso) : NaN; if (Number.isFinite(ms)) last = Math.max(last, ms); };
  for (const r of records) for (const round of r.rounds ?? []) {
    if (r.lane === name) at(round.fixedAt);
    if (round.reviewer === name) at(round.blockedAt);
  }
  if (!(last > 0)) return { live: true, sinceMs: null };
  const sinceMs = Math.max(0, now - last);
  // #636: a lane building or queued for a build slot is alive: queue time is not silence.
  return { live: sinceMs < LIVENESS_MS || slotStatus(name, slots) !== null, sinceMs };
}

/** When the task entered its stage: a pushed fix's time for the re-review (#591), else the Runtime's
 * step clock (what the Team view times), else the unclipped start of the lane bar working it. */
export function stageSince(t: TaskState, lanes: Lane[]): number | null {
  const fixed = pushedFix(t)?.fixedAt;
  const fixedAt = fixed ? Date.parse(fixed) : NaN;
  if (Number.isFinite(fixedAt)) return fixedAt;
  const at = t.clock?.since ? Date.parse(t.clock.since) : NaN;
  if (Number.isFinite(at)) return at;
  return openBar(t, lanes)?.bar.since ?? null;
}

const median = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b), m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m]! : (s[m - 1]! + s[m]!) / 2;
};

export type PaceStage = "plan" | "implement" | "review" | "fix" | "merge";
/** #591: each stage's named minimum allowance. Lanes record `planned` right after `claimed`, so a
 * group median can be ~1s; the expected time never drops under this. */
export const STAGE_ALLOWANCE_MS: Record<PaceStage, number> = {
  plan: 30 * 60_000, implement: 3 * 60 * 60_000, fix: 2 * 60 * 60_000, review: 60 * 60_000, merge: 15 * 60_000,
};
export const paceStage = (t: TaskState): PaceStage | null =>
  t.step === "merged" ? null : openBlock(t) ? "fix" : t.step === "critic" ? "plan" : t.step;

/** Expected time in the stage: max(allowance, median of the group's other PRs), the median only
 * from SLOW_MIN_SAMPLES samples up. `samples` is how many finished samples there were. */
export function expectedTime(t: TaskState, groupTasks: TaskState[]): { expectedMs: number; samples: number } | null {
  const stage = paceStage(t);
  if (!stage) return null;
  const steps = timed(t);
  const xs = groupTasks.filter((o) => o.key !== t.key)
    .map((o) => steps.reduce((sum, s) => sum + (o.clock?.spent?.[s] ?? 0), 0)).filter((ms) => ms > 0);
  const med = xs.length >= SLOW_MIN_SAMPLES ? median(xs) : 0;
  return { expectedMs: Math.max(STAGE_ALLOWANCE_MS[stage], med), samples: xs.length };
}

/** Merged work has no health (null). Order: Needs you, Stalled (the owner lane silent LIVENESS_MS), Blocked, Slow, Moving. `needsYou` is the
 * caller's owner-wait fact for this task (a held destructive step or the summary's need-you rule). */
export function stageHealth(t: TaskState, lanes: Lane[], groupTasks: TaskState[], now: number, needsYou = false, slots: SlotView[] = []): StageHealth | null {
  if (t.step === "merged") return null;
  const since = stageSince(t, lanes);
  const elapsedMs = since === null ? null : Math.max(0, now - since);
  const base = { elapsedMs, elapsed: elapsedMs === null ? null : stageDuration(elapsedMs) };
  if (needsYou) return { ...base, flag: "needs_you", text: "Needs you", tone: "red" };
  const owner = ownerLane(t);
  // #636: the owner lane building or waiting for a build slot is never Slow or Stalled.
  const slot = slotStatus(owner, slots);
  if (slot) return { ...base, flag: slot.kind === "building" ? "building" : "waiting_build", text: slotText(slot), tone: "blue" };
  const live = laneLiveness(owner, lanes, groupTasks, now);
  if (!live.live && live.sinceMs !== null) {
    return { ...base, flag: "stalled", text: `${owner} silent ${stageDuration(live.sinceMs)}`, tone: "red" };
  }
  const block = openBlock(t);
  if (block) return { ...base, flag: "blocked", text: `Blocked by ${block.reviewer || "a reviewer"}`, tone: "orange" };
  const exp = expectedTime(t, groupTasks);
  if (elapsedMs !== null && exp && exp.samples >= SLOW_MIN_SAMPLES && elapsedMs > SLOW_FACTOR * exp.expectedMs) {
    const who = live.sinceMs !== null ? ` · ${ownerRole(t)} active ${stageDuration(live.sinceMs)} ago` : "";
    return { ...base, flag: "slow", text: `Slow${who}`, tone: "amber" };
  }
  return { ...base, flag: "moving", text: "Moving", tone: "green" };
}

/** "5h 28m 12s"; under an hour "4m 03s"; under a minute "12s". The live card timer. */
export function liveDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
  const ss = String(r).padStart(2, "0");
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m ${ss}s`;
  if (m > 0) return `${m}m ${ss}s`;
  return `${r}s`;
}

/** Red belongs to Stalled alone (the liveness rule), which the view applies; pace never paints red. */
export type PaceTone = "green" | "amber" | "red";
/** `tone`: green up to the expected time, amber past it. */
export interface StageProgress { elapsedMs: number; expectedMs: number; ratio: number; pace: number; tone: "green" | "amber" }

/** Elapsed in the stage against its expected time (`expectedTime`). `ratio` is capped at 1 (the
 * bar's fill); `pace` is the uncapped multiple of the usual time. */
export function stageProgress(t: TaskState, groupTasks: TaskState[], now: number, lanes: Lane[] = []): StageProgress | null {
  const exp = expectedTime(t, groupTasks);
  if (!exp) return null;
  const since = stageSince(t, lanes);
  if (since === null) return null;
  const elapsedMs = Math.max(0, now - since);
  const pace = elapsedMs / exp.expectedMs;
  return { elapsedMs, expectedMs: exp.expectedMs, ratio: Math.min(1, pace), pace, tone: pace > 1 ? "amber" : "green" };
}

/** Under this the lane counts as active right now (pulsing green dot): the one liveness rule. */
export const ACTIVE_MS = LIVENESS_MS;
export interface Activity { sinceMs: number | null; tone: PaceTone; role?: OwnerRole }
/** How long since the lane's latest record: green while `laneLiveness` says live, red once it is
 * stalled, so the dot always agrees with the card's Stalled flag. No record at all is red. */
export function activity(lane: string | null | undefined, lanes: Lane[], now: number, records: TaskState[] = [], role?: OwnerRole, slots: SlotView[] = []): Activity {
  const l = laneLiveness(lane, lanes, records, now, slots);
  const r = role ? { role } : {};
  if (l.sinceMs === null) return { sinceMs: null, tone: "red", ...r };
  return { sinceMs: l.sinceMs, tone: l.live ? "green" : "red", ...r };
}
