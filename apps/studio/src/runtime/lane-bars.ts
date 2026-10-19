import type { TaskEventRecord } from "./team-tasks";
import type { LaneFold, LaneLiveness } from "./tasks-route";
import type { RuntimeEvent } from "./types";
import { sameHead } from "./heads";

export type TimedTaskEvent = TaskEventRecord & { at: string };
export type BarKind = "implement" | "review" | "merge";
/** `taskId` names the task the bar works on; `since` is the unclipped start (`start` is clipped to the window). */
export interface LaneBar {
  kind: BarKind; label: string; start: number; end: number; open: boolean; taskId?: string; since?: number;
  lastActivityAt?: number; endedBy?: { kind: string; actor: string };
  quietSpans?: { start: number; end: number }[];
  displayLabel?: string;
  /** #591: the issue a claim names; the head a review was assigned on. */
  issue?: number; headSha?: string;
}
/** #591: the newest record a lane left: its kind without the `task.` prefix, and its PR or issue. */
export interface LaneRecord {
  kind: string; pr?: number; issue?: number; at: number;
  /** #1556: a heartbeat's stated reason, and when the lane first gave this same reason in an
   * unbroken run of heartbeats (any other step ends the run). */
  reason?: string; reasonSince?: number;
}
export interface Lane {
  /** #1234: `silent` is the Runtime's verdict (`liveness.lanes`), not a threshold kept here. */
  lane: string; bars: LaneBar[]; silent: boolean; lastEventAt: number;
  /** #1234: the budgets the Runtime judged by, in ms; absent when it served none (an older
   * Runtime), and then nothing is drawn quiet and nobody counts as recently seen. */
  silentAfterMs?: number; freeAfterMs?: number;
  /** #1193/#1234: the tasks whose claim lease the Runtime reads as overdue: stale claims. */
  overdueTasks?: string[];
  /** #1281: when the Runtime starts this lane's quiet, `max(lastRecordAt, owesSince)` in ms; the
   * Stalled age is `now` minus this. Absent when the Runtime judged nothing for the lane. */
  quietSinceAt?: number;
  /** Latest observed session end for this lane. */
  endedAt?: number;
  lastRecord?: LaneRecord | null;
  /** #591: PRs this lane opened that wait on a review: no verdict on their newest head, not merged. */
  awaiting?: number[];
}

/** #1281: the instant the Runtime measures a lane's quiet from: the later of its last record and
 * its oldest open obligation, the same pair `lane_liveness` takes. */
function quietSince(row: LaneLiveness | undefined): { quietSinceAt?: number } {
  const times = [row?.lastRecordAt, row?.owesSince].map((at) => (at ? Date.parse(at) : NaN)).filter(Number.isFinite);
  return times.length ? { quietSinceAt: Math.max(...times) } : {};
}

/** #1234: the budgets a set of lanes was judged by (the same for every lane of one run). */
export function laneBudgets(lanes: readonly Lane[]): { silentAfterMs: number; freeAfterMs: number } {
  const judged = lanes.find((lane) => lane.silentAfterMs !== undefined);
  return { silentAfterMs: judged?.silentAfterMs ?? Number.POSITIVE_INFINITY, freeAfterMs: judged?.freeAfterMs ?? 0 };
}
/** #1192: the bars are drawn from the Runtime's fold (`GET /v1/executions/{id}/tasks`): `fold`
 * carries every slice the Runtime created, live or not, and the slice each record landed in. No
 * fold runs in the browser; a record the Runtime routed nowhere draws nothing. */
