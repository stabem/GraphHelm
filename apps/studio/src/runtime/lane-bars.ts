import { foldTaskEvents, type TaskEventRecord } from "./team-tasks";

export type TimedTaskEvent = TaskEventRecord & { at: string };
export type BarKind = "implement" | "review" | "merge";
/** `taskId` names the task the bar works on; `since` is the unclipped start (`start` is clipped to the window). */
export interface LaneBar {
  kind: BarKind; label: string; start: number; end: number; open: boolean; taskId?: string; since?: number;
  lastActivityAt?: number; endedBy?: { kind: string; actor: string };
  displayLabel?: string;
  /** #591: the issue a claim names; the head a review was assigned on. */
  issue?: number; headSha?: string;
}
/** #591: the newest record a lane left: its kind without the `task.` prefix, and its PR or issue. */
export interface LaneRecord { kind: string; pr?: number; issue?: number; at: number }
export interface Lane {
  lane: string; bars: LaneBar[]; silent: boolean; lastEventAt: number;
  /** Latest observed session end for this lane. */
  endedAt?: number;
  lastRecord?: LaneRecord | null;
  /** #591: PRs this lane opened that wait on a review: no verdict on their newest head, not merged. */
  awaiting?: number[];
}

/** #591: a lane with no record of any kind for this long is silent; a step it owns reads Stalled.
 * One rule for the Graph cards, the pace dot and the Lanes agent board. */
