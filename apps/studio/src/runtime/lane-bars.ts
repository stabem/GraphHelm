import type { TaskEventRecord } from "./team-tasks";

export type TimedTaskEvent = TaskEventRecord & { at: string };
export type BarKind = "implement" | "review" | "merge";
/** `taskId` names the task the bar works on; `since` is the unclipped start (`start` is clipped to the window). */
export interface LaneBar { kind: BarKind; label: string; start: number; end: number; open: boolean; taskId?: string; since?: number }
export interface Lane { lane: string; bars: LaneBar[]; silent: boolean; lastEventAt: number }

export const STALL_MS = 2 * 60 * 60 * 1000;

export function laneBars(events: TimedTaskEvent[], now: number, windowMs: number): Lane[] {
  const lanes = new Map<string, { bars: LaneBar[]; last: number }>();
  const open = new Map<string, { lane: string; bar: LaneBar; slice: string }>();
  // #591: a bar belongs to one slice of its task, as foldTaskEvents keys it: its PR once one is
  // recorded, else its claim. Keying by taskId let one slice's merge or a later claim close another
  // slice's open review.
  const claims = new Map<string, { key: string; lane?: string }[]>();
  const known = new Set<string>();
  const latest = new Map<string, string>();
  const prOfKey = new Map<string, number>();
  const laneOf = (name: string) => {
    let l = lanes.get(name);
    if (!l) { l = { bars: [], last: 0 }; lanes.set(name, l); }
    return l;
  };
  const rekey = (from: string, to: string, label: string) => {
    for (const [k, v] of [...open]) {
      const [kind, slice, ...rest] = k.split("|");
      if (slice === from) { open.delete(k); v.slice = to; v.bar.label = label; open.set([kind, to, ...rest].join("|"), v); }
    }
  };
  const sliceOf = (e: TimedTaskEvent): string => {
    let key: string;
    if (e.kind === "task.claimed") {
      key = `${e.taskId}#claim-${e.sequence}`;
      const list = claims.get(e.taskId) ?? [];
      list.push({ key, lane: e.lane });
      claims.set(e.taskId, list);
    } else if (e.pr !== undefined) {
      key = `${e.taskId}#pr-${e.pr}`;
      prOfKey.set(key, e.pr);
      if (!known.has(key)) {
        // A PR no slice holds yet joins the oldest open claim of its task (the same lane's, when named).
        const list = claims.get(e.taskId) ?? [];
        const i = list.findIndex((c) => e.lane === undefined || c.lane === undefined || c.lane === e.lane);
        if (i >= 0) {
          const [claim] = list.splice(i, 1);
          rekey(claim.key, key, `#${e.pr}`);
        }
      }
    } else {
      key = latest.get(e.taskId) ?? e.taskId;
    }
    known.add(key);
    latest.set(e.taskId, key);
    return key;
  };
  const start = (lane: string, kind: BarKind, taskId: string, slice: string, t: number) => {
    const k = [kind, slice, lane].join("|");
    const prev = open.get(k);
    if (prev) { prev.bar.end = t; prev.bar.open = false; open.delete(k); }
    const pr = prOfKey.get(slice);
    const bar: LaneBar = { kind, label: pr !== undefined ? `#${pr}` : taskId, start: t, end: now, open: true, taskId, since: t };
    laneOf(lane).bars.push(bar);
    open.set(k, { lane, bar, slice });
  };
  const close = (kind: BarKind, slice: string, t: number, lane?: string) => {
    for (const [k, v] of [...open]) {
      if (k.startsWith(`${kind}|${slice}|`) && (!lane || v.lane === lane)) {
        v.bar.end = t; v.bar.open = false; open.delete(k);
      }
    }
  };
  for (const e of [...events].sort((a, b) => a.sequence - b.sequence)) {
    const t = Date.parse(e.at);
    if (!Number.isFinite(t)) continue;
    const slice = sliceOf(e);
    const actor = e.kind === "task.claimed" ? e.lane : e.reviewer;
    if (actor) laneOf(actor).last = Math.max(laneOf(actor).last, t);
    switch (e.kind) {
      case "task.claimed": if (e.lane) start(e.lane, "implement", e.taskId, slice, t); break;
      case "task.review_assigned": close("implement", slice, t); if (e.reviewer) start(e.reviewer, "review", e.taskId, slice, t); break;
      case "task.review_verdict":
        close("review", slice, t, e.reviewer);
        if (e.reviewer && String(e.verdict).startsWith("APPROVE")) start(e.reviewer, "merge", e.taskId, slice, t);
        break;
      case "task.merged": for (const k of ["implement", "review", "merge"] as const) close(k, slice, t); break;
      default: break;
    }
  }
  const from = now - windowMs;
  return [...lanes.entries()]
    .map(([lane, l]) => {
      const bars = l.bars.filter((b) => b.end >= from).map((b) => ({ ...b, start: Math.max(b.start, from) }));
      const waiting = l.bars.some((b) => b.open && b.kind !== "implement");
      return { lane, bars, lastEventAt: l.last, silent: waiting && now - l.last >= STALL_MS };
    })
    .sort((a, b) => a.lane.localeCompare(b.lane));
}
