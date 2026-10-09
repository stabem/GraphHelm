import type { TaskEventRecord } from "./team-tasks";

export type TimedTaskEvent = TaskEventRecord & { at: string };
export type BarKind = "implement" | "review" | "merge";
export interface LaneBar { kind: BarKind; label: string; start: number; end: number; open: boolean }
export interface Lane { lane: string; bars: LaneBar[]; silent: boolean; lastEventAt: number }

export const STALL_MS = 2 * 60 * 60 * 1000;

export function laneBars(events: TimedTaskEvent[], now: number, windowMs: number): Lane[] {
  const lanes = new Map<string, { bars: LaneBar[]; last: number }>();
  const open = new Map<string, { lane: string; bar: LaneBar }>();
  const prOf = new Map<string, number>();
  const laneOf = (name: string) => {
    let l = lanes.get(name);
    if (!l) { l = { bars: [], last: 0 }; lanes.set(name, l); }
    return l;
  };
  const label = (taskId: string) => (prOf.has(taskId) ? `#${prOf.get(taskId)}` : taskId);
  const start = (lane: string, kind: BarKind, taskId: string, t: number) => {
    const bar: LaneBar = { kind, label: label(taskId), start: t, end: now, open: true };
    laneOf(lane).bars.push(bar);
    open.set(`${kind}:${taskId}:${lane}`, { lane, bar });
  };
  const close = (kind: BarKind, taskId: string, t: number, lane?: string) => {
    for (const [k, v] of open) {
      if (k.startsWith(`${kind}:${taskId}:`) && (!lane || v.lane === lane)) {
        v.bar.end = t; v.bar.open = false; open.delete(k);
      }
    }
  };
  for (const e of [...events].sort((a, b) => a.sequence - b.sequence)) {
    const t = Date.parse(e.at);
    if (e.pr !== undefined) prOf.set(e.taskId, e.pr);
    const actor = e.kind === "task.claimed" ? e.lane : e.reviewer;
    if (actor) laneOf(actor).last = Math.max(laneOf(actor).last, t);
    switch (e.kind) {
      case "task.claimed": if (e.lane) start(e.lane, "implement", e.taskId, t); break;
      case "task.review_assigned": close("implement", e.taskId, t); if (e.reviewer) start(e.reviewer, "review", e.taskId, t); break;
      case "task.review_verdict":
        close("review", e.taskId, t, e.reviewer);
        if (e.reviewer && String(e.verdict) === "APPROVE") start(e.reviewer, "merge", e.taskId, t);
        break;
      case "task.merged": close("merge", e.taskId, t); break;
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