export const LIVENESS_MS = 30 * 60 * 1000;
export function laneBars(events: TimedTaskEvent[], now: number, windowMs: number): Lane[] {
  const lanes = new Map<string, { bars: LaneBar[]; last: number; record: LaneRecord | null; awaiting: Set<number> }>();
  const open = new Map<string, { lane: string; bar: LaneBar; slice: string }>();
  // The kanban fold assigns every record to a slice; stable indices also survive PR renames.
  const routed = new Map<number, string>();
  const slices = foldTaskEvents(events.map(({ at, ...event }) => ({ ...event, occurredAt: at })),
    (event, index) => routed.set(event.sequence, `slice-${index}`));
  const prOfKey = new Map<string, number>();
  const prSlices = new Map<number, Set<string>>();
  const titleOf = new Map<string, string>();
  const headOf = new Map<string, string>();
  const authorOf = new Map<string, string>();
  const issueOf = new Map<string, number>();
  // Read titles from the same completed slices that feed the kanban. Older records may
  // carry a title only on the claim or PR opening, not on later timeline records.
  slices.forEach((state, index) => {
    const slice = `slice-${index}`;
    if (state.title || state.prTitle) titleOf.set(slice, state.prTitle ?? state.title!);
  });
  const laneOf = (name: string) => {
    let l = lanes.get(name);
    if (!l) { l = { bars: [], last: 0, record: null, awaiting: new Set() }; lanes.set(name, l); }
    return l;
  };
  const start = (lane: string, kind: BarKind, taskId: string, slice: string, t: number) => {
    const k = [kind, slice, lane].join("|");
    const prev = open.get(k);
    if (prev) { prev.bar.end = t; prev.bar.open = false; prev.bar.endedBy = { kind: "claimed", actor: lane }; open.delete(k); }
    const pr = prOfKey.get(slice);
    const issue = issueOf.get(slice);
    const title = titleOf.get(slice);
    const label = pr !== undefined ? `#${pr}` : taskId;
    const bar: LaneBar = { kind, label, ...(title ? { displayLabel: `${pr !== undefined ? `#${pr}` : issue !== undefined ? `#${issue}` : taskId} ${title}` } : {}), start: t, end: now, open: true, taskId, since: t, lastActivityAt: t };
    if (issue !== undefined) bar.issue = issue;
    if (kind === "review" && headOf.has(slice)) bar.headSha = headOf.get(slice);
    laneOf(lane).bars.push(bar);
    open.set(k, { lane, bar, slice });
  };
  const close = (kind: BarKind, slice: string, t: number, event: TimedTaskEvent, lane?: string) => {
    for (const [k, v] of [...open]) {
      if (k.startsWith(`${kind}|${slice}|`) && (!lane || v.lane === lane)) {
        v.bar.end = t; v.bar.open = false; v.bar.endedBy = { kind: event.kind.replace(/^task\./, ""), actor: event.actorId }; open.delete(k);
      }
    }
  };
  const closePr = (pr: number, t: number, event: TimedTaskEvent, kind?: BarKind, head?: string, onlySlice?: string) => {
    for (const [key, value] of [...open]) {
      if (prOfKey.get(value.slice) !== pr || (onlySlice && value.slice !== onlySlice) || (kind && value.bar.kind !== kind)
        || (head && value.bar.headSha !== head)) continue;
      value.bar.end = t; value.bar.open = false; value.bar.endedBy = { kind: event.kind.replace(/^task\./, ""), actor: event.actorId }; open.delete(key);
    }
  };
  for (const e of [...events].sort((a, b) => a.sequence - b.sequence)) {
    const t = Date.parse(e.at);
    if (!Number.isFinite(t)) continue;
    const slice = routed.get(e.sequence);
    if (slice === undefined) continue;
    // Ending records describe the boundary, not continuous work in the gap before it.
    if (e.kind === "task.planned" || e.kind === "task.pr_opened" || e.kind === "task.critic_verdict") {
      for (const v of open.values()) if (v.slice === slice && v.lane === e.actorId)
        v.bar.lastActivityAt = Math.max(v.bar.lastActivityAt ?? v.bar.since ?? v.bar.start, t);
    }
    if (e.pr !== undefined) {
      prOfKey.set(slice, e.pr);
      const matching = prSlices.get(e.pr) ?? new Set<string>();
      matching.add(slice);
      prSlices.set(e.pr, matching);
      if (e.kind === "task.pr_opened") {
        for (const v of open.values()) if (v.slice === slice) {
          v.bar.label = `#${e.pr}`;
          const title = titleOf.get(slice);
          if (title) v.bar.displayLabel = `#${e.pr} ${title}`;
        }
      }
    }
    // #591: any record a lane leaves (claim, plan, pr_opened, assignment, verdict, merge) is a sign of life.
    for (const actor of new Set([e.lane, e.reviewer])) {
      if (!actor) continue;
      const l = laneOf(actor);
      if (t >= l.last) {
        l.last = t;
        const rec: LaneRecord = { kind: e.kind.replace(/^task\./, ""), at: t };
        const pr = e.pr ?? prOfKey.get(slice);
        if (pr !== undefined) rec.pr = pr;
        if (e.issue !== undefined) rec.issue = e.issue;
        l.record = rec;
      }
    }
    if (e.issue !== undefined) issueOf.set(slice, e.issue);
    if (typeof e.title === "string" && !titleOf.has(slice)) {
      titleOf.set(slice, e.title);
      for (const v of open.values()) if (v.slice === slice) v.bar.displayLabel = `${e.pr !== undefined ? `#${e.pr}` : e.issue !== undefined ? `#${e.issue}` : e.taskId} ${e.title}`;
    }
    if (e.headSha !== undefined && (e.kind === "task.pr_opened" || e.kind === "task.claimed")) {
      if (e.kind === "task.pr_opened" && headOf.has(slice) && headOf.get(slice) !== e.headSha) close("review", slice, t, e);
      headOf.set(slice, e.headSha);
    }
    if (e.lane && (e.kind === "task.claimed" || e.kind === "task.pr_opened")) authorOf.set(slice, e.lane);
    const pr = prOfKey.get(slice);
    const author = authorOf.get(slice);
    switch (e.kind) {
      case "task.claimed": if (e.lane) start(e.lane, "implement", e.taskId, slice, t); break;
      case "task.pr_opened": if (author && pr !== undefined) laneOf(author).awaiting.add(pr); break;
      case "task.released": close("implement", slice, t, e, e.lane); break;
      case "task.review_assigned":
        close("implement", slice, t, e);
        if (e.headSha !== undefined && e.headSha === headOf.get(slice)) close("review", slice, t, e);
        if (e.reviewer) start(e.reviewer, "review", e.taskId, slice, t);
        break;
      case "task.review_verdict":
        if (e.pr !== undefined && e.headSha !== undefined) closePr(e.pr, t, e, "review", e.headSha, slice);
        if (e.headSha !== undefined && headOf.get(slice) !== undefined && e.headSha === headOf.get(slice)) {
          if (e.pr === undefined) close("review", slice, t, e, e.reviewer);
          if (author && pr !== undefined) laneOf(author).awaiting.delete(pr);
        }
        break;
      case "task.merged": {
        if (e.pr !== undefined) for (const lane of lanes.values()) lane.awaiting.delete(e.pr);
        for (const s of e.pr !== undefined ? prSlices.get(e.pr) ?? new Set([slice]) : [slice])
          for (const k of ["implement", "review", "merge"] as const) close(k, s, t, e);
        // #591: only issue closure ends PR-less claims across lanes. A Refs slice ends only
        // its author's matching claims; other PR slices stay open.
        const issue = e.issue ?? issueOf.get(slice);
        for (const [k, v] of [...open]) {
          const [kind, s] = k.split("|");
          if (kind !== "implement" || prOfKey.has(s!)) continue;
          if ((v.bar.issue !== undefined && e.closes?.includes(v.bar.issue))
            || (v.lane === author && (v.bar.taskId === e.taskId
              || (issue !== undefined && v.bar.issue === issue)))) {
            v.bar.end = t; v.bar.open = false; v.bar.endedBy = { kind: "merged", actor: e.actorId }; open.delete(k);
          }
        }
        break;
      }
      case "task.closed":
        if (e.pr !== undefined) for (const lane of lanes.values()) lane.awaiting.delete(e.pr);
        for (const s of e.pr !== undefined ? prSlices.get(e.pr) ?? new Set([slice]) : [slice])
          for (const k of ["implement", "review", "merge"] as const) close(k, s, t, e);
        break;
      default: break;
    }
  }
  const from = now - windowMs;
  return [...lanes.entries()]
    .map(([lane, l]) => {
      const bars = l.bars.filter((b) => b.end >= from).map((b) => ({ ...b, start: Math.max(b.start, from) }));
      const owes = l.bars.some((b) => b.open);
      return { lane, bars, lastEventAt: l.last, silent: owes && now - l.last >= LIVENESS_MS, lastRecord: l.record, awaiting: [...l.awaiting] };
    })
    .sort((a, b) => a.lane.localeCompare(b.lane));
}

/** #591: greedy interval packing. Bars sorted by start go into the first sub-row whose last bar
 * ended at or before their start, else a new sub-row; for intervals this uses the minimum number of
 * sub-rows (the largest number of bars open at one instant). Returns each bar's sub-row, in input order. */
export function packBars(bars: readonly { start: number; end: number }[]): { rows: number; row: number[] } {
  const order = bars.map((_, i) => i).sort((a, b) => bars[a]!.start - bars[b]!.start || bars[a]!.end - bars[b]!.end);
  const ends: number[] = [];
  const row = new Array<number>(bars.length).fill(0);
  for (const i of order) {
    const b = bars[i]!;
    let r = ends.findIndex((e) => e <= b.start);
    if (r < 0) { r = ends.length; ends.push(b.end); } else ends[r] = b.end;
    row[i] = r;
  }
  return { rows: Math.max(1, ends.length), row };
}

/** A lane named "TBD" (any case) or nothing is a placeholder, not an agent. */
export const placeholderLane = (name: string | null | undefined) => !name || !name.trim() || name.trim().toLowerCase() === "tbd";
/** #591: the one name filter the Graph shows through: a real name, or null for "TBD" / empty. */
export const realName = (name: string | null | undefined): string | null => (placeholderLane(name) ? null : name!.trim());
/** #591: the real names of a list, placeholders and empties dropped. */
export const realNames = (names: readonly (string | null | undefined)[] | null | undefined): string[] =>
  (names ?? []).map(realName).filter((n): n is string => n !== null);