export function laneBars(events: TimedTaskEvent[], fold: LaneFold, now: number, windowMs: number): Lane[] {
  // #1234: the Runtime's budgets and verdicts; without them nothing is drawn quiet or silent.
  const silentAfterMs = fold.liveness ? fold.liveness.budgets.silentAfterSeconds * 1000 : undefined;
  const freeAfterMs = fold.liveness ? fold.liveness.budgets.freeAfterSeconds * 1000 : undefined;
  const quietAfter = silentAfterMs ?? Number.POSITIVE_INFINITY;
  const verdictOf = new Map((fold.liveness?.lanes ?? []).map((row) => [row.lane, row] as const));
  const overdueOf = new Map<string, string[]>();
  for (const lease of fold.leases ?? []) if (lease.overdue) overdueOf.set(lease.lane, [...(overdueOf.get(lease.lane) ?? []), lease.taskId]);
  const lanes = new Map<string, { bars: LaneBar[]; last: number; record: LaneRecord | null; awaiting: Set<number> }>();
  const open = new Map<string, { lane: string; bar: LaneBar; slice: string }>();
  // Slice identity is the Runtime's id, stable across the slices it later closed or absorbed.
  const routed = new Map<number, string>(fold.routes.map((route) => [route.sequence, `slice-${route.slice}`]));
  const keys = new Map(fold.slices.map((slice) => [slice.state, `slice-${slice.id}`] as const));
  const prOfKey = new Map<string, number>();
  const prSlices = new Map<number, Set<string>>();
  const titleOf = new Map<string, string>();
  const headOf = new Map<string, string>();
  const authorOf = new Map<string, string>();
  const issueOf = new Map<string, number>();
  const claimSequenceOf = new Map<string, number>();
  const absorbedBars = new Map<number, LaneBar[]>();
  // Read titles from the same completed slices that feed the kanban. Older records may
  // carry a title only on the claim or PR opening, not on later timeline records.
  keys.forEach((slice, state) => {
    if (state.title || state.prTitle) titleOf.set(slice, state.prTitle ?? state.title!);
  });
  const mergedPrs = new Set<number>();
  const laneOf = (name: string) => {
    let l = lanes.get(name);
    if (!l) { l = { bars: [], last: 0, record: null, awaiting: new Set() }; lanes.set(name, l); }
    return l;
  };
  const start = (lane: string, kind: BarKind, taskId: string, slice: string, t: number) => {
    const k = [kind, slice, lane].join("|");
    const prev = open.get(k);
    if (prev) { finish(prev.bar, t); prev.bar.endedBy = { kind: "claimed", actor: lane }; open.delete(k); }
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
  const finish = (bar: LaneBar, t: number) => {
    const quietStart = (bar.lastActivityAt ?? bar.since ?? bar.start) + quietAfter;
    if (quietStart < t) (bar.quietSpans ??= []).push({ start: quietStart, end: t });
    bar.end = t;
    bar.open = false;
  };
  const close = (kind: BarKind, slice: string, t: number, event: TimedTaskEvent, lane?: string) => {
    for (const [k, v] of [...open]) {
      if (k.startsWith(`${kind}|${slice}|`) && (!lane || v.lane === lane)) {
        finish(v.bar, t); v.bar.endedBy = { kind: event.kind.replace(/^task\./, ""), actor: event.actorId }; open.delete(k);
      }
    }
  };
  const closePr = (pr: number, t: number, event: TimedTaskEvent, kind?: BarKind, head?: string, onlySlice?: string) => {
    for (const [key, value] of [...open]) {
      if (prOfKey.get(value.slice) !== pr || (onlySlice && value.slice !== onlySlice) || (kind && value.bar.kind !== kind)
        || (head && !sameHead(value.bar.headSha, head))) continue;
      value.bar.end = t; value.bar.open = false; value.bar.endedBy = { kind: event.kind.replace(/^task\./, ""), actor: event.actorId }; open.delete(key);
    }
  };
  for (const e of [...events].sort((a, b) => a.sequence - b.sequence)) {
    const t = Date.parse(e.at);
    if (!Number.isFinite(t)) continue;
    // #1424: a retirement routes to no slice; it ends every bar the retired lanes hold and every
    // bar on a slice they authored (a live reviewer's review of a retired author's PR).
    if (e.kind === "task.lane_retired") {
      if (e.retired) for (const [k, v] of [...open]) {
        if (!e.lanes?.includes(v.lane) && !e.lanes?.includes(authorOf.get(v.slice) ?? "")) continue;
        finish(v.bar, t); v.bar.endedBy = { kind: "lane_retired", actor: e.actorId }; open.delete(k);
      }
      continue;
    }
    const slice = routed.get(e.sequence);
    if (slice === undefined) continue;
    // Ending records describe the boundary, not continuous work in the gap before it.
    if (e.kind === "task.planned" || e.kind === "task.pr_opened" || e.kind === "task.critic_verdict") {
      for (const v of open.values()) if (v.slice === slice && v.lane === e.actorId) {
        const last = v.bar.lastActivityAt ?? v.bar.since ?? v.bar.start;
        if (last + quietAfter < t) (v.bar.quietSpans ??= []).push({ start: last + quietAfter, end: t });
        v.bar.lastActivityAt = Math.max(last, t);
      }
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
      // #1556 (review): the Stop hook's reasonless heartbeat is not a step and never breaks or
      // replaces a stated wait, so a forgotten wait still reaches the 6 h marker.
      if (e.kind === "task.heartbeat" && e.reason === undefined) continue;
      if (t >= l.last) {
        l.last = t;
        const rec: LaneRecord = { kind: e.kind.replace(/^task\./, ""), at: t };
        const pr = e.pr ?? prOfKey.get(slice);
        if (pr !== undefined) rec.pr = pr;
        if (e.issue !== undefined) rec.issue = e.issue;
        if (e.kind === "task.heartbeat" && e.reason !== undefined) {
          const same = l.record?.kind === "heartbeat" && l.record.reason === e.reason && l.record.reasonSince !== undefined;
          rec.reason = e.reason;
          rec.reasonSince = same ? l.record!.reasonSince : t;
        }
        l.record = rec;
      }
    }
    if (e.issue !== undefined) issueOf.set(slice, e.issue);
    if (e.kind === "task.claimed") claimSequenceOf.set(slice, e.sequence);
    if (typeof e.title === "string" && !titleOf.has(slice)) {
      titleOf.set(slice, e.title);
      for (const v of open.values()) if (v.slice === slice) v.bar.displayLabel = `${e.pr !== undefined ? `#${e.pr}` : e.issue !== undefined ? `#${e.issue}` : e.taskId} ${e.title}`;
    }
    if (e.headSha !== undefined && (e.kind === "task.pr_opened" || e.kind === "task.claimed")) {
      if (e.kind === "task.pr_opened" && headOf.has(slice) && !sameHead(headOf.get(slice), e.headSha)) close("review", slice, t, e);
      headOf.set(slice, e.headSha);
    }
    if (e.lane && (e.kind === "task.claimed" || e.kind === "task.pr_opened")) authorOf.set(slice, e.lane);
    if (e.kind === "task.pr_opened" && e.pr !== undefined && e.lane) {
      const opening = events.find((candidate) => candidate.kind === "task.pr_opened" && candidate.pr === e.pr
        && candidate.lane === e.lane && candidate.taskId === e.taskId && candidate.sequence < e.sequence);
      const firstOpening = opening?.sequence ?? e.sequence;
      for (const value of open.values()) {
        const claimedAt = claimSequenceOf.get(value.slice);
        if (value.lane === e.lane && value.bar.kind === "implement" && value.bar.taskId === e.taskId
          && claimedAt !== undefined && claimedAt > firstOpening && claimedAt < e.sequence) {
          value.bar.label = `#${e.pr}`;
          const bars = absorbedBars.get(e.pr) ?? [];
          bars.push(value.bar);
          absorbedBars.set(e.pr, bars);
          open.delete([value.bar.kind, value.slice, value.lane].join("|"));
        }
      }
    }
    const pr = prOfKey.get(slice);
    const author = authorOf.get(slice);
    // A merge is terminal for this PR, even if a delayed record arrives later.
    if (pr !== undefined && mergedPrs.has(pr)) continue;
    switch (e.kind) {
      case "task.claimed": if (e.lane) start(e.lane, "implement", e.taskId, slice, t); break;
      case "task.pr_opened": if (author && pr !== undefined) laneOf(author).awaiting.add(pr); break;
      case "task.released":
        // An owner release by key ends the whole slice, whoever holds its bars.
        if (e.sliceKey !== undefined) for (const k of ["implement", "review", "merge"] as const) close(k, slice, t, e);
        else close("implement", slice, t, e, e.lane);
        break;
      case "task.review_assigned":
        close("implement", slice, t, e);
        if (e.headSha !== undefined && sameHead(e.headSha, headOf.get(slice))) close("review", slice, t, e);
        if (e.reviewer) start(e.reviewer, "review", e.taskId, slice, t);
        break;
      case "task.review_verdict":
        if (e.pr !== undefined && e.headSha !== undefined) closePr(e.pr, t, e, "review", e.headSha, slice);
        if (e.headSha !== undefined && headOf.get(slice) !== undefined && sameHead(e.headSha, headOf.get(slice))) {
          if (e.pr === undefined) close("review", slice, t, e, e.reviewer);
          if (author && pr !== undefined) laneOf(author).awaiting.delete(pr);
        }
        break;
      case "task.merged": {
        if (e.pr !== undefined) mergedPrs.add(e.pr);
        if (e.pr !== undefined) for (const lane of lanes.values()) lane.awaiting.delete(e.pr);
        if (e.pr !== undefined) for (const bar of absorbedBars.get(e.pr) ?? []) {
          finish(bar, t);
          bar.endedBy = { kind: "merged", actor: e.actorId };
        }
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
            finish(v.bar, t); v.bar.endedBy = { kind: "merged", actor: e.actorId }; open.delete(k);
          }
        }
        break;
      }
      case "task.closed":
        if (e.pr !== undefined) for (const lane of lanes.values()) lane.awaiting.delete(e.pr);
        if (e.pr !== undefined) for (const bar of absorbedBars.get(e.pr) ?? []) {
          finish(bar, t);
          bar.endedBy = { kind: "closed", actor: e.actorId };
        }
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
      return {
        lane, bars, lastEventAt: l.last, silent: verdictOf.get(lane)?.silent ?? false, lastRecord: l.record, awaiting: [...l.awaiting],
        ...quietSince(verdictOf.get(lane)),
        ...(silentAfterMs !== undefined ? { silentAfterMs } : {}), ...(freeAfterMs !== undefined ? { freeAfterMs } : {}),
        ...(overdueOf.has(lane) ? { overdueTasks: overdueOf.get(lane)! } : {}),
      };
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

/** #1184: the latest `agent_session_ended` signed as a lane sets that lane's `endedAt`. The hooks
 * sign a lane's session end as the lane (the same name its task records carry); an end signed as
 * `{host}-session-{id}` names no lane and is left alone. Mutates and returns `lanes`. */
export function applySessionEnds(lanes: Lane[], events: readonly RuntimeEvent[]): Lane[] {
  for (const event of events) {
    if (event.kind !== "signal_recorded" || !event.payload || typeof event.payload !== "object"
      || !("kind" in event.payload) || event.payload.kind !== "agent_session_ended" || !event.actorId) continue;
    const lane = lanes.find((row) => row.lane === event.actorId);
    const at = event.occurredAt ? Date.parse(event.occurredAt) : NaN;
    if (lane && Number.isFinite(at) && (lane.endedAt === undefined || at > lane.endedAt)) lane.endedAt = at;
  }
  return lanes;
}

/** A lane named "TBD" (any case) or nothing is a placeholder, not an agent. */
export const placeholderLane = (name: string | null | undefined) => !name || !name.trim() || name.trim().toLowerCase() === "tbd";
/** #591: the one name filter the Graph shows through: a real name, or null for "TBD" / empty. */
export const realName = (name: string | null | undefined): string | null => (placeholderLane(name) ? null : name!.trim());
/** #591: the real names of a list, placeholders and empties dropped. */
export const realNames = (names: readonly (string | null | undefined)[] | null | undefined): string[] =>
  (names ?? []).map(realName).filter((n): n is string => n !== null);
