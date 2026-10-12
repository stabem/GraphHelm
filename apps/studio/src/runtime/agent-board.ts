import type { Bot } from "./team";
import type { Lane, LaneBar } from "./lane-bars";
import type { MissionTask } from "./mission";
import { ordinal, slotDuration, slotStatus, type SlotView } from "./slots";

/** #591 Lanes board: who is free, who does what, and for how long. */
export type AgentStatus = "silent" | "stale" | "building" | "build_wait" | "working" | "waiting" | "free" | "offline";
export const FREE_LIVENESS_MS = 15 * 60_000;
/** #591: a claim with no PR and no record of its lane for this long is a stale claim, not work. */
export const STALE_CLAIM_MS = 2 * 3_600_000;
export interface AgentRow {
  name: string;
  status: AgentStatus;
  /** The open bar's stage, absent when free. */
  stage: LaneBar["kind"] | null;
  pr: number | null;
  title: string | null;
  /** Link to the PR when the task knows its repository. */
  href: string | null;
  /** #591: ms since the lane's latest record; null when it left none. */
  forMs: number | null;
  /** #591: that record and its age, e.g. "pr_opened #613 · 12m ago"; null when none. */
  latest?: string | null;
  /** ms in the open stage (sorts Working longest first); absent when no bar is open. */
  stageMs?: number;
  lastDelivered: string | null;
  /** Pill text override: "Reviewing" for a lane busy on an open review. */
  label?: string | null;
  /** Full "what" text override, e.g. "reviewing #7 Add board". */
  doing?: string | null;
}

const ORDER: Record<AgentStatus, number> = { silent: 0, stale: 1, building: 2, build_wait: 3, working: 4, waiting: 5, free: 7, offline: 8 };

