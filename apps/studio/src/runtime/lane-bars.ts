import type { TaskEventRecord } from "./team-tasks";

export type TimedTaskEvent = TaskEventRecord & { at: string };
export type BarKind = "implement" | "review" | "merge";
/** `taskId` names the task the bar works on; `since` is the unclipped start (`start` is clipped to the window). */
export interface LaneBar {
  kind: BarKind; label: string; start: number; end: number; open: boolean; taskId?: string; since?: number;
  /** #591: the issue a claim names; the head a review was assigned on. */
  issue?: number; headSha?: string;
}
/** #591: the newest record a lane left: its kind without the `task.` prefix, and its PR or issue. */
export interface LaneRecord { kind: string; pr?: number; issue?: number; at: number }
export interface Lane {
  lane: string; bars: LaneBar[]; silent: boolean; lastEventAt: number;
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
  // #591: a bar belongs to one slice of its task, as foldTaskEvents keys it: its PR once one is
  // recorded, else its claim. Keying by taskId let one slice's merge or a later claim close another
  // slice's open review.
  const claims = new Map<string, { key: string; lane?: string; sequence: number; slice: string; pr: boolean }[]>();
  const known = new Set<string>();
  const latest = new Map<string, string>();
  const prOfKey = new Map<string, number>();
  // #591: a PR is one slice whatever task id its records carry (a merge record may name the PR's own task).
  const prKey = new Map<number, string>();
  const headOf = new Map<string, string>();
  const authorOf = new Map<string, string>();
  const issueOf = new Map<string, number>();
  const laneOf = (name: string) => {
    let l = lanes.get(name);
    if (!l) { l = { bars: [], last: 0, record: null, awaiting: new Set() }; lanes.set(name, l); }
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
      list.push({ key, lane: e.lane, sequence: e.sequence, slice: key, pr: false });
      claims.set(e.taskId, list);
    } else if (e.pr !== undefined) {
      key = prKey.get(e.pr) ?? `${e.taskId}#pr-${e.pr}`;
      prKey.set(e.pr, key);
      prOfKey.set(key, e.pr);
      if (!known.has(key)) {
        // A PR no slice holds yet joins the oldest open claim of its task (the same lane's, when named).
        const list = claims.get(e.taskId) ?? [];
        const i = list.findIndex((c) => !c.pr && (e.lane === undefined || c.lane === undefined || c.lane === e.lane));
        if (i >= 0) {
          const claim = list[i]!;
          claim.pr = true;
          rekey(claim.key, key, `#${e.pr}`);
          claim.slice = key;
          if (claim.lane) authorOf.set(key, claim.lane);
          const issue = issueOf.get(claim.key);
          if (issue !== undefined) issueOf.set(key, issue);
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
    const issue = issueOf.get(slice);
    if (issue !== undefined) bar.issue = issue;
    if (kind === "review" && headOf.has(slice)) bar.headSha = headOf.get(slice);
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
    if (e.headSha !== undefined && (e.kind === "task.pr_opened" || e.kind === "task.claimed")) headOf.set(slice, e.headSha);
    if (e.lane && (e.kind === "task.claimed" || e.kind === "task.pr_opened")) authorOf.set(slice, e.lane);
    const pr = prOfKey.get(slice);
    const author = authorOf.get(slice);
    switch (e.kind) {
      case "task.claimed": if (e.lane) start(e.lane, "implement", e.taskId, slice, t); break;
      case "task.pr_opened": if (author && pr !== undefined) laneOf(author).awaiting.add(pr); break;
      case "task.released":
        if (e.lane) {
          const claim = claims.get(e.taskId)?.find((candidate) => candidate.sequence === e.claimSequence && candidate.lane === e.lane && !candidate.pr);
          if (claim) close("implement", claim.slice, t, e.lane);
        }
        break;
      case "task.review_assigned": close("implement", slice, t); if (e.reviewer) start(e.reviewer, "review", e.taskId, slice, t); break;
      case "task.review_verdict":
        close("review", slice, t, e.reviewer);
        if (author && pr !== undefined) laneOf(author).awaiting.delete(pr);
        if (e.reviewer && String(e.verdict).startsWith("APPROVE")) start(e.reviewer, "merge", e.taskId, slice, t);
        break;
      case "task.merged": {
        for (const k of ["implement", "review", "merge"] as const) close(k, slice, t);
        if (author && pr !== undefined) laneOf(author).awaiting.delete(pr);
        // #591: only issue closure ends PR-less claims across lanes. A Refs slice ends only
        // its author's matching claims; other PR slices stay open.
        const issue = e.issue ?? issueOf.get(slice);
        for (const [k, v] of [...open]) {
          const [kind, s] = k.split("|");
          if (kind !== "implement" || !s!.includes("#claim-")) continue;
          if ((v.bar.issue !== undefined && e.closes?.includes(v.bar.issue))
            || (v.lane === author && (s!.startsWith(`${e.taskId}#`)
              || (issue !== undefined && v.bar.issue === issue)))) {
            v.bar.end = t; v.bar.open = false; open.delete(k);
          }
        }
        break;
      }
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
