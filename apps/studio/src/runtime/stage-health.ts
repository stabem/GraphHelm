/* #591 (owner): each PR card on the Graph says how long the agent has been in its stage and, in
 * plain words, whether it needs help. Pure: the view passes the lanes and the clock it already has. */
import type { TaskState } from "./team-tasks";
import { LIVENESS_MS, type Lane } from "./lane-bars";
import { openBlock } from "./mission";
export { LIVENESS_MS };
import type { TimedStep } from "./step-timing";

export type HealthFlag = "needs_you" | "stalled" | "blocked" | "slow" | "moving";
export interface StageHealth { flag: HealthFlag; text: string; tone: "red" | "orange" | "amber" | "green"; elapsedMs: number | null; elapsed: string | null }

/** Slow: more than this many times the median time the group's other PRs spent in the same stage. */
export const SLOW_FACTOR = 2;
/** Fewer finished samples than this is no median: the card says Moving rather than invent a target. */
export const SLOW_MIN_SAMPLES = 2;

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
export function ownerLane(t: TaskState): string | null {
  if (t.step !== "review" || openBlock(t)) return t.lane;
  return pushedFix(t)?.reviewer || t.reviewers[t.reviewers.length - 1] || null;
}

export interface Liveness { live: boolean; sinceMs: number | null }
/** #591: a lane is live while its newest record of any kind, on any task, is under LIVENESS_MS old:
 * the lane bars' last event (claims, PRs, assignments, verdicts, merges) and the times the task
 * records carry for it (a pushed fix, a BLOCK). No record at all says nothing: live, `sinceMs` null. */
export function laneLiveness(name: string | null | undefined, lanes: Lane[], records: TaskState[], now: number): Liveness {
  if (!name) return { live: true, sinceMs: null };
  let last = lanes.find((l) => l.lane === name)?.lastEventAt ?? 0;
  const at = (iso: string | null | undefined) => { const ms = iso ? Date.parse(iso) : NaN; if (Number.isFinite(ms)) last = Math.max(last, ms); };
  for (const r of records) for (const round of r.rounds ?? []) {
    if (r.lane === name) at(round.fixedAt);
    if (round.reviewer === name) at(round.blockedAt);
  }
  if (!(last > 0)) return { live: true, sinceMs: null };
  const sinceMs = Math.max(0, now - last);
  return { live: sinceMs < LIVENESS_MS, sinceMs };
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

/** Merged work has no health (null). Order: Needs you, Stalled (the owner lane silent LIVENESS_MS), Blocked, Slow, Moving. `needsYou` is the
 * caller's owner-wait fact for this task (a held destructive step or the summary's need-you rule). */
export function stageHealth(t: TaskState, lanes: Lane[], groupTasks: TaskState[], now: number, needsYou = false): StageHealth | null {
  if (t.step === "merged") return null;
  const since = stageSince(t, lanes);
  const elapsedMs = since === null ? null : Math.max(0, now - since);
  const base = { elapsedMs, elapsed: elapsedMs === null ? null : stageDuration(elapsedMs) };
  if (needsYou) return { ...base, flag: "needs_you", text: "Needs you", tone: "red" };
  const owner = ownerLane(t);
  const live = laneLiveness(owner, lanes, groupTasks, now);
  if (!live.live && live.sinceMs !== null) {
    return { ...base, flag: "stalled", text: `${owner} silent ${stageDuration(live.sinceMs)}`, tone: "red" };
  }
  const block = openBlock(t);
  if (block) return { ...base, flag: "blocked", text: `Blocked by ${block.reviewer || "a reviewer"}`, tone: "orange" };
  const steps = timed(t);
  const samples = groupTasks.filter((o) => o.key !== t.key)
    .map((o) => steps.reduce((sum, s) => sum + (o.clock?.spent?.[s] ?? 0), 0)).filter((ms) => ms > 0);
  if (elapsedMs !== null && samples.length >= SLOW_MIN_SAMPLES && elapsedMs > SLOW_FACTOR * median(samples)) {
    return { ...base, flag: "slow", text: "Slow", tone: "amber" };
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

export type PaceStage = "plan" | "implement" | "review" | "fix" | "merge";
/** The usual time of a stage when the group has fewer than SLOW_MIN_SAMPLES finished samples. */
export const EXPECTED_FALLBACK_MS: Record<PaceStage, number> = {
  plan: 60 * 60_000, implement: 3 * 60 * 60_000, review: 60 * 60_000, fix: 2 * 60 * 60_000, merge: 15 * 60_000,
};
/** Under this share of the usual time the bar is green; up to 1 it is amber; at or over 1 red. */
export const PACE_AMBER = 0.75;
export type PaceTone = "green" | "amber" | "red";
export interface StageProgress { elapsedMs: number; expectedMs: number; ratio: number; tone: PaceTone }

export const paceStage = (t: TaskState): PaceStage | null =>
  t.step === "merged" ? null : openBlock(t) ? "fix" : t.step === "critic" ? "plan" : t.step;

/** Elapsed in the stage against the usual time for it: the median the group's other PRs spent in
 * the same stage, else the stage's named fallback. `ratio` is capped at 1 (the bar's fill). */
export function stageProgress(t: TaskState, groupTasks: TaskState[], now: number, lanes: Lane[] = []): StageProgress | null {
  const stage = paceStage(t);
  if (!stage) return null;
  const since = stageSince(t, lanes);
  if (since === null) return null;
  const elapsedMs = Math.max(0, now - since);
  const steps = timed(t);
  const samples = groupTasks.filter((o) => o.key !== t.key)
    .map((o) => steps.reduce((sum, s) => sum + (o.clock?.spent?.[s] ?? 0), 0)).filter((ms) => ms > 0);
  const expectedMs = samples.length >= SLOW_MIN_SAMPLES ? median(samples) : EXPECTED_FALLBACK_MS[stage];
  const raw = expectedMs > 0 ? elapsedMs / expectedMs : 1;
  return { elapsedMs, expectedMs, ratio: Math.min(1, raw), tone: raw >= 1 ? "red" : raw >= PACE_AMBER ? "amber" : "green" };
}

/** Under this the lane counts as active right now (pulsing green dot): the one liveness rule. */
export const ACTIVE_MS = LIVENESS_MS;
export interface Activity { sinceMs: number | null; tone: PaceTone }
/** How long since the lane's latest record: green while `laneLiveness` says live, red once it is
 * stalled, so the dot always agrees with the card's Stalled flag. No record at all is red. */
export function activity(lane: string | null | undefined, lanes: Lane[], now: number, records: TaskState[] = []): Activity {
  const l = laneLiveness(lane, lanes, records, now);
  if (l.sinceMs === null) return { sinceMs: null, tone: "red" };
  return { sinceMs: l.sinceMs, tone: l.live ? "green" : "red" };
}