function latestOf(lane: Lane | undefined, now: number): { latest: string | null; forMs: number | null } {
  const rec = lane?.lastRecord;
  if (!rec) return { latest: null, forMs: null };
  const ms = Math.max(0, now - rec.at);
  const n = rec.pr ?? rec.issue;
  return { latest: `${rec.kind}${n !== undefined ? ` #${n}` : ""} · ${span(ms)} ago`, forMs: ms };
}

function laneFor(b: Bot, lanes: Lane[]): Lane | undefined {
  return lanes.find((l) => l.lane === b.name || l.lane === b.actorId || l.lane === b.key);
}

const blank = (n: string | undefined | null) => !n || n.trim() === "" || n.trim().toLowerCase() === "tbd";

/** A verdict on the task's current head: a BLOCK round on it, or the unanswered BLOCK on it. */
function hasVerdictOnHead(t: MissionTask, names: string[]): boolean {
  if (!t.headSha) return false;
  if (t.rounds.some((r) => names.includes(r.reviewer) && r.headSha === t.headSha)) return true;
  return !!t.blockedBy && names.includes(t.blockedBy.reviewer) && t.blockedBy.headSha === t.headSha;
}

/** One row per bot, plus a row per lane no bot claims; silent first, then working (longest
 * first), then waiting, then free. Silent is the lane-bars STALL rule (`Lane.silent`). */
export function agentBoard(bots: Bot[], lanes: Lane[], tasks: MissionTask[], now: number, slots: SlotView[] = [], onDemand: readonly string[] = [], live: readonly string[] = []): AgentRow[] {
  bots = bots.filter((b) => !blank(b.name));
  lanes = lanes.filter((l) => !blank(l.lane));
  const used = new Set<Lane>();
  const pairs: { name: string; lane: Lane | undefined }[] = bots.map((b) => {
    const lane = laneFor(b, lanes);
    if (lane) used.add(lane);
    return { name: b.name, lane };
  });
  for (const l of lanes) if (!used.has(l)) pairs.push({ name: l.lane, lane: l });
  for (const name of onDemand) if (!blank(name) && !pairs.some((p) => p.name === name || p.lane?.lane === name
    || bots.some((b) => b.name === p.name && (b.key === name || b.actorId === name)))) pairs.push({ name, lane: undefined });
  const delivered = new Map<string, string>();
  for (const t of tasks) if (t.step === "merged" && t.lane) delivered.set(t.lane, `${t.pr ? `#${t.pr} ` : ""}${t.title}`);
  const rows0 = pairs.map(({ name, lane }): AgentRow => {
    const ids = [name, lane?.lane].filter((n): n is string => !!n);
    const when = latestOf(lane, now);
    const quietMs = lane && lane.lastEventAt > 0 ? now - lane.lastEventAt : 0;
    // #591: a PR-less claim whose lane left no record for STALE_CLAIM_MS is not work.
    const staleClaim = (b: LaneBar) => b.kind === "implement" && !b.label.startsWith("#") && quietMs > STALE_CLAIM_MS;
    const allOpen = (lane?.bars ?? []).filter((b) => b.open);
    const awaiting = lane?.awaiting ?? [];
    const endedClaim = (b: LaneBar) => b.kind === "implement" && !b.label.startsWith("#")
      && lane?.endedAt !== undefined && lane.endedAt > (b.since ?? b.start);
    // An implement bar on a PR its lane opened and that waits on review is not writing.
    const openBars = allOpen.filter((b) => !staleClaim(b) && !endedClaim(b)
      && !(b.kind === "implement" && awaiting.some((p) => b.label === `#${p}`)));
    if (!openBars.some((b) => b.kind === "implement")) {
      const rt = tasks.find((t) => t.step !== "merged" && t.step !== "merge" && t.pr !== null
        && !hasVerdictOnHead(t, ids)
        && !!t.currentReviewer && ids.includes(t.currentReviewer));
      if (rt) {
        const last0 = delivered.get(name) ?? (lane ? delivered.get(lane.lane) : undefined) ?? null;
        return {
          name, status: "waiting", stage: "review", pr: rt.pr, title: rt.title, label: "Reviewing", doing: `reviewing #${rt.pr} ${rt.title}`,
          href: rt.repoUrl ? `${rt.repoUrl}/pull/${rt.pr}` : null, lastDelivered: last0, ...when,
        };
      }
    }
    const bar = openBars.reduce<LaneBar | null>((a, b) => (a === null || (b.since ?? b.start) > (a.since ?? a.start) ? b : a), null);
    const last = lane ? delivered.get(lane.lane) ?? delivered.get(name) ?? null : delivered.get(name) ?? null;
    if (!bar) {
      if (awaiting.length > 0) {
        const p = awaiting[awaiting.length - 1]!;
        const task = tasks.find((t) => t.pr === p) ?? null;
        return { name, status: "waiting", stage: null, pr: p, title: task?.title ?? null, label: `awaiting review #${p}`, doing: `awaiting review #${p}${task ? ` ${task.title}` : ""}`,
          href: task?.repoUrl ? `${task.repoUrl}/pull/${p}` : null, lastDelivered: last, ...when };
      }
      const ended = allOpen.find(endedClaim);
      if (ended) return { name, status: "free", stage: null, pr: null, title: null,
        label: "ended without a PR", doing: `ended without a PR · ${ended.issue !== undefined ? `#${ended.issue}` : ended.label}`,
        href: null, lastDelivered: last, ...when };
      const stale = allOpen.find(staleClaim);
      if (stale) {
        const ref = stale.issue !== undefined ? `#${stale.issue}` : stale.label;
        return { name, status: "stale", stage: null, pr: null, title: null, label: `stale claim · ${ref}`, doing: `stale claim · ${ref}`, href: null, lastDelivered: last, ...when };
      }
      return { name, status: "free", stage: null, pr: null, title: null, href: null, lastDelivered: last, ...when };
    }
    const task = tasks.find((t) => (bar.taskId !== undefined && t.key === bar.taskId) || (t.pr !== null && bar.label === `#${t.pr}`)) ?? null;
    const pr = task?.pr ?? (bar.label.startsWith("#") ? Number(bar.label.slice(1)) : null);
    const status: AgentStatus = lane!.silent ? "silent" : bar.kind === "implement" ? "working" : "waiting";
    return {
      name, status, stage: bar.kind, pr, title: task?.title ?? null,
      href: task?.repoUrl && pr ? `${task.repoUrl}/pull/${pr}` : null,
      lastDelivered: last, ...when, stageMs: Math.max(0, now - (bar.since ?? bar.start)),
    };
  });
  // #636: a lane holding or queued for a build slot shows that, whatever its bars say.
  const rows = rows0.map((r, i): AgentRow => {
    const bot = bots.find((b) => b.name === r.name);
    const ids = [r.name, pairs[i]!.lane?.lane, bot?.key, bot?.actorId].filter((n): n is string => !!n);
    const slot = ids.map((n) => slotStatus(n, slots)).find((x) => x !== null) ?? null;
    if (!slot) {
      if (r.status !== "free" || onDemand.some((name) => ids.includes(name))) return r;
      // A wake listener is the live signal; sessions must arm one before this path can identify them.
      if (live.some((name) => ids.includes(name))) return { ...r, latest: "live now · wake listener", forMs: 0 };
      const seen = Math.max(pairs[i]!.lane?.lastEventAt ?? 0,
        ...bots.filter((b) => ids.includes(b.name) || (b.actorId !== null && ids.includes(b.actorId)))
          .map((b) => b.lastRecordAt === null ? 0 : Date.parse(b.lastRecordAt)).filter(Number.isFinite));
      if (seen > 0 && now - seen < FREE_LIVENESS_MS) return r;
      return { ...r, status: "offline", label: `Offline · last seen ${seen > 0 ? `${span(Math.max(0, now - seen))} ago` : "never"}` };
    }
    const source = slots.find((s) => s.ok && s.root === slot.root);
    const label = source?.ok ? slot.kind === "building" ? source.holder?.label : source.waiting.find((w) => ids.includes(w.lane))?.label : null;
    const pr = label && /^#([1-9]\d*)$/.exec(label)?.[1];
    const slotPr = pr ? Number(pr) : null;
    const slotTask = slotPr === null ? null : tasks.find((t) => t.pr === slotPr) ?? null;
    const d = slotDuration(slot.seconds);
    return slot.kind === "building"
      ? { ...r, status: "building", pr: slotPr, title: slotTask?.title ?? null, href: slotTask?.repoUrl && slotPr ? `${slotTask.repoUrl}/pull/${slotPr}` : null,
        label: `Building · ${d}`, doing: `building in ${slot.root}${slotPr ? ` · #${slotPr}` : ""}`, stageMs: slot.seconds * 1000 }
      : { ...r, status: "build_wait", pr: slotPr, title: slotTask?.title ?? null, href: slotTask?.repoUrl && slotPr ? `${slotTask.repoUrl}/pull/${slotPr}` : null,
        label: `Waiting for build · ${ordinal(slot.position)} · ${d}`, doing: `waiting for a build slot in ${slot.root}${slotPr ? ` · #${slotPr}` : ""}`, stageMs: slot.seconds * 1000 };
  });
  const represented = new Set(rows.map((r) => r.name));
  const slotLanes = new Set(slots.flatMap((slot) => slot.ok
    ? [...(slot.holder ? [slot.holder.lane] : []), ...slot.waiting.map((waiting) => waiting.lane)] : []));
  for (const name of slotLanes) {
    if (represented.has(name)) continue;
    const slot = slotStatus(name, slots)!;
    const base = rows0.find((candidate) => candidate.name === name) ?? {
      name, status: "free" as const, stage: null, pr: null, title: null, href: null,
      lastDelivered: delivered.get(name) ?? null, ...latestOf(undefined, now),
    };
    const label = slot.kind === "building" ? `Building · ${slotDuration(slot.seconds)}`
      : `Waiting for build · ${ordinal(slot.position)} · ${slotDuration(slot.seconds)}`;
    rows.push({ ...base, status: slot.kind === "building" ? "building" : "build_wait", label,
      doing: `${slot.kind === "building" ? "building" : "waiting for a build slot"} in ${slot.root}` });
    represented.add(name);
  }
  return rows.sort((a, b) => ORDER[a.status] - ORDER[b.status]
    || (a.status === "working" ? (b.stageMs ?? 0) - (a.stageMs ?? 0) : 0)
    || a.name.localeCompare(b.name));
}

/** `1h 17m`, `23m`, `<1m`. */
export function span(ms: number): string {
  const m = Math.floor(ms / 60_000);
  if (m < 1) return "<1m";
  const h = Math.floor(m / 60);
  return h > 0 ? `${h}h ${m % 60}m` : `${m}m`;
}
